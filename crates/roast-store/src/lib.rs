//! SQLite storage and backups ([ADR 0003]).
//!
//! [`Store`] is the database: one SQLite file on the local disk, in WAL mode
//! with foreign keys on, opened by the daemon alone. It holds the roasts, the
//! green coffee inventory, the profiles, the experiments, the cuppings, the
//! maintenance tasks with their log and the settings;
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
use roast_core::{
    Cupping, Experiment, Inventory, MaintenanceLogEntry, MaintenanceTask, Profile, ProfileVersion,
    Roast,
};
use rusqlite::{Connection, OpenFlags, OptionalExtension, params};
use serde::Serialize;
use serde::de::DeserializeOwned;
use uuid::Uuid;

/// Schema versions in order; version `n` is reached by running the first `n`.
const MIGRATIONS: &[&str] = &[
    include_str!("schema.sql"),
    include_str!("migrations/0002_experiments.sql"),
    include_str!("migrations/0003_cupping_care.sql"),
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

    // ------------------------------------------------------------ cuppings

    /// The cuppings of one roast, or all of them; newest first.
    ///
    /// # Errors
    ///
    /// Fails on a database error or an undecodable record.
    pub fn cuppings(&self, roast: Option<Uuid>) -> Result<Vec<Cupping>> {
        let conn = self.conn();
        match roast {
            Some(id) => column_with(
                &conn,
                "SELECT cupping FROM cuppings WHERE roast_id = ?1 ORDER BY cupped_at DESC, id",
                params![id.to_string()],
            ),
            None => column(
                &conn,
                "SELECT cupping FROM cuppings ORDER BY cupped_at DESC, id",
            ),
        }
    }

    /// The cupping with this id.
    ///
    /// # Errors
    ///
    /// Fails on a database error or an undecodable record.
    pub fn cupping(&self, id: Uuid) -> Result<Option<Cupping>> {
        Ok(column_with(
            &self.conn(),
            "SELECT cupping FROM cuppings WHERE id = ?1",
            params![id.to_string()],
        )?
        .pop())
    }

    /// Saves a cupping (new, or replacing the stored one with its id).
    ///
    /// # Errors
    ///
    /// Fails on a database error or if its roast does not exist.
    pub fn save_cupping(&self, c: &Cupping) -> Result<()> {
        self.conn().execute(
            "INSERT INTO cuppings (id, roast_id, cupped_at, cupping)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT (id) DO UPDATE SET
                roast_id = excluded.roast_id,
                cupped_at = excluded.cupped_at,
                cupping = excluded.cupping",
            params![
                c.id.to_string(),
                c.roast_id.to_string(),
                instant(c.cupped_at),
                serde_json::to_string(c)?
            ],
        )?;
        Ok(())
    }

    /// Removes a cupping; `false` if there was none with this id.
    ///
    /// # Errors
    ///
    /// Fails on a database error.
    pub fn delete_cupping(&self, id: Uuid) -> Result<bool> {
        let n = self
            .conn()
            .execute("DELETE FROM cuppings WHERE id = ?1", [id.to_string()])?;
        Ok(n > 0)
    }

    // --------------------------------------------------------- maintenance

    /// All maintenance tasks, in the order they were created.
    ///
    /// # Errors
    ///
    /// Fails on a database error or an undecodable record.
    pub fn maintenance_tasks(&self) -> Result<Vec<MaintenanceTask>> {
        column(
            &self.conn(),
            "SELECT task FROM maintenance_tasks ORDER BY pos, id",
        )
    }

    /// Saves a maintenance task (new at the end, or replacing the stored one
    /// with its id in its place).
    ///
    /// # Errors
    ///
    /// Fails on a database error.
    pub fn save_maintenance_task(&self, t: &MaintenanceTask) -> Result<()> {
        self.conn().execute(
            "INSERT INTO maintenance_tasks (id, pos, name, archived, task)
             VALUES (?1, (SELECT coalesce(max(pos), -1) + 1 FROM maintenance_tasks), ?2, ?3, ?4)
             ON CONFLICT (id) DO UPDATE SET
                name = excluded.name,
                archived = excluded.archived,
                task = excluded.task",
            params![
                t.id.to_string(),
                t.name,
                t.archived,
                serde_json::to_string(t)?
            ],
        )?;
        Ok(())
    }

    /// The maintenance log of one task, or of all; newest first, at most
    /// `limit` entries.
    ///
    /// # Errors
    ///
    /// Fails on a database error or an undecodable record.
    pub fn maintenance_log(
        &self,
        task: Option<Uuid>,
        limit: usize,
    ) -> Result<Vec<MaintenanceLogEntry>> {
        let conn = self.conn();
        match task {
            Some(id) => column_with(
                &conn,
                "SELECT entry FROM maintenance_log WHERE task_id = ?1
                 ORDER BY done_at DESC, id LIMIT ?2",
                params![id.to_string(), int(limit)],
            ),
            None => column_with(
                &conn,
                "SELECT entry FROM maintenance_log ORDER BY done_at DESC, id LIMIT ?1",
                params![int(limit)],
            ),
        }
    }

    /// The log entry with this id.
    ///
    /// # Errors
    ///
    /// Fails on a database error or an undecodable record.
    pub fn maintenance_log_entry(&self, id: Uuid) -> Result<Option<MaintenanceLogEntry>> {
        Ok(column_with(
            &self.conn(),
            "SELECT entry FROM maintenance_log WHERE id = ?1",
            params![id.to_string()],
        )?
        .pop())
    }

    /// Writes down that a task was done.
    ///
    /// # Errors
    ///
    /// Fails on a database error or if its task does not exist.
    pub fn add_maintenance_log(&self, e: &MaintenanceLogEntry) -> Result<()> {
        self.conn().execute(
            "INSERT INTO maintenance_log (id, task_id, done_at, entry) VALUES (?1, ?2, ?3, ?4)",
            params![
                e.id.to_string(),
                e.task_id.to_string(),
                instant(e.done_at),
                serde_json::to_string(e)?
            ],
        )?;
        Ok(())
    }

    /// Removes a log entry; `false` if there was none with this id.
    ///
    /// # Errors
    ///
    /// Fails on a database error.
    pub fn delete_maintenance_log(&self, id: Uuid) -> Result<bool> {
        let n = self.conn().execute(
            "DELETE FROM maintenance_log WHERE id = ?1",
            [id.to_string()],
        )?;
        Ok(n > 0)
    }

    // -------------------------------------------------------------- roasts

    /// The roasts recorded at or after `from`, newest first.
    ///
    /// # Errors
    ///
    /// Fails on a database error or an undecodable roast.
    pub fn roasts_since(&self, from: DateTime<Utc>) -> Result<Vec<Roast>> {
        column_with(
            &self.conn(),
            "SELECT roast FROM roasts WHERE roasted_at >= ?1 ORDER BY roasted_at DESC, id",
            params![instant(from)],
        )
    }

    /// How many roasts on a real machine were recorded after `after`; with
    /// `machine`, only those on that machine. Simulator roasts never count.
    ///
    /// # Errors
    ///
    /// Fails on a database error.
    pub fn real_roasts_after(&self, after: DateTime<Utc>, machine: Option<&str>) -> Result<u32> {
        let n: i64 = self.conn().query_row(
            "SELECT count(*) FROM roasts
             WHERE roasted_at > ?1
               AND coalesce(json_extract(roast, '$.origin.kind'), '') <> 'simulated'
               AND (?2 IS NULL OR json_extract(roast, '$.machine') = ?2)",
            params![instant(after), machine],
            |r| r.get(0),
        )?;
        Ok(u32::try_from(n).unwrap_or(u32::MAX))
    }

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
    column_with(conn, sql, [])
}

