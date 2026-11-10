//! The JSON files of the time before the database (up to 1.16):
//! `inventory.json`, `profiles.json`, `control.json`. On the first start
//! with the database each is read once, stored there, and renamed to
//! `<name>.migrated`, so it stays as a copy but is no longer live.

use std::path::{Path, PathBuf};

use serde::de::DeserializeOwned;

/// The content of a legacy file: `Ok(None)` if there is none.
///
/// # Errors
/// With a message for the person if the file is there but cannot be read;
/// it is then left alone.
pub fn read<T: DeserializeOwned>(path: &Path) -> Result<Option<T>, String> {
    match std::fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes).map(Some).map_err(|e| {
            format!(
                "{} could not be read ({e}); fix it or restore {}.bak",
                path.display(),
                path.display()
            )
        }),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("{} could not be read ({e})", path.display())),
    }
}

/// Marks a legacy file as taken over.
pub fn retire(path: &Path) {
    let mut to = path.as_os_str().to_owned();
    to.push(".migrated");
    let to = PathBuf::from(to);
    match std::fs::rename(path, &to) {
        Ok(()) => tracing::info!(
            "{} is now in the database; the file is kept as {}",
            path.display(),
            to.display()
        ),
        Err(e) => tracing::warn!("{} could not be renamed: {e}", path.display()),
    }
}
