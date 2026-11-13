//! The roast archive. Roasts live in the database (ADR 0003); each also has
//! an open copy on disk, `<title>.roast.json` (ADR 0008) with an Artisan
//! `.alog` next to it. Besides: the autosave of the running recording.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use chrono::{DateTime, Utc};
use roast_core::{EventKind, ROAST_FILE_SCHEMA_VERSION, Roast};
use roast_store::Store;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Folder inside the roasts folder that deleted roasts move to.
pub const TRASH_DIR: &str = "trash";
const AUTOSAVE_NAME: &str = ".recording.roast.json";
const JSON_EXT: &str = ".roast.json";

/// The `.roast.json` file: the roast plus a schema version.
#[derive(Debug, Serialize, Deserialize)]
pub struct RoastFile {
    /// Format version.
    pub schema_version: u32,
    /// The roast.
    #[serde(flatten)]
    pub roast: Roast,
}

/// One line of the archive listing.
#[derive(Debug, Clone, Serialize)]
pub struct Summary {
    id: uuid::Uuid,
    title: String,
    roasted_at: DateTime<Utc>,
    total_s: Option<f64>,
    drop_bt: Option<f64>,
    green_weight_g: Option<f64>,
    roasted_weight_g: Option<f64>,
    simulated: bool,
    /// Bean name (the lot's, or free text from an import).
    beans: Option<String>,
    lot_id: Option<uuid::Uuid>,
    /// First crack, seconds after CHARGE.
    first_crack_s: Option<f64>,
    /// Development time ratio at the drop, percent.
    dtr_pct: Option<f64>,
    goal: Option<roast_core::RoastGoal>,
    /// The batch number on the bag's label: the roast's own, or for an
    /// imported Artisan file the one its name starts with (`BR011_…`).
    batch_no: Option<String>,
    /// The profile version the roast followed.
    profile_version_id: Option<uuid::Uuid>,
}

/// What the roasts of a group have in common, for statistics over them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Group {
    /// Roasts of one green coffee lot.
    Lot(Uuid),
    /// Roasts that followed one of these profile versions.
    Versions(Vec<Uuid>),
    /// These roasts.
    Ids(Vec<Uuid>),
}

/// The most roasts a group is read for; the newest win.
pub const GROUP_MAX: usize = 60;

/// Opens a file for appending, creating it and its directory.
pub fn open_append(path: &Path) -> io::Result<File> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    OpenOptions::new().create(true).append(true).open(path)
}

pub(crate) fn write_atomic(path: &Path, contents: &[u8]) -> io::Result<()> {
    let tmp = path.with_extension("tmp");
    {
        let mut f = File::create(&tmp)?;
        f.write_all(contents)?;
        f.sync_all()?;
    }
    fs::rename(tmp, path)
}

fn to_json(roast: &Roast) -> io::Result<Vec<u8>> {
    let file = RoastFile {
        schema_version: ROAST_FILE_SCHEMA_VERSION,
        roast: roast.clone(),
    };
    serde_json::to_vec_pretty(&file).map_err(io::Error::other)
}

fn slug(title: &str) -> String {
    let mut s = String::new();
    for c in title.chars() {
        if c.is_alphanumeric() {
            s.extend(c.to_lowercase());
        } else if !s.ends_with('-') {
            s.push('-');
        }
    }
    let s = s.trim_matches('-');
    s.chars().take(60).collect()
}

/// The base name (without extension) for a new roast file; it starts with
/// the batch number when the roast has one, as the roaster names files.
fn base_name(roast: &Roast) -> String {
    let local = roast.roasted_at.with_timezone(&chrono::Local);
    let base = format!("{}_{}", local.format("%Y-%m-%d_%H%M"), slug(&roast.title));
    match roast.batch.batch_no.as_deref().map(slug) {
        Some(b) if !b.is_empty() => format!("{}_{base}", b.to_uppercase()),
        _ => base,
    }
}

