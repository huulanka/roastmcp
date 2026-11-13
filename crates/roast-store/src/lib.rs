//! SQLite storage and backups ([ADR 0003]).
//!
//! [`Store`] is the database: one SQLite file on the local disk, in WAL mode
//! with foreign keys on, opened by the daemon alone. It holds the roasts, the
//! green coffee inventory, the profiles, the experiments and the settings;
//! the schema is in `schema.sql` and later versions (`migrations/`) are
//! applied on open.
//!
//! [`Store::backup`] writes a verified copy into another folder (which may be
//! on a NAS) and keeps the newest few; [`restore`] puts one back.
//!
//! [ADR 0003]: https://github.com/huulanka/roastmcp/blob/main/docs/adr/0003-local-sqlite-with-backups.md

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError};

use chrono::{DateTime, SecondsFormat, Utc};
use roast_core::{Experiment, Inventory, Profile, ProfileVersion, Roast};
use rusqlite::{Connection, OpenFlags, OptionalExtension, params};
use serde::Serialize;
use serde::de::DeserializeOwned;
use uuid::Uuid;

/// Schema versions in order; version `n` is reached by running the first `n`.
const MIGRATIONS: &[&str] = &[
    include_str!("schema.sql"),
    include_str!("migrations/0002_experiments.sql"),
];

/// Marks a file as a RoastMCP database (`PRAGMA application_id`, "RMCP").
const APPLICATION_ID: i64 = 0x524D_4350;

const BACKUP_PREFIX: &str = "roastmcp-";
const BACKUP_EXT: &str = ".sqlite";

/// Errors of the storage layer.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// SQLite reported an error.
    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),
    /// A stored value could not be encoded or decoded.
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    /// A file operation failed.
    #[error("{path}: {source}")]
    Io {
        /// The file or folder.
        path: PathBuf,
        /// What went wrong.
        source: std::io::Error,
    },
    /// The database was written by a newer RoastMCP.
    #[error(
        "{path} was written by a newer RoastMCP (schema {found}, this version reads up to {supported})"
    )]
    Newer {
        /// The database file.
        path: PathBuf,
        /// Its schema version.
        found: i64,
        /// The newest version this build knows.
        supported: i64,
    },
    /// The file is not a RoastMCP database.
    #[error("{0} is not a RoastMCP database")]
    NotOurs(PathBuf),
    /// SQLite's integrity check found problems.
    #[error("{path} is damaged: {details}")]
    Integrity {
        /// The database file.
        path: PathBuf,
        /// What SQLite reported.
        details: String,
    },
}

/// Result type of the storage layer.
pub type Result<T, E = Error> = std::result::Result<T, E>;

fn io(path: &Path) -> impl FnOnce(std::io::Error) -> Error + '_ {
    move |source| Error::Io {
        path: path.to_owned(),
        source,
    }
}

/// A count or position as SQLite's integer.
fn int(n: usize) -> i64 {
    i64::try_from(n).unwrap_or(i64::MAX)
}

fn instant(t: DateTime<Utc>) -> String {
    t.to_rfc3339_opts(SecondsFormat::Millis, true)
}

/// A roast as stored, with the name of its open file copy.
#[derive(Debug, Clone)]
pub struct StoredRoast {
    /// The roast.
    pub roast: Roast,
    /// Its `.roast.json` in the roasts folder, if it has one.
    pub file_name: Option<String>,
}

/// What a backup did.
#[derive(Debug, Clone, Serialize)]
pub struct BackupReport {
    /// The backup file.
    pub path: PathBuf,
    /// Its size.
    pub bytes: u64,
    /// Older backups removed by the rotation.
    pub removed: usize,
}

/// The database.
#[derive(Debug)]
pub struct Store {
    conn: Mutex<Connection>,
}

fn migrate(conn: &mut Connection, path: &Path) -> Result<()> {
    let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    let supported = i64::try_from(MIGRATIONS.len()).unwrap_or(i64::MAX);
    if version > supported {
        return Err(Error::Newer {
            path: path.to_owned(),
            found: version,
            supported,
        });
    }
    let app_id: i64 = conn.query_row("PRAGMA application_id", [], |r| r.get(0))?;
    if version > 0 && app_id != APPLICATION_ID {
        return Err(Error::NotOurs(path.to_owned()));
    }
    let done = usize::try_from(version).unwrap_or(0);
    for (i, sql) in MIGRATIONS.iter().enumerate().skip(done) {
        let tx = conn.transaction()?;
        tx.execute_batch(sql)?;
        tx.pragma_update(None, "user_version", int(i + 1))?;
        tx.pragma_update(None, "application_id", APPLICATION_ID)?;
        tx.commit()?;
    }
    Ok(())
}

