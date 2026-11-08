//! Experiments: one deliberate change, what was expected, what happened.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Where an experiment stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExperimentStatus {
    /// Written down, no roast yet.
    Planned,
    /// At least one roast done, conclusion still open.
    Running,
    /// Concluded.
    Done,
}

impl ExperimentStatus {
    /// Stable identifier, identical to the serialized form.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            ExperimentStatus::Planned => "planned",
            ExperimentStatus::Running => "running",
            ExperimentStatus::Done => "done",
        }
    }

    /// Parses the serialized form produced by [`ExperimentStatus::as_str`].
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        [
            ExperimentStatus::Planned,
            ExperimentStatus::Running,
            ExperimentStatus::Done,
        ]
        .into_iter()
        .find(|v| v.as_str() == s)
    }
}

/// A structured description of the changed variable, for comparisons and for
/// the AI. Values are kept as text so that any variable fits ("200" g, "washed").
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VariableChange {
    /// What was changed, for example `charge_bt_c` or "batch size".
    pub name: String,
    pub from: String,
    pub to: String,
}

/// The one variable an experiment changes.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ChangedVariable {
    /// In words, for example "charge 10 °C hotter".
    pub description: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub structured: Option<VariableChange>,
}

/// An experiment: a hypothesis tested by changing one variable over one or
/// more roasts.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Experiment {
    pub id: Uuid,
    pub hypothesis: String,
    pub changed_variable: ChangedVariable,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_outcome: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conclusion: Option<String>,
    pub status: ExperimentStatus,
    /// The profile version the change was applied to, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile_version_id: Option<Uuid>,
    /// Roasts that belong to the experiment.
    #[serde(default)]
    pub roast_ids: Vec<Uuid>,
    pub created_at: DateTime<Utc>,
}

impl Experiment {
    /// A planned experiment with a fresh id.
    #[must_use]
    pub fn new(
        hypothesis: impl Into<String>,
        changed_variable: ChangedVariable,
        created_at: DateTime<Utc>,
    ) -> Self {
        Experiment {
            id: Uuid::new_v4(),
            hypothesis: hypothesis.into(),
            changed_variable,
            expected_outcome: None,
            conclusion: None,
            status: ExperimentStatus::Planned,
            profile_version_id: None,
            roast_ids: Vec::new(),
            created_at,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_round_trip() {
        let mut e = Experiment::new(
            "A hotter charge shortens drying without scorching",
            ChangedVariable {
                description: "charge 10 °C hotter".into(),
                structured: Some(VariableChange {
                    name: "charge_bt_c".into(),
                    from: "200".into(),
                    to: "210".into(),
                }),
            },
            DateTime::UNIX_EPOCH,
        );
        e.roast_ids.push(Uuid::new_v4());
        let json = serde_json::to_value(&e).unwrap();
        assert_eq!(json["status"], "planned");
        let back: Experiment = serde_json::from_value(json).unwrap();
        assert_eq!(back, e);
        assert_eq!(
            ExperimentStatus::parse("done"),
            Some(ExperimentStatus::Done)
        );
    }
}