/// The batch number a file name starts with, as in `BR011_yellow_bourbon.alog`
/// or `BRYB-002_…`: two to six letters, an optional dash, one to four digits.
fn batch_no_of_file_name(name: &str) -> Option<String> {
    let head = name.split(['_', ' ', '.']).next()?;
    let letters = head.chars().take_while(char::is_ascii_alphabetic).count();
    let rest = head[letters..]
        .strip_prefix('-')
        .unwrap_or(&head[letters..]);
    let digits_ok = (1..=4).contains(&rest.len()) && rest.chars().all(|c| c.is_ascii_digit());
    ((2..=6).contains(&letters) && digits_ok).then(|| head.to_ascii_uppercase())
}

/// The batch number of a roast for the archive.
fn batch_no(roast: &Roast) -> Option<String> {
    roast
        .batch
        .batch_no
        .clone()
        .or_else(|| match &roast.origin {
            roast_core::RoastOrigin::ImportedAlog { file_name, .. } => {
                batch_no_of_file_name(file_name)
            }
            _ => None,
        })
}

/// Moves a roast's files (`.roast.json` and `.alog`) into `trash/` beside
/// them: out of the archive, but not gone. Returns the new `.roast.json` path.
fn trash(json: &Path) -> io::Result<PathBuf> {
    let dir = json
        .parent()
        .ok_or_else(|| io::Error::other("a roast file without a folder"))?
        .join(TRASH_DIR);
    fs::create_dir_all(&dir)?;
    let alog = alog_path(json);
    let name = json
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| io::Error::other("a roast file name that is not text"))?;
    let stem = name.trim_end_matches(JSON_EXT);
    let mut target = dir.join(name);
    let mut n = 2;
    while target.exists() {
        target = dir.join(format!("{stem}-{n}{JSON_EXT}"));
        n += 1;
    }
    fs::rename(json, &target)?;
    if alog.exists() {
        fs::rename(&alog, alog_path(&target))?;
    }
    Ok(target)
}

/// Writes a roast as `.roast.json` and `.alog`; returns both paths.
fn save(dir: &Path, roast: &Roast) -> io::Result<(PathBuf, PathBuf)> {
    fs::create_dir_all(dir)?;
    let base = base_name(roast);
    let mut json = dir.join(format!("{base}{JSON_EXT}"));
    let mut n = 2;
    while json.exists() && read(&json).ok().is_none_or(|r| r.id != roast.id) {
        json = dir.join(format!("{base}-{n}{JSON_EXT}"));
        n += 1;
    }
    let alog = alog_path(&json);
    write_atomic(&json, &to_json(roast)?)?;
    write_atomic(&alog, roast_alog::write_alog(roast).as_bytes())?;
    Ok((json, alog))
}

/// What importing one file did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Imported {
    /// Saved as this roast.
    Saved(Uuid),
    /// The archive already holds this roast (same time, same curve length).
    AlreadyThere(Uuid),
}

/// Rewrites an existing roast's files in place.
fn overwrite(json: &Path, roast: &Roast) -> io::Result<()> {
    write_atomic(json, &to_json(roast)?)?;
    write_atomic(&alog_path(json), roast_alog::write_alog(roast).as_bytes())
}

fn alog_path(json: &Path) -> PathBuf {
    let name = json
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("roast.roast.json");
    json.with_file_name(format!("{}.alog", name.trim_end_matches(JSON_EXT)))
}

/// Reads a `.roast.json` file.
fn read(path: &Path) -> io::Result<Roast> {
    let bytes = fs::read(path)?;
    let file: RoastFile = serde_json::from_slice(&bytes).map_err(io::Error::other)?;
    Ok(file.roast)
}

fn autosave(dir: &Path, roast: &Roast) -> io::Result<()> {
    fs::create_dir_all(dir)?;
    write_atomic(&dir.join(AUTOSAVE_NAME), &to_json(roast)?)
}