impl Store {
    /// Opens the database at `path`, creating it and its folder if needed,
    /// and brings its schema up to date.
    ///
    /// # Errors
    ///
    /// Fails if the file cannot be opened, is not a RoastMCP database or was
    /// written by a newer version.
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(io(dir))?;
        }
        let mut conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        // Writes are rare and precious: wait for the disk.
        conn.pragma_update(None, "synchronous", "FULL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        migrate(&mut conn, path)?;
        Ok(Store {
            conn: Mutex::new(conn),
        })
    }

    /// A database in memory, for tests.
    ///
    /// # Errors
    ///
    /// Fails if SQLite cannot create it.
    pub fn in_memory() -> Result<Self> {
        let mut conn = Connection::open_in_memory()?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        migrate(&mut conn, Path::new(":memory:"))?;
        Ok(Store {
            conn: Mutex::new(conn),
        })
    }

    fn conn(&self) -> MutexGuard<'_, Connection> {
        // A panic while holding the lock leaves SQLite itself consistent.
        self.conn.lock().unwrap_or_else(PoisonError::into_inner)
    }

    // ------------------------------------------------------------ settings

    /// The setting stored under `key`.
    ///
    /// # Errors
    ///
    /// Fails on a database error or if the stored value does not fit `T`.
    pub fn setting<T: DeserializeOwned>(&self, key: &str) -> Result<Option<T>> {
        let text: Option<String> = self
            .conn()
            .query_row("SELECT value FROM settings WHERE key = ?1", [key], |r| {
                r.get(0)
            })
            .optional()?;
        Ok(match text {
            Some(t) => Some(serde_json::from_str(&t)?),
            None => None,
        })
    }

    /// Stores `value` under `key`.
    ///
    /// # Errors
    ///
    /// Fails on a database error.
    pub fn set_setting<T: Serialize>(&self, key: &str, value: &T) -> Result<()> {
        let text = serde_json::to_string(value)?;
        self.conn().execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2)
             ON CONFLICT (key) DO UPDATE SET value = excluded.value",
            params![key, text],
        )?;
        Ok(())
    }

    // ----------------------------------------------------------- inventory

    /// The whole green coffee inventory.
    ///
    /// # Errors
    ///
    /// Fails on a database error or an undecodable record.
    pub fn inventory(&self) -> Result<Inventory> {
        let conn = self.conn();
        Ok(Inventory {
            lots: column(&conn, "SELECT lot FROM lots ORDER BY pos")?,
            movements: column(&conn, "SELECT movement FROM stock_movements ORDER BY pos")?,
        })
    }

    /// Replaces the stored inventory with `inv`, all or nothing.
    ///
    /// # Errors
    ///
    /// Fails, changing nothing, on a database error or if a movement refers
    /// to a lot that does not exist.
    pub fn save_inventory(&self, inv: &Inventory) -> Result<()> {
        let mut conn = self.conn();
        let tx = conn.transaction()?;
        tx.execute("DELETE FROM stock_movements", [])?;
        tx.execute("DELETE FROM lots", [])?;
        for (pos, lot) in inv.lots.iter().enumerate() {
            tx.execute(
                "INSERT INTO lots (id, pos, name, archived, lot) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    lot.id.to_string(),
                    int(pos),
                    lot.name,
                    lot.archived,
                    serde_json::to_string(lot)?
                ],
            )?;
        }
        for (pos, m) in inv.movements.iter().enumerate() {
            tx.execute(
                "INSERT INTO stock_movements (id, pos, lot_id, at, grams, movement)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    m.id.to_string(),
                    int(pos),
                    m.lot_id.to_string(),
                    instant(m.at),
                    m.grams,
                    serde_json::to_string(m)?
                ],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    // ------------------------------------------------------------ profiles

    /// All profiles and all their versions.
    ///
    /// # Errors
    ///
    /// Fails on a database error or an undecodable record.
    pub fn profiles(&self) -> Result<(Vec<Profile>, Vec<ProfileVersion>)> {
        let conn = self.conn();
        Ok((
            column(&conn, "SELECT profile FROM profiles ORDER BY pos")?,
            column(
                &conn,
                "SELECT profile_version FROM profile_versions ORDER BY pos",
            )?,
        ))
    }

    /// Replaces the stored profiles and versions, all or nothing.
    ///
    /// # Errors
    ///
    /// Fails, changing nothing, on a database error or if a version refers to
    /// a profile that does not exist.
    pub fn save_profiles(&self, profiles: &[Profile], versions: &[ProfileVersion]) -> Result<()> {
        let mut conn = self.conn();
        let tx = conn.transaction()?;
        tx.execute("DELETE FROM profile_versions", [])?;
        tx.execute("DELETE FROM profiles", [])?;
        for (pos, p) in profiles.iter().enumerate() {
            tx.execute(
                "INSERT INTO profiles (id, pos, name, archived, profile)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    p.id.to_string(),
                    int(pos),
                    p.name,
                    p.archived,
                    serde_json::to_string(p)?
                ],
            )?;
        }
        for (pos, v) in versions.iter().enumerate() {
            tx.execute(
                "INSERT INTO profile_versions
                    (id, pos, profile_id, version, created_at, profile_version)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    v.id.to_string(),
                    int(pos),
                    v.profile_id.to_string(),
                    v.version,
                    instant(v.created_at),
                    serde_json::to_string(v)?
                ],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    // --------------------------------------------------------- experiments

    /// All experiments, newest first.
    ///
    /// # Errors
    ///
    /// Fails on a database error or an undecodable record.
    pub fn experiments(&self) -> Result<Vec<Experiment>> {
        column(
            &self.conn(),
            "SELECT experiment FROM experiments ORDER BY created_at DESC, id",
        )
    }

    /// Saves an experiment (new, or replacing the stored one with its id).
    ///
    /// # Errors
    ///
    /// Fails on a database error.
    pub fn save_experiment(&self, e: &Experiment) -> Result<()> {
        self.conn().execute(
            "INSERT INTO experiments (id, created_at, status, experiment)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT (id) DO UPDATE SET
                created_at = excluded.created_at,
                status = excluded.status,
                experiment = excluded.experiment",
            params![
                e.id.to_string(),
                instant(e.created_at),
                e.status.as_str(),
                serde_json::to_string(e)?
            ],
        )?;
        Ok(())
    }

    /// Removes an experiment; `false` if there was none with this id.
    ///
    /// # Errors
    ///
    /// Fails on a database error.
    pub fn delete_experiment(&self, id: Uuid) -> Result<bool> {
        let n = self.conn().execute(
            "DELETE FROM experiments WHERE id = ?1",
            params![id.to_string()],
        )?;
        Ok(n > 0)
    }

    // -------------------------------------------------------------- roasts

    /// Saves a roast (new, or replacing the stored one with the same id).
    /// `summary` is its line in the archive listing; `file_name` its open
    /// file copy, kept as stored when `None`.
    ///
    /// # Errors
    ///
    /// Fails on a database error.
    pub fn save_roast(
        &self,
        roast: &Roast,
        summary: &serde_json::Value,
        file_name: Option<&str>,
    ) -> Result<()> {
        self.conn().execute(
            "INSERT INTO roasts
                (id, roasted_at, title, lot_id, sample_count, file_name, summary, roast, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
             ON CONFLICT (id) DO UPDATE SET
                roasted_at = excluded.roasted_at,
                title = excluded.title,
                lot_id = excluded.lot_id,
                sample_count = excluded.sample_count,
                file_name = coalesce(excluded.file_name, roasts.file_name),
                summary = excluded.summary,
                roast = excluded.roast,
                updated_at = excluded.updated_at",
            params![
                roast.id.to_string(),
                instant(roast.roasted_at),
                roast.title,
                roast.green_lot_id.map(|id| id.to_string()),
                int(roast.samples.len()),
                file_name,
                serde_json::to_string(summary)?,
                serde_json::to_string(roast)?,
                instant(Utc::now()),
            ],
        )?;
        Ok(())
    }

    /// The roast with this id.
    ///
    /// # Errors
    ///
    /// Fails on a database error or an undecodable roast.
    pub fn roast(&self, id: Uuid) -> Result<Option<StoredRoast>> {
        let row: Option<(String, Option<String>)> = self
            .conn()
            .query_row(
                "SELECT roast, file_name FROM roasts WHERE id = ?1",
                [id.to_string()],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        Ok(match row {
            Some((json, file_name)) => Some(StoredRoast {
                roast: serde_json::from_str(&json)?,
                file_name,
            }),
            None => None,
        })
    }

    /// The archive listing: every roast's summary, newest first.
    ///
    /// # Errors
    ///
    /// Fails on a database error.
    pub fn roast_summaries(&self) -> Result<Vec<serde_json::Value>> {
        column(
            &self.conn(),
            "SELECT summary FROM roasts ORDER BY roasted_at DESC, id",
        )
    }

    /// Removes a roast; `false` if there was none with this id.
    ///
    /// # Errors
    ///
    /// Fails on a database error.
    pub fn delete_roast(&self, id: Uuid) -> Result<bool> {
        let n = self
            .conn()
            .execute("DELETE FROM roasts WHERE id = ?1", [id.to_string()])?;
        Ok(n > 0)
    }

    /// The roast recorded at this instant with this many samples, if the
    /// archive already holds it (an import of the same file).
    ///
    /// # Errors
    ///
    /// Fails on a database error.
    pub fn same_roast(&self, roasted_at: DateTime<Utc>, samples: usize) -> Result<Option<Uuid>> {
        let id: Option<String> = self
            .conn()
            .query_row(
                "SELECT id FROM roasts WHERE roasted_at = ?1 AND sample_count = ?2",
                params![instant(roasted_at), int(samples)],
                |r| r.get(0),
            )
            .optional()?;
        Ok(id.and_then(|s| s.parse().ok()))
    }

    /// The file names of all roasts' open copies.
    ///
    /// # Errors
    ///
    /// Fails on a database error.
    pub fn roast_file_names(&self) -> Result<HashSet<String>> {
        let conn = self.conn();
        let mut stmt = conn.prepare("SELECT file_name FROM roasts WHERE file_name IS NOT NULL")?;
        let names = stmt
            .query_map([], |r| r.get::<_, String>(0))?
            .collect::<rusqlite::Result<_>>()?;
        Ok(names)
    }

    /// Writes every roast's summary anew with `summarise` (after the listing
    /// changed). Returns how many roasts there are.
    ///
    /// # Errors
    ///
    /// Fails, changing nothing, on a database error or an undecodable roast.
    pub fn resummarise(&self, summarise: impl Fn(&Roast) -> serde_json::Value) -> Result<usize> {
        let mut conn = self.conn();
        let tx = conn.transaction()?;
        let roasts: Vec<Roast> = column(&tx, "SELECT roast FROM roasts")?;
        for r in &roasts {
            tx.execute(
                "UPDATE roasts SET summary = ?2 WHERE id = ?1",
                params![r.id.to_string(), serde_json::to_string(&summarise(r))?],
            )?;
        }
        tx.commit()?;
        Ok(roasts.len())
    }

    // -------------------------------------------------------------- backup

    /// Writes a backup into `dir` (`roastmcp-<UTC time>.sqlite`), checks it,
    /// and removes all but the newest `keep` backups there. The database
    /// stays usable meanwhile; a failed backup leaves no file behind.
    ///
    /// # Errors
    ///
    /// Fails if the folder cannot be written or the copy does not pass the
    /// integrity check.
    pub fn backup(&self, dir: &Path, keep: usize) -> Result<BackupReport> {
        std::fs::create_dir_all(dir).map_err(io(dir))?;
        let name = format!(
            "{BACKUP_PREFIX}{}{BACKUP_EXT}",
            Utc::now().format("%Y%m%d-%H%M%S")
        );
        let path = dir.join(&name);
        let partial = dir.join(format!(".{name}.partial"));
        // Left over from an interrupted backup: VACUUM INTO needs a new file.
        let _ = std::fs::remove_file(&partial);
        let made = self
            .conn()
            .execute("VACUUM INTO ?1", [partial.to_string_lossy().as_ref()])
            .map_err(Error::from)
            .and_then(|_| verify(&partial))
            .and_then(|()| std::fs::rename(&partial, &path).map_err(io(&path)));
        if let Err(e) = made {
            let _ = std::fs::remove_file(&partial);
            return Err(e);
        }
        let bytes = std::fs::metadata(&path).map_err(io(&path))?.len();
        let mut old = backups(dir)?;
        let mut removed = 0;
        // Newest first; never the one just written.
        for stale in old.drain(..).skip(keep.max(1)) {
            if std::fs::remove_file(&stale).is_ok() {
                removed += 1;
            }
        }
        Ok(BackupReport {
            path,
            bytes,
            removed,
        })
    }
}

fn column<T: DeserializeOwned>(conn: &Connection, sql: &str) -> Result<Vec<T>> {
    let mut stmt = conn.prepare(sql)?;
    let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
    let mut out = Vec::new();
    for json in rows {
        out.push(serde_json::from_str(&json?)?);
    }
    Ok(out)
}

/// The backups in `dir`, newest first.
///
/// # Errors
///
/// Fails if the folder cannot be read.
pub fn backups(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut found: Vec<PathBuf> = std::fs::read_dir(dir)
        .map_err(io(dir))?
        .filter_map(std::result::Result::ok)
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with(BACKUP_PREFIX) && n.ends_with(BACKUP_EXT))
        })
        .collect();
    // The names carry the UTC time, so name order is time order.
    found.sort();
    found.reverse();
    Ok(found)
}

