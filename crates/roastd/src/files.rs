//! Roast files on disk: `<title>.roast.json` (the open format, ADR 0008) plus
//! an Artisan `.alog` next to it, an autosave of the running recording, and
//! the archive listing.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use roast_core::{EventKind, ROAST_FILE_SCHEMA_VERSION, Roast};
use serde::{Deserialize, Serialize};

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
}

/// Opens a file for appending, creating it and its directory.
pub fn open_append(path: &Path) -> io::Result<File> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    OpenOptions::new().create(true).append(true).open(path)
}

fn write_atomic(path: &Path, contents: &[u8]) -> io::Result<()> {
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

/// The base name (without extension) for a new roast file.
fn base_name(roast: &Roast) -> String {
    let local = roast.roasted_at.with_timezone(&chrono::Local);
    format!("{}_{}", local.format("%Y-%m-%d_%H%M"), slug(&roast.title))
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
    Summary {
        id: roast.id,
        title: roast.title.clone(),
        roasted_at: roast.roasted_at,
        total_s,
        drop_bt,
        green_weight_g: roast.batch.green_weight_g,
        roasted_weight_g: roast.batch.roasted_weight_g,
        simulated: matches!(roast.origin, roast_core::RoastOrigin::Simulated),
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
    fn alog_sits_next_to_json() {
        let p = alog_path(Path::new("/x/2026-11-08_1900_a.roast.json"));
        assert_eq!(p, Path::new("/x/2026-11-08_1900_a.alog"));
    }
}