/// Removes the autosave after a successful save.
fn clear_autosave(dir: &Path) {
    let _ = fs::remove_file(dir.join(AUTOSAVE_NAME));
}

/// The `.roast.json` files in the roasts folder (not the autosave, not the trash).
fn roast_files(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.ends_with(JSON_EXT) && !n.starts_with('.'))
        })
        .collect();
    out.sort();
    out
}

fn name_of(path: &Path) -> Option<String> {
    path.file_name().and_then(|n| n.to_str()).map(str::to_owned)
}

fn db(e: roast_store::Error) -> io::Error {
    io::Error::other(e)
}

/// Bumped whenever [`Summary`] changes: the stored summaries are then
/// written anew on the next start.
const SUMMARY_VERSION: u32 = 2;
const SUMMARY_SETTING: &str = "summary_version";

fn summary_json(roast: &Roast) -> serde_json::Value {
    serde_json::to_value(summary(roast)).unwrap_or_default()
}

/// The roast archive: the database with each roast's open file copy in the
/// roasts folder.
#[derive(Debug, Clone)]
pub struct Archive {
    store: Arc<Store>,
    dir: PathBuf,
}

impl Archive {
    /// The archive in `store` with its file copies in `dir`.
    #[must_use]
    pub fn new(store: Arc<Store>, dir: PathBuf) -> Self {
        Archive { store, dir }
    }

    /// The roasts folder.
    #[must_use]
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Takes roast files the database does not know into it: all of them on
    /// the first start with the database, later a file put into the folder
    /// by hand (or moved back from `trash/`). Returns how many were added.
    ///
    /// # Errors
    /// On a database error; a file that cannot be read is skipped.
    pub fn sync(&self) -> io::Result<usize> {
        if self.store.setting::<u32>(SUMMARY_SETTING).map_err(db)? != Some(SUMMARY_VERSION) {
            self.store.resummarise(summary_json).map_err(db)?;
            self.store
                .set_setting(SUMMARY_SETTING, &SUMMARY_VERSION)
                .map_err(db)?;
        }
        let known = self.store.roast_file_names().map_err(db)?;
        let mut added = 0;
        for path in roast_files(&self.dir) {
            let Some(name) = name_of(&path) else { continue };
            if known.contains(&name) {
                continue;
            }
            let roast = match read(&path) {
                Ok(r) => r,
                Err(e) => {
                    tracing::warn!("{} is not a readable roast file: {e}", path.display());
                    continue;
                }
            };
            if self.store.roast(roast.id).map_err(db)?.is_some() {
                continue;
            }
            self.store
                .save_roast(&roast, &summary_json(&roast), Some(&name))
                .map_err(db)?;
            added += 1;
        }
        Ok(added)
    }

    /// The archive listing, newest first.
    ///
    /// # Errors
    /// On a database error.
    pub fn summaries(&self) -> io::Result<Vec<serde_json::Value>> {
        self.store.roast_summaries().map_err(db)
    }

    /// The roast with this id.
    ///
    /// # Errors
    /// On a database error.
    pub fn find(&self, id: Uuid) -> io::Result<Option<Roast>> {
        Ok(self.store.roast(id).map_err(db)?.map(|s| s.roast))
    }

