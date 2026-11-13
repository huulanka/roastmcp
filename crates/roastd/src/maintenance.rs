//! Care of the roaster: recurring maintenance tasks with the log of when
//! each was done, and the checklist a person goes through before a roast.
//!
//! A task is due after a number of roasts or days since it was last done
//! (`roast_core::MaintenanceTask`); only roasts on a real machine count.
//! The checklist is the person's list of things to check, followed by the
//! maintenance that is due. Nothing here touches the roaster: the checklist
//! informs, it does not gate a command.

use std::sync::Arc;

use chrono::{DateTime, Duration, Utc};
use roast_core::{DueReason, MaintenanceLogEntry, MaintenanceTask};
use roast_store::Store;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::engine::Fail;

/// The setting the checklist's own items are stored under.
const CHECKLIST_SETTING: &str = "checklist";

/// The most items the person's checklist holds.
const MAX_CHECKLIST_ITEMS: usize = 20;

/// The most log entries one request returns.
pub const LOG_MAX: usize = 500;

/// The checklist before anyone changed it: `(key, text)`. The key names a
/// built-in item, so the app can show it in the person's language.
const DEFAULT_CHECKLIST: &[(&str, &str)] = &[
    (
        "extinguisher",
        "A fire extinguisher or a fire blanket is within reach",
    ),
    ("chaff", "The chaff collector is empty"),
    (
        "surroundings",
        "Nothing that burns stands near the roaster and the exhaust is free",
    ),
    ("cooling", "The cooling tray is empty and in place"),
    ("stay", "I stay at the roaster until the beans are cooling"),
];

/// Body of `POST /api/maintenance/tasks`.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct NewTask {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    /// Only roasts on this machine count; every roast without it.
    #[serde(default)]
    pub machine_id: Option<String>,
    #[serde(default)]
    pub interval_roasts: Option<u32>,
    #[serde(default)]
    pub interval_days: Option<u32>,
}

/// Body of `PATCH /api/maintenance/tasks/{id}`; what is left out stays.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct TaskPatch {
    #[serde(default)]
    pub name: Option<String>,
    /// Empty text removes the description.
    #[serde(default)]
    pub description: Option<String>,
    /// 0 removes the interval.
    #[serde(default)]
    pub interval_roasts: Option<u32>,
    /// 0 removes the interval.
    #[serde(default)]
    pub interval_days: Option<u32>,
    #[serde(default)]
    pub archived: Option<bool>,
}

/// Body of `POST /api/maintenance/tasks/{id}/done`.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct DoneReq {
    /// When it was done; now if left out.
    #[serde(default)]
    pub done_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub note: Option<String>,
}

/// A task as the API gives it: with how long ago it was done and whether it
/// is due.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TaskView {
    #[serde(flatten)]
    pub task: MaintenanceTask,
    /// Roasts on a real machine since it was last done (or created).
    pub roasts_since: u32,
    /// Whole days since it was last done (or created).
    pub days_since: u32,
    /// Why it is due; `null` if it is not.
    pub due: Option<DueReason>,
}

/// A log entry with the name of its task.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LogView {
    #[serde(flatten)]
    pub entry: MaintenanceLogEntry,
    pub task_name: String,
}

/// One item of the person's checklist.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChecklistItem {
    /// Names a built-in item (the app translates it); none for the
    /// person's own.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    pub text: String,
}

/// The checklist before a roast.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Checklist {
    /// What the person checks every time.
    pub items: Vec<ChecklistItem>,
    /// The maintenance that is due.
    pub due: Vec<TaskView>,
}

fn db(e: roast_store::Error) -> Fail {
    Fail::internal(format!("the database failed: {e}"))
}

fn text(s: Option<String>) -> Option<String> {
    s.map(|t| t.trim().to_owned()).filter(|t| !t.is_empty())
}

fn interval(n: Option<u32>) -> Option<u32> {
    n.filter(|n| *n > 0)
}

fn default_checklist() -> Vec<ChecklistItem> {
    DEFAULT_CHECKLIST
        .iter()
        .map(|(key, text)| ChecklistItem {
            key: Some((*key).to_owned()),
            text: (*text).to_owned(),
        })
        .collect()
}

/// The maintenance tasks, their log and the checklist.
#[derive(Debug, Clone)]
pub struct Maintenance {
    store: Arc<Store>,
}

impl Maintenance {
    /// The maintenance in `store`.
    #[must_use]
    pub fn new(store: Arc<Store>) -> Self {
        Maintenance { store }
    }

    fn stored(&self, id: Uuid) -> Result<MaintenanceTask, Fail> {
        self.store
            .maintenance_tasks()
            .map_err(db)?
            .into_iter()
            .find(|t| t.id == id)
            .ok_or_else(|| Fail::not_found("no such maintenance task"))
    }

