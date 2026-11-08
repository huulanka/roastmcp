//! Maintenance tasks (empty the chaff collector, clean the drum, check the
//! probe …), when they were done and when they are due again.

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A recurring maintenance task.
///
/// A task is due after `interval_roasts` roasts or `interval_days` days since it
/// was last done, whichever comes first. A task that was never done counts from
/// its creation. A task without any interval is never due by itself (a
/// checklist item that is only logged).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MaintenanceTask {
    pub id: Uuid,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Only roasts on this machine count towards `interval_roasts`; `None`
    /// counts every roast.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub machine_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub interval_roasts: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub interval_days: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_done_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub archived: bool,
    pub created_at: DateTime<Utc>,
}

/// Why a task is due.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "by", rename_all = "snake_case")]
pub enum DueReason {
    /// Enough roasts since the task was last done.
    Roasts { since: u32, interval: u32 },
    /// Enough days since the task was last done.
    Days { since: u32, interval: u32 },
}

impl MaintenanceTask {
    /// A new task with a fresh id.
    #[must_use]
    pub fn new(name: impl Into<String>, created_at: DateTime<Utc>) -> Self {
        MaintenanceTask {
            id: Uuid::new_v4(),
            name: name.into(),
            description: None,
            machine_id: None,
            interval_roasts: None,
            interval_days: None,
            last_done_at: None,
            archived: false,
            created_at,
        }
    }

    /// The moment intervals count from: when the task was last done, or when it
    /// was created.
    #[must_use]
    pub fn baseline(&self) -> DateTime<Utc> {
        self.last_done_at.unwrap_or(self.created_at)
    }

    /// Why the task is due, or `None` if it is not.
    /// `roasts_since` is the number of roasts since [`MaintenanceTask::baseline`].
    #[must_use]
    pub fn due_reason(&self, now: DateTime<Utc>, roasts_since: u32) -> Option<DueReason> {
        if let Some(interval) = self.interval_roasts {
            if roasts_since >= interval {
                return Some(DueReason::Roasts {
                    since: roasts_since,
                    interval,
                });
            }
        }
        if let Some(interval) = self.interval_days {
            let elapsed = now - self.baseline();
            if elapsed >= Duration::days(i64::from(interval)) {
                let since = u32::try_from(elapsed.num_days()).unwrap_or(u32::MAX);
                return Some(DueReason::Days { since, interval });
            }
        }
        None
    }

    /// Whether the task is due. See [`MaintenanceTask::due_reason`].
    #[must_use]
    pub fn is_due(&self, now: DateTime<Utc>, roasts_since: u32) -> bool {
        self.due_reason(now, roasts_since).is_some()
    }
}

/// A record that a maintenance task was done.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MaintenanceLogEntry {
    pub id: Uuid,
    pub task_id: Uuid,
    pub done_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

impl MaintenanceLogEntry {
    /// A log entry with a fresh id.
    #[must_use]
    pub fn new(task_id: Uuid, done_at: DateTime<Utc>, note: Option<String>) -> Self {
        MaintenanceLogEntry {
            id: Uuid::new_v4(),
            task_id,
            done_at,
            note,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn task() -> MaintenanceTask {
        let mut t = MaintenanceTask::new("empty chaff collector", DateTime::UNIX_EPOCH);
        t.interval_roasts = Some(5);
        t.interval_days = Some(30);
        t
    }

    #[test]
    fn due_by_roasts() {
        let now = DateTime::UNIX_EPOCH + Duration::days(1);
        assert!(!task().is_due(now, 4));
        assert_eq!(
            task().due_reason(now, 5),
            Some(DueReason::Roasts {
                since: 5,
                interval: 5
            })
        );
    }

    #[test]
    fn due_by_days_counts_from_last_done() {
        let mut t = task();
        let now = DateTime::UNIX_EPOCH + Duration::days(31);
        assert_eq!(
            t.due_reason(now, 0),
            Some(DueReason::Days {
                since: 31,
                interval: 30
            })
        );
        t.last_done_at = Some(DateTime::UNIX_EPOCH + Duration::days(10));
        assert!(!t.is_due(now, 0));
    }

    #[test]
    fn task_without_interval_is_never_due() {
        let t = MaintenanceTask::new("check probe", DateTime::UNIX_EPOCH);
        assert!(!t.is_due(DateTime::UNIX_EPOCH + Duration::days(1000), 1000));
    }
}