    /// The roasts of a group, oldest first, at most [`GROUP_MAX`] (the
    /// newest). Simulator roasts only when `simulated` is set or they are
    /// named by id.
    ///
    /// # Errors
    /// On a database error.
    pub fn group(&self, group: &Group, simulated: bool) -> io::Result<Vec<Roast>> {
        let id_of = |v: &serde_json::Value, key: &str| {
            v.get(key)
                .and_then(serde_json::Value::as_str)
                .and_then(|s| s.parse::<Uuid>().ok())
        };
        let mut roasts = Vec::new();
        // The listing is newest first.
        for line in self.summaries()? {
            let Some(id) = id_of(&line, "id") else {
                continue;
            };
            let sim = line.get("simulated").and_then(serde_json::Value::as_bool) == Some(true);
            let member = match group {
                Group::Lot(lot) => id_of(&line, "lot_id") == Some(*lot) && (simulated || !sim),
                Group::Versions(versions) => {
                    id_of(&line, "profile_version_id").is_some_and(|v| versions.contains(&v))
                        && (simulated || !sim)
                }
                Group::Ids(ids) => ids.contains(&id),
            };
            if !member {
                continue;
            }
            if let Some(roast) = self.find(id)? {
                roasts.push(roast);
            }
            if roasts.len() == GROUP_MAX {
                break;
            }
        }
        roasts.reverse();
        Ok(roasts)
    }

    /// Saves a new roast: its files, then the database. Returns the paths
    /// of the `.roast.json` and the `.alog`.
    ///
    /// # Errors
    /// When the files or the database cannot be written.
    pub fn save(&self, roast: &Roast) -> io::Result<(PathBuf, PathBuf)> {
        let (json, alog) = save(&self.dir, roast)?;
        self.store
            .save_roast(roast, &summary_json(roast), name_of(&json).as_deref())
            .map_err(db)?;
        Ok((json, alog))
    }

    /// Saves a changed roast: its files in place, then the database.
    ///
    /// # Errors
    /// When the files or the database cannot be written.
    pub fn update(&self, roast: &Roast) -> io::Result<()> {
        let file = self
            .store
            .roast(roast.id)
            .map_err(db)?
            .and_then(|s| s.file_name)
            .map(|n| self.dir.join(n))
            .filter(|p| p.exists());
        let name = match file {
            Some(path) => {
                overwrite(&path, roast)?;
                None
            }
            // The copy is gone: write a fresh one.
            None => name_of(&save(&self.dir, roast)?.0),
        };
        self.store
            .save_roast(roast, &summary_json(roast), name.as_deref())
            .map_err(db)
    }

    /// Deletes a roast: its files move into `trash/` in the roasts folder
    /// (out of the archive, but not gone), its row leaves the database.
    /// Returns where the `.roast.json` went.
    ///
    /// # Errors
    /// When the files cannot be moved or the database cannot be written.
    pub fn trash(&self, id: Uuid) -> io::Result<Option<PathBuf>> {
        let file = self
            .store
            .roast(id)
            .map_err(db)?
            .and_then(|s| s.file_name)
            .map(|n| self.dir.join(n))
            .filter(|p| p.exists());
        let moved = file.map(|p| trash(&p)).transpose()?;
        self.store.delete_roast(id).map_err(db)?;
        Ok(moved)
    }

    /// Imports an Artisan `.alog` file; see [`Archive::import_alog_bytes`].
    ///
    /// # Errors
    /// When the file cannot be read or is not an Artisan profile.
    pub fn import_alog(&self, alog: &Path) -> io::Result<Imported> {
        let name = name_of(alog).unwrap_or_default();
        self.import_alog_bytes(&name, &fs::read(alog)?)
    }