/// The JSON values in the first column of a query with parameters.
fn column_with<T: DeserializeOwned>(
    conn: &Connection,
    sql: &str,
    params: impl rusqlite::Params,
) -> Result<Vec<T>> {
    let mut stmt = conn.prepare(sql)?;
    let rows = stmt.query_map(params, |r| r.get::<_, String>(0))?;
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
    fn cuppings_are_kept_per_roast_and_leave_with_it() {
        let s = Store::in_memory().unwrap();
        let a = roast("a", "2026-11-08T10:00:00Z", 3);
        let b = roast("b", "2026-11-09T10:00:00Z", 3);
        s.save_roast(&a, &summary(&a), None).unwrap();
        s.save_roast(&b, &summary(&b), None).unwrap();
        let early = Cupping::new(a.id, "2026-11-12T08:00:00Z".parse().unwrap());
        let mut late = Cupping::new(a.id, "2026-11-15T08:00:00Z".parse().unwrap());
        let other = Cupping::new(b.id, "2026-11-13T08:00:00Z".parse().unwrap());
        for c in [&early, &late, &other] {
            s.save_cupping(c).unwrap();
        }
        assert_eq!(
            s.cuppings(Some(a.id)).unwrap(),
            [late.clone(), early.clone()]
        );
        assert_eq!(s.cuppings(None).unwrap().len(), 3);

        late.scores.flavor = Some(8.0);
        late.descriptors = vec!["jasmine".into()];
        s.save_cupping(&late).unwrap();
        assert_eq!(s.cupping(late.id).unwrap(), Some(late.clone()));

        // A cupping of a roast that does not exist is refused.
        let orphan = Cupping::new(Uuid::new_v4(), early.cupped_at);
        assert!(s.save_cupping(&orphan).is_err());

        assert!(s.delete_cupping(early.id).unwrap());
        assert!(!s.delete_cupping(early.id).unwrap());
        // Saving the roast again keeps its cuppings; deleting it takes them.
        s.save_roast(&a, &summary(&a), None).unwrap();
        assert_eq!(s.cuppings(Some(a.id)).unwrap(), [late]);
        assert!(s.delete_roast(a.id).unwrap());
        assert_eq!(s.cuppings(None).unwrap(), [other]);
    }

    #[test]
    fn maintenance_tasks_keep_their_order_and_their_log() {
        let s = Store::in_memory().unwrap();
        let at = |t: &str| t.parse::<DateTime<Utc>>().unwrap();
        let mut chaff =
            MaintenanceTask::new("Empty the chaff collector", at("2026-11-01T12:00:00Z"));
        let drum = MaintenanceTask::new("Clean the drum", at("2026-11-01T12:00:00Z"));
        s.save_maintenance_task(&chaff).unwrap();
        s.save_maintenance_task(&drum).unwrap();
        chaff.interval_roasts = Some(1);
        chaff.archived = true;
        s.save_maintenance_task(&chaff).unwrap();
        assert_eq!(
            s.maintenance_tasks().unwrap(),
            [chaff.clone(), drum.clone()]
        );

        let first = MaintenanceLogEntry::new(chaff.id, at("2026-11-02T12:00:00Z"), None);
        let second =
            MaintenanceLogEntry::new(chaff.id, at("2026-11-04T12:00:00Z"), Some("full".into()));
        let third = MaintenanceLogEntry::new(drum.id, at("2026-11-03T12:00:00Z"), None);
        for e in [&first, &second, &third] {
            s.add_maintenance_log(e).unwrap();
        }
        let orphan = MaintenanceLogEntry::new(Uuid::new_v4(), first.done_at, None);
        assert!(s.add_maintenance_log(&orphan).is_err());
        assert_eq!(
            s.maintenance_log(None, 10).unwrap(),
            [second.clone(), third.clone(), first.clone()]
        );
        assert_eq!(
            s.maintenance_log(None, 1).unwrap(),
            std::slice::from_ref(&second)
        );
        assert_eq!(
            s.maintenance_log(Some(chaff.id), 10).unwrap(),
            [second.clone(), first.clone()]
        );
        assert_eq!(s.maintenance_log_entry(third.id).unwrap(), Some(third));
        assert!(s.delete_maintenance_log(second.id).unwrap());
        assert!(!s.delete_maintenance_log(second.id).unwrap());
        assert_eq!(s.maintenance_log(Some(chaff.id), 10).unwrap(), [first]);
    }

    #[test]
    fn roasts_are_counted_and_read_from_a_moment_on() {
        let s = Store::in_memory().unwrap();
        let mut old = roast("old", "2026-11-01T10:00:00Z", 3);
        old.machine = Some("skywalker-cubean-plus".into());
        let mut new = roast("new", "2026-11-09T10:00:00Z", 3);
        new.machine = Some("skywalker-cubean-plus".into());
        let other = roast("no machine", "2026-11-10T10:00:00Z", 3);
        let mut sim = roast("simulated", "2026-11-11T10:00:00Z", 3);
        sim.origin = roast_core::RoastOrigin::Simulated;
        for r in [&old, &new, &other, &sim] {
            s.save_roast(r, &summary(r), None).unwrap();
        }
        let from = "2026-11-05T00:00:00Z".parse().unwrap();
        assert_eq!(s.real_roasts_after(from, None).unwrap(), 2);
        assert_eq!(
            s.real_roasts_after(from, Some("skywalker-cubean-plus"))
                .unwrap(),
            1
        );
        assert_eq!(s.real_roasts_after(sim.roasted_at, None).unwrap(), 0);
        // Not the roast recorded at that very moment: it was there already.
        assert_eq!(s.real_roasts_after(new.roasted_at, None).unwrap(), 1);
        let titles: Vec<String> = s
            .roasts_since(from)
            .unwrap()
            .into_iter()
            .map(|r| r.title)
            .collect();
        assert_eq!(titles, ["simulated", "no machine", "new"]);
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
        assert!(s.cuppings(None).unwrap().is_empty());
        assert!(s.maintenance_tasks().unwrap().is_empty());
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