    fn view(&self, task: MaintenanceTask, now: DateTime<Utc>) -> Result<TaskView, Fail> {
        let roasts_since = self
            .store
            .real_roasts_after(task.baseline(), task.machine_id.as_deref())
            .map_err(db)?;
        Ok(TaskView {
            roasts_since,
            days_since: roast_core::rest_days_between(task.baseline(), now),
            due: task.due_reason(now, roasts_since),
            task,
        })
    }

    /// The tasks in the order they were created; archived ones only with
    /// `archived`.
    ///
    /// # Errors
    /// On a database error.
    pub fn list(&self, now: DateTime<Utc>, archived: bool) -> Result<Vec<TaskView>, Fail> {
        self.store
            .maintenance_tasks()
            .map_err(db)?
            .into_iter()
            .filter(|t| archived || !t.archived)
            .map(|t| self.view(t, now))
            .collect()
    }

    /// Adds a task.
    ///
    /// # Errors
    /// Without a name or on a database error.
    pub fn create(&self, new: NewTask, now: DateTime<Utc>) -> Result<TaskView, Fail> {
        let name = text(Some(new.name)).ok_or_else(|| Fail::bad("the task needs a name"))?;
        let mut t = MaintenanceTask::new(name, now);
        t.description = text(new.description);
        t.machine_id = text(new.machine_id);
        t.interval_roasts = interval(new.interval_roasts);
        t.interval_days = interval(new.interval_days);
        self.store.save_maintenance_task(&t).map_err(db)?;
        self.view(t, now)
    }

    /// Changes a task.
    ///
    /// # Errors
    /// For an unknown task, an emptied name, or on a database error.
    pub fn update(&self, id: Uuid, p: TaskPatch, now: DateTime<Utc>) -> Result<TaskView, Fail> {
        let mut t = self.stored(id)?;
        if let Some(n) = p.name {
            t.name = text(Some(n)).ok_or_else(|| Fail::bad("the name cannot be empty"))?;
        }
        if let Some(d) = p.description {
            t.description = text(Some(d));
        }
        if let Some(n) = p.interval_roasts {
            t.interval_roasts = interval(Some(n));
        }
        if let Some(n) = p.interval_days {
            t.interval_days = interval(Some(n));
        }
        if let Some(a) = p.archived {
            t.archived = a;
        }
        self.store.save_maintenance_task(&t).map_err(db)?;
        self.view(t, now)
    }

    /// Writes down that a task was done; its intervals count from then.
    ///
    /// # Errors
    /// For an unknown task, a moment in the future, or on a database error.
    pub fn done(&self, id: Uuid, req: DoneReq, now: DateTime<Utc>) -> Result<TaskView, Fail> {
        let mut t = self.stored(id)?;
        let done_at = req.done_at.unwrap_or(now);
        if done_at > now + Duration::minutes(1) {
            return Err(Fail::bad("a task cannot be done in the future"));
        }
        let entry = MaintenanceLogEntry::new(t.id, done_at, text(req.note));
        self.store.add_maintenance_log(&entry).map_err(db)?;
        if t.last_done_at.is_none_or(|last| done_at > last) {
            t.last_done_at = Some(done_at);
            self.store.save_maintenance_task(&t).map_err(db)?;
        }
        self.view(t, now)
    }

    /// The log, newest first: of one task or of all, at most `limit`
    /// entries ([`LOG_MAX`] at most).
    ///
    /// # Errors
    /// For an unknown task or on a database error.
    pub fn log(&self, task: Option<Uuid>, limit: usize) -> Result<Vec<LogView>, Fail> {
        let tasks = self.store.maintenance_tasks().map_err(db)?;
        if let Some(id) = task
            && !tasks.iter().any(|t| t.id == id)
        {
            return Err(Fail::not_found("no such maintenance task"));
        }
        Ok(self
            .store
            .maintenance_log(task, limit.clamp(1, LOG_MAX))
            .map_err(db)?
            .into_iter()
            .map(|entry| LogView {
                task_name: tasks
                    .iter()
                    .find(|t| t.id == entry.task_id)
                    .map(|t| t.name.clone())
                    .unwrap_or_default(),
                entry,
            })
            .collect())
    }

    /// Takes a log entry back (it was entered by mistake); its task counts
    /// from the entry before it again.
    ///
    /// # Errors
    /// For an unknown entry or on a database error.
    pub fn delete_log(&self, id: Uuid, now: DateTime<Utc>) -> Result<TaskView, Fail> {
        let entry = self
            .store
            .maintenance_log_entry(id)
            .map_err(db)?
            .ok_or_else(|| Fail::not_found("no such log entry"))?;
        self.store.delete_maintenance_log(id).map_err(db)?;
        let mut t = self.stored(entry.task_id)?;
        t.last_done_at = self
            .store
            .maintenance_log(Some(t.id), 1)
            .map_err(db)?
            .first()
            .map(|e| e.done_at);
        self.store.save_maintenance_task(&t).map_err(db)?;
        self.view(t, now)
    }