/// Checks that `path` is an undamaged RoastMCP database this build can read.
/// The file is only read.
///
/// # Errors
///
/// Fails with the reason if it is not.
pub fn verify(path: &Path) -> Result<()> {
    let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let app_id: i64 = conn
        .query_row("PRAGMA application_id", [], |r| r.get(0))
        .map_err(|_| Error::NotOurs(path.to_owned()))?;
    if app_id != APPLICATION_ID {
        return Err(Error::NotOurs(path.to_owned()));
    }
    let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    let supported = i64::try_from(MIGRATIONS.len()).unwrap_or(i64::MAX);
    if version > supported {
        return Err(Error::Newer {
            path: path.to_owned(),
            found: version,
            supported,
        });
    }
    let mut problems: Vec<String> = Vec::new();
    {
        let mut stmt = conn.prepare("PRAGMA integrity_check")?;
        for line in stmt.query_map([], |r| r.get::<_, String>(0))? {
            let line = line?;
            if line != "ok" {
                problems.push(line);
            }
        }
        let mut stmt = conn.prepare("PRAGMA foreign_key_check")?;
        for table in stmt.query_map([], |r| r.get::<_, String>(0))? {
            problems.push(format!("broken reference in {}", table?));
        }
    }
    if problems.is_empty() {
        Ok(())
    } else {
        Err(Error::Integrity {
            path: path.to_owned(),
            details: problems.join("; "),
        })
    }
}

