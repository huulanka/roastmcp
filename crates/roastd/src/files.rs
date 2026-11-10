//! Roast files on disk: `<title>.roast.json` (the open format, ADR 0008) plus
//! an Artisan `.alog` next to it, an autosave of the running recording, and
//! the archive listing.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use roast_core::{EventKind, ROAST_FILE_SCHEMA_VERSION, Roast};
use serde::{Deserialize, Serialize};

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
}

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
pub fn trash(json: &Path) -> io::Result<PathBuf> {
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

/// Saves a roast as `.roast.json` and `.alog`; returns both paths.
pub fn save(dir: &Path, roast: &Roast) -> io::Result<(PathBuf, PathBuf)> {
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
#[derive(Debug)]
pub enum Imported {
    /// Saved as this `.roast.json`.
    Saved(PathBuf),
    /// The archive already holds this roast (same time, same curve length).
    AlreadyThere(PathBuf),
}

/// Imports an Artisan `.alog` into the archive: the roast as `.roast.json`
/// and the original file unchanged beside it. Importing the same file again
/// does nothing.
pub fn import_alog(dir: &Path, alog: &Path) -> io::Result<Imported> {
    let name = alog
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    import_alog_bytes(dir, &name, &fs::read(alog)?)
}

/// As [`import_alog`], for the content of a file called `file_name` (an
/// upload from the app). Content that is not valid UTF-8 is read as Latin-1.
pub fn import_alog_bytes(dir: &Path, file_name: &str, bytes: &[u8]) -> io::Result<Imported> {
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
    if let Some((path, _)) = list(dir)
        .into_iter()
        .find(|(_, r)| r.roasted_at == roast.roasted_at && r.samples.len() == roast.samples.len())
    {
        return Ok(Imported::AlreadyThere(path));
    }
    fs::create_dir_all(dir)?;
    let base = base_name(&roast);
    let mut json = dir.join(format!("{base}{JSON_EXT}"));
    let mut n = 2;
    while json.exists() {
        json = dir.join(format!("{base}-{n}{JSON_EXT}"));
        n += 1;
    }
    write_atomic(&json, &to_json(&roast)?)?;
    write_atomic(&alog_path(&json), bytes)?;
    Ok(Imported::Saved(json))
}

/// Rewrites an existing roast in place (both files).
pub fn overwrite(json: &Path, roast: &Roast) -> io::Result<()> {
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
pub fn read(path: &Path) -> io::Result<Roast> {
    let bytes = fs::read(path)?;
    let file: RoastFile = serde_json::from_slice(&bytes).map_err(io::Error::other)?;
    Ok(file.roast)
}

/// Writes the autosave of the running recording.
pub fn autosave(dir: &Path, roast: &Roast) -> io::Result<()> {
    fs::create_dir_all(dir)?;
    write_atomic(&dir.join(AUTOSAVE_NAME), &to_json(roast)?)
}

/// Removes the autosave after a successful save.
pub fn clear_autosave(dir: &Path) {
    let _ = fs::remove_file(dir.join(AUTOSAVE_NAME));
}

/// Turns a leftover autosave (the daemon stopped during a recording) into a
/// normal roast file.
pub fn recover_autosave(dir: &Path) -> io::Result<Option<PathBuf>> {
    let path = dir.join(AUTOSAVE_NAME);
    if !path.exists() {
        return Ok(None);
    }
    let mut roast = read(&path)?;
    roast.title = format!("{} (recovered)", roast.title);
    let (json, _) = save(dir, &roast)?;
    fs::remove_file(path)?;
    Ok(Some(json))
}

/// All saved roasts, newest first.
pub fn list(dir: &Path) -> Vec<(PathBuf, Roast)> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out: Vec<(PathBuf, Roast)> = entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.ends_with(JSON_EXT) && !n.starts_with('.'))
        })
        .filter_map(|p| read(&p).ok().map(|r| (p, r)))
        .collect();
    out.sort_by_key(|(_, r)| std::cmp::Reverse(r.roasted_at));
    out
}

/// Finds a roast by id.
pub fn find(dir: &Path, id: uuid::Uuid) -> Option<(PathBuf, Roast)> {
    list(dir).into_iter().find(|(_, r)| r.id == id)
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
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn a_trashed_roast_leaves_the_archive_but_keeps_its_files() {
        let dir = tempfile::tempdir().unwrap();
        let mut r: Roast = serde_json::from_value(serde_json::json!({
            "id": uuid::Uuid::new_v4(),
            "title": "Yellow Bourbon",
            "roasted_at": "2026-11-09T18:04:00Z",
            "sampling_interval_s": 1.5,
            "batch": {},
            "origin": {"kind": "recorded"},
            "samples": [],
            "events": [],
            "control_log": [],
            "notes": [],
        }))
        .unwrap();
        r.batch.batch_no = Some("BRYB-002".into());
        let (json, alog) = save(dir.path(), &r).unwrap();
        let name = json.file_name().unwrap().to_str().unwrap().to_owned();
        assert!(name.starts_with("BRYB-002_2026-11-09_"), "{name}");
        assert_eq!(find(dir.path(), r.id).unwrap().0, json);

        let moved = trash(&json).unwrap();
        assert!(find(dir.path(), r.id).is_none());
        assert!(!json.exists() && !alog.exists());
        assert!(moved.exists() && alog_path(&moved).exists());
        assert_eq!(read(&moved).unwrap().id, r.id);
    }

    #[test]
    fn an_uploaded_alog_is_imported_once_and_stays_in_the_archive_folder() {
        let dir = tempfile::tempdir().unwrap();
        let r: Roast = serde_json::from_value(serde_json::json!({
            "id": uuid::Uuid::new_v4(),
            "title": "",
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
        .unwrap();
        let alog = roast_alog::write_alog(&r);

        let saved = import_alog_bytes(dir.path(), "../../BR011_bourbon.alog", alog.as_bytes());
        let Ok(Imported::Saved(json)) = saved else {
            panic!("not saved: {saved:?}");
        };
        assert_eq!(json.parent(), Some(dir.path()));
        let read_back = read(&json).unwrap();
        assert_eq!(read_back.title, "BR011_bourbon");
        assert_eq!(read_back.samples.len(), 3);
        assert_eq!(summary(&read_back).batch_no.as_deref(), Some("BR011"));

        let again = import_alog_bytes(dir.path(), "copy.alog", alog.as_bytes());
        assert!(matches!(again, Ok(Imported::AlreadyThere(p)) if p == json));
        assert!(import_alog_bytes(dir.path(), "x.alog", b"not an alog").is_err());
    }

    #[test]
    fn alog_sits_next_to_json() {
        let p = alog_path(Path::new("/x/2026-11-08_1900_a.roast.json"));
        assert_eq!(p, Path::new("/x/2026-11-08_1900_a.alog"));
    }
}