    /// The person's checklist items: their own, or the built-in ones.
    fn checklist_items(&self) -> Result<Vec<ChecklistItem>, Fail> {
        Ok(self
            .store
            .setting(CHECKLIST_SETTING)
            .map_err(db)?
            .unwrap_or_else(default_checklist))
    }

    /// The checklist before a roast: the person's items and the maintenance
    /// that is due.
    ///
    /// # Errors
    /// On a database error.
    pub fn checklist(&self, now: DateTime<Utc>) -> Result<Checklist, Fail> {
        Ok(Checklist {
            items: self.checklist_items()?,
            due: self
                .list(now, false)?
                .into_iter()
                .filter(|t| t.due.is_some())
                .collect(),
        })
    }

    /// Replaces the person's checklist items. An item keeps its key only if
    /// its text is still the built-in one.
    ///
    /// # Errors
    /// With too many items or on a database error.
    pub fn set_checklist(
        &self,
        items: Vec<ChecklistItem>,
        now: DateTime<Utc>,
    ) -> Result<Checklist, Fail> {
        let items: Vec<ChecklistItem> = items
            .into_iter()
            .filter_map(|i| {
                let text = text(Some(i.text))?;
                let key = i.key.filter(|k| {
                    DEFAULT_CHECKLIST
                        .iter()
                        .any(|(key, built_in)| key == k && *built_in == text)
                });
                Some(ChecklistItem { key, text })
            })
            .collect();
        if items.len() > MAX_CHECKLIST_ITEMS {
            return Err(Fail::bad(format!(
                "at most {MAX_CHECKLIST_ITEMS} checklist items"
            )));
        }
        self.store
            .set_setting(CHECKLIST_SETTING, &items)
            .map_err(db)?;
        self.checklist(now)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use crate::files::Archive;
    use roast_core::Roast;

    fn at(t: &str) -> DateTime<Utc> {
        t.parse().unwrap()
    }

    fn roast(roasted_at: &str, simulated: bool) -> Roast {
        serde_json::from_value(serde_json::json!({
            "id": Uuid::new_v4(),
            "title": "r",
            "roasted_at": roasted_at,
            "machine": "skywalker-cubean-plus",
            "sampling_interval_s": 1.0,
            "batch": {},
            "origin": {"kind": if simulated { "simulated" } else { "recorded" }},
            "samples": [{"t": 0.0, "bt": 180.0}, {"t": 1.0, "bt": 179.0}],
        }))
        .unwrap()
    }

    fn setup() -> (tempfile::TempDir, Archive, Maintenance) {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(Store::in_memory().unwrap());
        let archive = Archive::new(store.clone(), dir.path().join("roasts"));
        (dir, archive, Maintenance::new(store))
    }

    #[test]
    fn a_task_comes_due_by_roasts_or_days_and_starts_over_when_done() {
        let (_dir, archive, m) = setup();
        let created = at("2026-11-01T12:00:00Z");
        assert_eq!(
            m.create(NewTask::default(), created).unwrap_err().status,
            400
        );
        let chaff = m
            .create(
                NewTask {
                    name: " Empty the chaff collector ".into(),
                    interval_roasts: Some(2),
                    interval_days: Some(0),
                    ..NewTask::default()
                },
                created,
            )
            .unwrap();
        assert_eq!(chaff.task.name, "Empty the chaff collector");
        assert_eq!(chaff.task.interval_days, None);
        let probe = m
            .create(
                NewTask {
                    name: "Check the bean probe".into(),
                    interval_days: Some(30),
                    machine_id: Some("another-roaster".into()),
                    ..NewTask::default()
                },
                created,
            )
            .unwrap();

        // Two real roasts make the chaff due; the simulator's never count.
        archive.save(&roast("2026-11-02T18:00:00Z", false)).unwrap();
        archive.save(&roast("2026-11-03T18:00:00Z", true)).unwrap();
        let now = at("2026-11-04T08:00:00Z");
        assert!(m.list(now, false).unwrap().iter().all(|t| t.due.is_none()));
        archive.save(&roast("2026-11-04T18:00:00Z", false)).unwrap();
        let now = at("2026-11-05T08:00:00Z");
        let tasks = m.list(now, false).unwrap();
        assert_eq!(tasks[0].roasts_since, 2);
        assert_eq!(
            tasks[0].due,
            Some(DueReason::Roasts {
                since: 2,
                interval: 2
            })
        );
        // The other task counts another machine's roasts, and its days.
        assert_eq!((tasks[1].roasts_since, tasks[1].days_since), (0, 3));
        assert_eq!(tasks[1].due, None);
        assert_eq!(m.checklist(now).unwrap().due, [tasks[0].clone()]);

        // Done: it counts from now on.
        let done = m
            .done(
                chaff.task.id,
                DoneReq {
                    note: Some(" quite full ".into()),
                    ..DoneReq::default()
                },
                now,
            )
            .unwrap();
        assert_eq!((done.roasts_since, done.due), (0, None));
        assert_eq!(done.task.last_done_at, Some(now));
        let future = DoneReq {
            done_at: Some(now + Duration::days(1)),
            ..DoneReq::default()
        };
        assert_eq!(m.done(chaff.task.id, future, now).unwrap_err().status, 400);
        assert_eq!(
            m.done(Uuid::new_v4(), DoneReq::default(), now)
                .unwrap_err()
                .status,
            404
        );
        // An older entry entered later does not move the baseline back.
        let earlier = DoneReq {
            done_at: Some(at("2026-11-03T08:00:00Z")),
            ..DoneReq::default()
        };
        let t = m.done(chaff.task.id, earlier, now).unwrap();
        assert_eq!(t.task.last_done_at, Some(now));

        let log = m.log(None, 10).unwrap();
        assert_eq!(log.len(), 2);
        assert_eq!(log[0].task_name, "Empty the chaff collector");
        assert_eq!(log[0].entry.note.as_deref(), Some("quite full"));
        assert_eq!(m.log(Some(probe.task.id), 10).unwrap(), []);
        assert_eq!(m.log(Some(Uuid::new_v4()), 10).unwrap_err().status, 404);

        // Taking the newest entry back: the task counts from the one before.
        let t = m.delete_log(log[0].entry.id, now).unwrap();
        assert_eq!(t.task.last_done_at, Some(at("2026-11-03T08:00:00Z")));
        assert_eq!(t.roasts_since, 1);
        assert_eq!(m.delete_log(log[0].entry.id, now).unwrap_err().status, 404);

        // Days make the probe due; an archived task is out of the list.
        let later = at("2026-12-05T08:00:00Z");
        let tasks = m.list(later, false).unwrap();
        assert_eq!(
            tasks[1].due,
            Some(DueReason::Days {
                since: 33,
                interval: 30
            })
        );
        let patch = TaskPatch {
            archived: Some(true),
            interval_days: Some(0),
            description: Some("with a thermometer".into()),
            ..TaskPatch::default()
        };
        let t = m.update(probe.task.id, patch, later).unwrap();
        assert_eq!((t.task.interval_days, t.due), (None, None));
        assert_eq!(m.list(later, false).unwrap().len(), 1);
        assert_eq!(m.list(later, true).unwrap().len(), 2);
        let empty = TaskPatch {
            name: Some(" ".into()),
            ..TaskPatch::default()
        };
        assert_eq!(
            m.update(probe.task.id, empty, later).unwrap_err().status,
            400
        );
    }

    #[test]
    fn the_checklist_starts_with_the_built_in_items_and_takes_the_persons_own() {
        let (_dir, _archive, m) = setup();
        let now = at("2026-11-05T08:00:00Z");
        let list = m.checklist(now).unwrap();
        assert_eq!(list.items.len(), DEFAULT_CHECKLIST.len());
        assert_eq!(list.items[0].key.as_deref(), Some("extinguisher"));
        assert!(list.due.is_empty());

        let mut items = list.items[..2].to_vec();
        // A built-in item with other words is the person's own from then on.
        items[1].text = "Chaff collector emptied and brushed out".into();
        items.push(ChecklistItem {
            key: Some("made-up".into()),
            text: " Window open ".into(),
        });
        items.push(ChecklistItem {
            key: None,
            text: "  ".into(),
        });
        let list = m.set_checklist(items, now).unwrap();
        assert_eq!(
            list.items,
            [
                ChecklistItem {
                    key: Some("extinguisher".into()),
                    text: DEFAULT_CHECKLIST[0].1.into(),
                },
                ChecklistItem {
                    key: None,
                    text: "Chaff collector emptied and brushed out".into(),
                },
                ChecklistItem {
                    key: None,
                    text: "Window open".into(),
                },
            ]
        );
        assert_eq!(m.checklist(now).unwrap(), list);
        let many = vec![list.items[2].clone(); MAX_CHECKLIST_ITEMS + 1];
        assert_eq!(m.set_checklist(many, now).unwrap_err().status, 400);
    }
}