    /// Imports the content of an Artisan `.alog` called `file_name` (an
    /// upload from the app): the roast into the archive, the original
    /// unchanged beside its `.roast.json`. The same roast again does
    /// nothing. Content that is not valid UTF-8 is read as Latin-1.
    ///
    /// # Errors
    /// When the content is not an Artisan profile or cannot be saved.
    pub fn import_alog_bytes(&self, file_name: &str, bytes: &[u8]) -> io::Result<Imported> {
        let text = match std::str::from_utf8(bytes) {
            Ok(text) => text.to_owned(),
            Err(_) => bytes.iter().copied().map(char::from).collect(),
        };
        let mut roast = roast_alog::read_alog(&text).map_err(io::Error::other)?;
        // Only the name: an upload must not choose where its copy is written.
        let file_name = Path::new(file_name)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        if let roast_core::RoastOrigin::ImportedAlog { file_name: f, .. } = &mut roast.origin {
            f.clone_from(&file_name);
        }
        if roast.title.trim().is_empty() {
            roast.title = Path::new(&file_name)
                .file_stem()
                .and_then(|s| s.to_str())
                .filter(|s| !s.is_empty())
                .unwrap_or("Imported roast")
                .to_owned();
        }
        if let Some(id) = self
            .store
            .same_roast(roast.roasted_at, roast.samples.len())
            .map_err(db)?
        {
            return Ok(Imported::AlreadyThere(id));
        }
        fs::create_dir_all(&self.dir)?;
        let base = base_name(&roast);
        let mut json = self.dir.join(format!("{base}{JSON_EXT}"));
        let mut n = 2;
        while json.exists() {
            json = self.dir.join(format!("{base}-{n}{JSON_EXT}"));
            n += 1;
        }
        write_atomic(&json, &to_json(&roast)?)?;
        write_atomic(&alog_path(&json), bytes)?;
        self.store
            .save_roast(&roast, &summary_json(&roast), name_of(&json).as_deref())
            .map_err(db)?;
        Ok(Imported::Saved(roast.id))
    }

    /// Writes the autosave of the running recording.
    ///
    /// # Errors
    /// When the file cannot be written.
    pub fn autosave(&self, roast: &Roast) -> io::Result<()> {
        autosave(&self.dir, roast)
    }

    /// Removes the autosave (the recording was saved).
    pub fn clear_autosave(&self) {
        clear_autosave(&self.dir);
    }

    /// Turns a leftover autosave (the daemon stopped during a recording)
    /// into a normal roast.
    ///
    /// # Errors
    /// When the autosave cannot be read or the roast cannot be saved.
    pub fn recover_autosave(&self) -> io::Result<Option<PathBuf>> {
        let path = self.dir.join(AUTOSAVE_NAME);
        if !path.exists() {
            return Ok(None);
        }
        let mut roast = read(&path)?;
        roast.title = format!("{} (recovered)", roast.title);
        let (json, _) = self.save(&roast)?;
        fs::remove_file(path)?;
        Ok(Some(json))
    }
}

