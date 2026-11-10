//! Backups of the database to a folder the person chose (ADR 0003): after
//! every saved roast and once a day, checked and rotated by `roast-store`.
//! The folder and the last result are settings in the database itself.

use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};

use chrono::{DateTime, Duration, Utc};
use roast_store::Store;
use serde::{Deserialize, Serialize};

const CONFIG: &str = "backup";
const LAST: &str = "backup_last";
/// How many backups stay in the folder unless set otherwise.
pub const DEFAULT_KEEP: usize = 30;

/// One backup at a time: two in the same second would share a file name.
static RUNNING: Mutex<()> = Mutex::new(());

fn default_keep() -> usize {
    DEFAULT_KEEP
}

/// Where backups go and how many are kept.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BackupConfig {
    /// The backup folder; none means no backups.
    pub dir: Option<PathBuf>,
    /// How many backups stay in the folder.
    #[serde(default = "default_keep")]
    pub keep: usize,
}

impl Default for BackupConfig {
    fn default() -> Self {
        BackupConfig {
            dir: None,
            keep: DEFAULT_KEEP,
        }
    }
}

/// How the last backup went.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupResult {
    /// When it ran.
    pub at: DateTime<Utc>,
    /// Whether it succeeded.
    pub ok: bool,
    /// The file written.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<PathBuf>,
    /// Its size.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bytes: Option<u64>,
    /// Why it failed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// The backup settings with the last result (`GET /api/backup`).
#[derive(Debug, Clone, Serialize)]
pub struct BackupStatus {
    /// The backup folder; none means no backups.
    pub dir: Option<PathBuf>,
    /// How many backups stay in the folder.
    pub keep: usize,
    /// How the last backup went, if one ran.
    pub last: Option<BackupResult>,
}

/// The stored settings.
#[must_use]
pub fn config(store: &Store) -> BackupConfig {
    store.setting(CONFIG).ok().flatten().unwrap_or_default()
}

/// The settings and the last result.
#[must_use]
pub fn status(store: &Store) -> BackupStatus {
    let c = config(store);
    BackupStatus {
        dir: c.dir,
        keep: c.keep,
        last: store.setting(LAST).ok().flatten(),
    }
}

/// Stores the settings. A folder is created (or must be writable) right
/// away, so a typo shows now and not after the next roast.
///
/// # Errors
/// With the reason when the folder cannot be used.
pub fn set_config(store: &Store, config: &BackupConfig) -> Result<(), String> {
    if let Some(dir) = &config.dir {
        if !dir.is_absolute() {
            return Err("the backup folder must be a full path".into());
        }
        std::fs::create_dir_all(dir)
            .map_err(|e| format!("{} cannot be used: {e}", dir.display()))?;
    }
    if config.keep == 0 {
        return Err("at least one backup must be kept".into());
    }
    store.set_setting(CONFIG, config).map_err(|e| e.to_string())
}

/// Makes a backup now and records how it went.
///
/// # Errors
/// With the reason when no folder is set or the backup failed.
pub fn run(store: &Store) -> Result<BackupResult, String> {
    let c = config(store);
    let dir = c.dir.ok_or("no backup folder is set")?;
    let _one = RUNNING.lock().unwrap_or_else(PoisonError::into_inner);
    let result = match store.backup(&dir, c.keep) {
        Ok(r) => BackupResult {
            at: Utc::now(),
            ok: true,
            path: Some(r.path),
            bytes: Some(r.bytes),
            error: None,
        },
        Err(e) => BackupResult {
            at: Utc::now(),
            ok: false,
            path: None,
            bytes: None,
            error: Some(e.to_string()),
        },
    };
    if let Err(e) = store.set_setting(LAST, &result) {
        tracing::warn!("the backup result could not be stored: {e}");
    }
    match &result.error {
        None => {
            tracing::info!("backup written to {}", dir.display());
            Ok(result)
        }
        Some(e) => {
            tracing::warn!("backup to {} failed: {e}", dir.display());
            Err(e.clone())
        }
    }
}

/// Makes a backup on another thread if a folder is set (after a roast was
/// saved: the roast must not wait for a slow network drive).
pub fn spawn(store: &Arc<Store>) {
    if config(store).dir.is_none() {
        return;
    }
    let store = Arc::clone(store);
    std::thread::spawn(move || {
        let _ = run(&store);
    });
}

/// Whether the daily backup is due: a folder is set and the last attempt is
/// a day old (a failed one is tried again after an hour).
#[must_use]
pub fn due(store: &Store, now: DateTime<Utc>) -> bool {
    let s = status(store);
    if s.dir.is_none() {
        return false;
    }
    match s.last {
        None => true,
        Some(last) if last.ok => now - last.at >= Duration::hours(24),
        Some(last) => now - last.at >= Duration::hours(1),
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn a_backup_needs_a_folder_and_is_due_once_a_day() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("roastmcp.sqlite")).unwrap();
        assert!(!due(&store, Utc::now()));
        assert!(run(&store).is_err());
        assert!(
            set_config(
                &store,
                &BackupConfig {
                    dir: Some("relative".into()),
                    keep: 3
                }
            )
            .is_err()
        );

        let target = dir.path().join("nas").join("roastmcp");
        set_config(
            &store,
            &BackupConfig {
                dir: Some(target.clone()),
                keep: 3,
            },
        )
        .unwrap();
        assert!(due(&store, Utc::now()));
        let done = run(&store).unwrap();
        assert!(done.path.unwrap().starts_with(&target));
        assert!(!due(&store, Utc::now()));
        assert!(due(&store, Utc::now() + Duration::hours(25)));
        assert!(status(&store).last.unwrap().ok);
    }
}