/// Replaces the database at `db` with the backup `backup`, after checking
/// the backup. The daemon must not be running. The database that was there
/// is kept beside it as `<db>.before-restore-<UTC time>`; its path is
/// returned.
///
/// # Errors
///
/// Fails, changing nothing, if the backup does not pass [`verify`]; fails if
/// files cannot be moved or copied.
pub fn restore(db: &Path, backup: &Path) -> Result<Option<PathBuf>> {
    verify(backup)?;
    let kept = if db.exists() {
        // Fold the write-ahead log into the file, so the kept copy is whole.
        if let Ok(conn) = Connection::open(db) {
            let _ = conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()));
        }
        let mut name = db.as_os_str().to_owned();
        name.push(format!(
            ".before-restore-{}",
            Utc::now().format("%Y%m%d-%H%M%S")
        ));
        let kept = PathBuf::from(name);
        std::fs::rename(db, &kept).map_err(io(db))?;
        for ext in ["-wal", "-shm"] {
            let mut side = db.as_os_str().to_owned();
            side.push(ext);
            let _ = std::fs::remove_file(PathBuf::from(side));
        }
        Some(kept)
    } else {
        if let Some(dir) = db.parent() {
            std::fs::create_dir_all(dir).map_err(io(dir))?;
        }
        None
    };
    let mut tmp = db.as_os_str().to_owned();
    tmp.push(".restoring");
    let tmp = PathBuf::from(tmp);
    std::fs::copy(backup, &tmp).map_err(io(&tmp))?;
    std::fs::rename(&tmp, db).map_err(io(db))?;
    Ok(kept)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;
    use roast_core::{GreenLot, StockMovement};

    fn roast(title: &str, at: &str, samples: usize) -> Roast {
        let samples: Vec<serde_json::Value> = (0..samples)
            .map(|i| serde_json::json!({"t": i as f64, "bt": 150.0 + i as f64 * 0.1}))
            .collect();
        serde_json::from_value(serde_json::json!({
            "id": Uuid::new_v4(),
            "title": title,
            "roasted_at": at,
            "sampling_interval_s": 1.0,
            "batch": {},
            "origin": {"kind": "recorded"},
            "samples": samples,
            "events": [],
            "control_log": [],
            "notes": [],
        }))
        .unwrap()
    }

    fn summary(r: &Roast) -> serde_json::Value {
        serde_json::json!({"id": r.id, "title": r.title})
    }

    #[test]
    fn roasts_are_saved_listed_newest_first_replaced_and_deleted() {
        let s = Store::in_memory().unwrap();
        let a = roast("first", "2026-11-08T10:00:00Z", 3);
        let mut b = roast("second", "2026-11-09T10:00:00Z", 5);
        s.save_roast(&a, &summary(&a), Some("a.roast.json"))
            .unwrap();
        s.save_roast(&b, &summary(&b), Some("b.roast.json"))
            .unwrap();

        let titles: Vec<String> = s
            .roast_summaries()
            .unwrap()
            .iter()
            .map(|v| v["title"].as_str().unwrap().to_owned())
            .collect();
        assert_eq!(titles, ["second", "first"]);

        // Saving again replaces the roast and keeps its file name.
        b.title = "second, renamed".into();
        s.save_roast(&b, &summary(&b), None).unwrap();
        let stored = s.roast(b.id).unwrap().unwrap();
        assert_eq!(stored.roast, b);
        assert_eq!(stored.file_name.as_deref(), Some("b.roast.json"));
        assert_eq!(s.roast_summaries().unwrap().len(), 2);

        assert_eq!(s.same_roast(a.roasted_at, 3).unwrap(), Some(a.id));
        assert_eq!(s.same_roast(a.roasted_at, 4).unwrap(), None);
        assert_eq!(s.roast_file_names().unwrap().len(), 2);

        assert_eq!(
            s.resummarise(|r| serde_json::json!({"title": r.title.to_uppercase()}))
                .unwrap(),
            2
        );
        assert_eq!(s.roast_summaries().unwrap()[1]["title"], "FIRST");

        assert!(s.delete_roast(a.id).unwrap());
        assert!(!s.delete_roast(a.id).unwrap());
        assert!(s.roast(a.id).unwrap().is_none());
    }

    #[test]
    fn the_inventory_comes_back_as_saved_and_keeps_its_references() {
        let s = Store::in_memory().unwrap();
        let at = "2026-11-01T12:00:00Z".parse().unwrap();
        let lot = GreenLot::new("Yellow Bourbon", at);
        let other = GreenLot::new("Castillo", at);
        let inv = Inventory {
            movements: vec![
                StockMovement::purchase(other.id, 1000.0, at),
                StockMovement::purchase(lot.id, 5000.0, at),
            ],
            lots: vec![lot.clone(), other],
        };
        s.save_inventory(&inv).unwrap();
        assert_eq!(s.inventory().unwrap(), inv);

        // A movement of a lot that does not exist is refused; nothing changes.
        let mut broken = inv.clone();
        broken
            .movements
            .push(StockMovement::purchase(Uuid::new_v4(), 1.0, at));
        assert!(s.save_inventory(&broken).is_err());
        assert_eq!(s.inventory().unwrap(), inv);
    }

    #[test]
    fn experiments_are_saved_replaced_listed_newest_first_and_deleted() {
        use roast_core::{ChangedVariable, ExperimentStatus};
        let s = Store::in_memory().unwrap();
        let change = |d: &str| ChangedVariable {
            description: d.into(),
            structured: None,
        };
        let old = Experiment::new(
            "A hotter charge shortens drying",
            change("charge 10 °C hotter"),
            "2026-11-01T12:00:00Z".parse().unwrap(),
        );
        let mut new = Experiment::new(
            "More air after first crack keeps it clean",
            change("air +10 after first crack"),
            "2026-11-05T12:00:00Z".parse().unwrap(),
        );
        s.save_experiment(&old).unwrap();
        s.save_experiment(&new).unwrap();
        assert_eq!(s.experiments().unwrap(), [new.clone(), old.clone()]);

        new.status = ExperimentStatus::Done;
        new.conclusion = Some("cleaner cup, 5 s shorter".into());
        new.roast_ids.push(Uuid::new_v4());
        s.save_experiment(&new).unwrap();
        assert_eq!(s.experiments().unwrap(), [new.clone(), old.clone()]);

        assert!(s.delete_experiment(old.id).unwrap());
        assert!(!s.delete_experiment(old.id).unwrap());
        assert_eq!(s.experiments().unwrap(), [new]);
    }

    #[test]
    fn a_database_of_schema_version_1_gets_the_later_tables() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("roastmcp.sqlite");
        {
            let conn = Connection::open(&db).unwrap();
            conn.execute_batch(MIGRATIONS[0]).unwrap();
            conn.pragma_update(None, "user_version", 1).unwrap();
            conn.pragma_update(None, "application_id", APPLICATION_ID)
                .unwrap();
            conn.execute(
                "INSERT INTO settings (key, value) VALUES ('answer', '42')",
                [],
            )
            .unwrap();
        }
        let s = Store::open(&db).unwrap();
        assert_eq!(s.setting::<u32>("answer").unwrap(), Some(42));
        assert!(s.experiments().unwrap().is_empty());
        let version: i64 = s
            .conn()
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(usize::try_from(version).unwrap(), MIGRATIONS.len());
    }

    #[test]
    fn settings_are_kept_by_key() {
        let s = Store::in_memory().unwrap();
        assert_eq!(s.setting::<u32>("answer").unwrap(), None);
        s.set_setting("answer", &41).unwrap();
        s.set_setting("answer", &42).unwrap();
        assert_eq!(s.setting::<u32>("answer").unwrap(), Some(42));
    }

    #[test]
    fn a_backup_is_checked_rotated_and_restores_the_data() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("data").join("roastmcp.sqlite");
        let backups_dir = dir.path().join("nas");
        let a = roast("kept", "2026-11-08T10:00:00Z", 3);
        let report = {
            let s = Store::open(&db).unwrap();
            s.save_roast(&a, &summary(&a), None).unwrap();
            // Older backups: only the newest two files stay.
            std::fs::create_dir_all(&backups_dir).unwrap();
            for old in ["20200101-000000", "20200102-000000", "20200103-000000"] {
                std::fs::write(backups_dir.join(format!("roastmcp-{old}.sqlite")), "x").unwrap();
            }
            std::fs::write(backups_dir.join("notes.txt"), "mine").unwrap();
            let report = s.backup(&backups_dir, 2).unwrap();
            // Lost after the backup:
            s.delete_roast(a.id).unwrap();
            report
        };
        assert_eq!(report.removed, 2);
        assert!(report.bytes > 0);
        let left = backups(&backups_dir).unwrap();
        assert_eq!(left.len(), 2);
        assert_eq!(left[0], report.path);
        assert!(left[1].ends_with("roastmcp-20200103-000000.sqlite"));
        assert!(backups_dir.join("notes.txt").exists());

        // Not a backup: refused, and the database stays as it is.
        assert!(matches!(
            restore(&db, &backups_dir.join("notes.txt")),
            Err(Error::Database(_) | Error::NotOurs(_))
        ));
        assert!(db.exists());

        let kept = restore(&db, &report.path).unwrap().unwrap();
        assert!(kept.exists());
        let s = Store::open(&db).unwrap();
        assert_eq!(s.roast(a.id).unwrap().unwrap().roast, a);
    }

    #[test]
    fn a_database_of_a_newer_version_is_not_touched() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("roastmcp.sqlite");
        drop(Store::open(&db).unwrap());
        Connection::open(&db)
            .unwrap()
            .pragma_update(None, "user_version", 99)
            .unwrap();
        assert!(matches!(
            Store::open(&db),
            Err(Error::Newer { found: 99, .. })
        ));
        assert!(matches!(verify(&db), Err(Error::Newer { .. })));
    }
}