/// The archive line of a roast.
#[must_use]
pub fn summary(roast: &Roast) -> Summary {
    let at = |k| roast.event(k).map(|e| e.t);
    let total_s = match (at(EventKind::Charge), at(EventKind::Drop)) {
        (Some(c), Some(d)) => Some(d - c),
        _ => None,
    };
    let drop_bt = roast.event(EventKind::Drop).and_then(|e| e.bt);
    let first_crack_s = match (at(EventKind::Charge), at(EventKind::FirstCrackStart)) {
        (Some(c), Some(f)) => Some(f - c),
        _ => None,
    };
    let dtr_pct = match (total_s, first_crack_s) {
        (Some(total), Some(fc)) if total > 0.0 && fc < total => Some((total - fc) / total * 100.0),
        _ => None,
    };
    Summary {
        id: roast.id,
        title: roast.title.clone(),
        roasted_at: roast.roasted_at,
        total_s,
        drop_bt,
        green_weight_g: roast.batch.green_weight_g,
        roasted_weight_g: roast.batch.roasted_weight_g,
        simulated: matches!(roast.origin, roast_core::RoastOrigin::Simulated),
        beans: roast.batch.beans.clone(),
        lot_id: roast.green_lot_id,
        first_crack_s,
        dtr_pct,
        goal: roast.plan.as_ref().and_then(|p| p.goal),
        batch_no: batch_no(roast),
        profile_version_id: roast
            .plan
            .as_ref()
            .and_then(|p| p.profile_version_id)
            .or(roast.profile_version_id),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn archive(dir: &Path) -> Archive {
        Archive::new(Arc::new(Store::in_memory().unwrap()), dir.join("roasts"))
    }

    #[test]
    fn slugs_are_file_safe() {
        assert_eq!(
            slug("CR002 Milenio / Black-Honey!"),
            "cr002-milenio-black-honey"
        );
    }

    #[test]
    fn batch_numbers_come_from_artisan_file_names() {
        let b = |n| batch_no_of_file_name(n);
        assert_eq!(
            b("BR011_yellow_bourbon_natural_espresso_FullCity.alog").as_deref(),
            Some("BR011")
        );
        assert_eq!(
            b("CR002_milenio_blackhoney_espresso_26-11-08_1150.alog").as_deref(),
            Some("CR002")
        );
        assert_eq!(
            b("BRYB-002_2026-11-09_1904_x.alog").as_deref(),
            Some("BRYB-002")
        );
        assert_eq!(b("2026-11-08_1150_roaster-scope.alog"), None);
        assert_eq!(b("Roast.alog"), None);
        assert_eq!(b("espresso_1.alog"), None);
    }

    fn roast(title: &str) -> Roast {
        serde_json::from_value(serde_json::json!({
            "id": Uuid::new_v4(),
            "title": title,
            "roasted_at": "2026-11-09T18:04:00Z",
            "sampling_interval_s": 1.0,
            "batch": {},
            "origin": {"kind": "recorded"},
            "samples": [
                {"t": 0.0, "bt": 180.0, "et": 200.0},
                {"t": 1.0, "bt": 179.0, "et": 200.0},
                {"t": 2.0, "bt": 178.5, "et": 201.0},
            ],
            "events": [],
            "control_log": [],
            "notes": [],
        }))
        .unwrap()
    }

    #[test]
    fn a_group_is_the_roasts_of_a_lot_a_profile_or_a_chosen_set() {
        let dir = tempfile::tempdir().unwrap();
        let a = archive(dir.path());
        let (lot, version) = (Uuid::new_v4(), Uuid::new_v4());
        let mut first = roast("first");
        first.roasted_at = "2026-11-01T10:00:00Z".parse().unwrap();
        first.green_lot_id = Some(lot);
        let mut second = roast("second");
        second.green_lot_id = Some(lot);
        second.plan = Some(roast_core::RoastPlan {
            profile_version_id: Some(version),
            ..roast_core::RoastPlan::default()
        });
        let mut sim = roast("simulated");
        sim.roasted_at = "2026-11-10T10:00:00Z".parse().unwrap();
        sim.green_lot_id = Some(lot);
        sim.origin = roast_core::RoastOrigin::Simulated;
        let other = roast("another lot");
        for r in [&first, &second, &sim, &other] {
            a.save(r).unwrap();
        }
        let titles = |g: Group, simulated: bool| -> Vec<String> {
            let roasts = a.group(&g, simulated).unwrap();
            roasts.into_iter().map(|r| r.title).collect()
        };
        // Oldest first, and the simulator only when asked.
        assert_eq!(titles(Group::Lot(lot), false), ["first", "second"]);
        assert_eq!(
            titles(Group::Lot(lot), true),
            ["first", "second", "simulated"]
        );
        assert_eq!(titles(Group::Versions(vec![version]), false), ["second"]);
        assert_eq!(
            titles(Group::Ids(vec![sim.id, other.id]), false),
            ["another lot", "simulated"]
        );
        assert!(titles(Group::Lot(Uuid::new_v4()), true).is_empty());
    }

    #[test]
    fn a_trashed_roast_leaves_the_archive_but_keeps_its_files() {
        let dir = tempfile::tempdir().unwrap();
        let a = archive(dir.path());
        let mut r = roast("Yellow Bourbon");
        r.batch.batch_no = Some("BRYB-002".into());
        let (json, alog) = a.save(&r).unwrap();
        let name = json.file_name().unwrap().to_str().unwrap().to_owned();
        assert!(name.starts_with("BRYB-002_2026-11-09_"), "{name}");
        assert_eq!(a.find(r.id).unwrap().unwrap(), r);
        assert_eq!(a.summaries().unwrap()[0]["batch_no"], "BRYB-002");

        let moved = a.trash(r.id).unwrap().unwrap();
        assert!(a.find(r.id).unwrap().is_none());
        assert!(a.summaries().unwrap().is_empty());
        assert!(!json.exists() && !alog.exists());
        assert!(moved.exists() && alog_path(&moved).exists());
        assert_eq!(read(&moved).unwrap().id, r.id);
    }

    #[test]
    fn a_changed_roast_is_rewritten_in_the_database_and_in_its_files() {
        let dir = tempfile::tempdir().unwrap();
        let a = archive(dir.path());
        let mut r = roast("Castillo");
        let (json, _) = a.save(&r).unwrap();
        r.batch.roasted_weight_g = Some(171.0);
        a.update(&r).unwrap();
        assert_eq!(a.find(r.id).unwrap().unwrap(), r);
        assert_eq!(read(&json).unwrap(), r);
        assert_eq!(a.summaries().unwrap()[0]["roasted_weight_g"], 171.0);
        assert_eq!(roast_files(a.dir()).len(), 1);

        // Without its file copy the roast is still there and gets a new one.
        fs::remove_file(&json).unwrap();
        r.title = "Castillo, washed".into();
        a.update(&r).unwrap();
        assert_eq!(roast_files(a.dir()).len(), 1);
        assert_eq!(a.find(r.id).unwrap().unwrap().title, "Castillo, washed");
    }

    #[test]
    fn roast_files_of_an_earlier_version_are_taken_into_the_database_once() {
        let dir = tempfile::tempdir().unwrap();
        let a = archive(dir.path());
        let (first, second) = (roast("first"), roast("second"));
        save(a.dir(), &first).unwrap();
        save(a.dir(), &second).unwrap();
        fs::write(a.dir().join("broken.roast.json"), "{").unwrap();
        autosave(a.dir(), &roast("running")).unwrap();
        assert!(a.summaries().unwrap().is_empty());

        assert_eq!(a.sync().unwrap(), 2);
        assert_eq!(a.sync().unwrap(), 0);
        assert_eq!(a.summaries().unwrap().len(), 2);
        assert_eq!(a.find(first.id).unwrap().unwrap(), first);

        // A leftover autosave becomes a roast of its own.
        let recovered = a.recover_autosave().unwrap().unwrap();
        assert_eq!(read(&recovered).unwrap().title, "running (recovered)");
        assert_eq!(a.summaries().unwrap().len(), 3);
        assert!(a.recover_autosave().unwrap().is_none());
    }

    #[test]
    fn an_uploaded_alog_is_imported_once_and_stays_in_the_archive_folder() {
        let dir = tempfile::tempdir().unwrap();
        let a = archive(dir.path());
        let alog = roast_alog::write_alog(&roast(""));

        let saved = a.import_alog_bytes("../../BR011_bourbon.alog", alog.as_bytes());
        let Ok(Imported::Saved(id)) = saved else {
            panic!("not saved: {saved:?}");
        };
        let files = roast_files(a.dir());
        assert_eq!(files.len(), 1, "{files:?}");
        let read_back = a.find(id).unwrap().unwrap();
        assert_eq!(read_back.title, "BR011_bourbon");
        assert_eq!(read_back.samples.len(), 3);
        assert_eq!(a.summaries().unwrap()[0]["batch_no"], "BR011");

        let again = a.import_alog_bytes("copy.alog", alog.as_bytes());
        assert_eq!(again.unwrap(), Imported::AlreadyThere(id));
        assert!(a.import_alog_bytes("x.alog", b"not an alog").is_err());
    }

    #[test]
    fn alog_sits_next_to_json() {
        let p = alog_path(Path::new("/x/2026-11-08_1900_a.roast.json"));
        assert_eq!(p, Path::new("/x/2026-11-08_1900_a.alog"));
    }
}
