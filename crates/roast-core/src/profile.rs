//! Roast profiles and their versions.
//!
//! A [`Profile`] is the stable identity ("Ethiopia washed, 120 g, light"); every
//! change to its plan creates a new immutable [`ProfileVersion`], so a roast can
//! always point at exactly the plan it followed. Versions form a tree through
//! [`ProfileVersion::parent_version`], which allows going back to an older
//! version and branching from it.
//!
//! Times in a target are **roast time**: seconds since CHARGE.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{Actuator, EventKind, Process};

/// A roast profile: what it is for, independent of the plan's versions.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Profile {
    pub id: Uuid,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Machine profile identifier the plan was made for, for example
    /// `skywalker-cubean-plus`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub machine_id: Option<String>,
    /// Beans the profile is meant for, in words ("Ethiopian washed heirloom").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub intended_beans: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub intended_process: Option<Process>,
    /// Green batch size the plan assumes, grams.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub batch_size_g: Option<f64>,
    #[serde(default)]
    pub archived: bool,
    pub created_at: DateTime<Utc>,
}

impl Profile {
    /// A new profile with a fresh id and no optional data.
    #[must_use]
    pub fn new(name: impl Into<String>, created_at: DateTime<Utc>) -> Self {
        Profile {
            id: Uuid::new_v4(),
            name: name.into(),
            description: None,
            machine_id: None,
            intended_beans: None,
            intended_process: None,
            batch_size_g: None,
            archived: false,
            created_at,
        }
    }
}

/// One immutable version of a profile's plan.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProfileVersion {
    pub id: Uuid,
    pub profile_id: Uuid,
    /// 1, 2, 3 … unique within the profile.
    pub version: u32,
    /// The version this one was derived from, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_version: Option<u32>,
    pub created_at: DateTime<Utc>,
    /// What changed and why.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub change_note: Option<String>,
    pub target: ProfileTarget,
}

/// The plan of a profile version: a target curve, planned actuator changes,
/// expected events, a drop rule and how much deviation is acceptable.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ProfileTarget {
    /// Target curve, ordered by roast time.
    #[serde(default)]
    pub points: Vec<TargetPoint>,
    /// Planned actuator changes.
    #[serde(default)]
    pub actions: Vec<PlannedAction>,
    #[serde(default)]
    pub events: EventTargets,
    #[serde(default)]
    pub tolerance: ToleranceBand,
}

/// One point of the target curve.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct TargetPoint {
    /// Seconds since CHARGE.
    pub roast_time_s: f64,
    /// Target bean temperature, °C.
    pub bt_c: f64,
    /// Target rate of rise, °C per minute, if the plan states one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ror_c_per_min: Option<f64>,
}

/// When a planned action fires.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "at", rename_all = "snake_case")]
pub enum ActionTrigger {
    /// At a roast time.
    Time { roast_time_s: f64 },
    /// When the bean temperature first reaches a value after the turning point.
    Bt { bt_c: f64 },
    /// A time after a roast event, marked or detected (ADR 0019): the step
    /// waits for the event, so it follows the beans rather than the clock.
    Event { event: RecipeAnchor, offset_s: f64 },
}

/// The events a recipe step can be tied to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecipeAnchor {
    DryEnd,
    FirstCrackStart,
}

impl RecipeAnchor {
    /// The roast event it stands for.
    #[must_use]
    pub fn kind(self) -> EventKind {
        match self {
            RecipeAnchor::DryEnd => EventKind::DryEnd,
            RecipeAnchor::FirstCrackStart => EventKind::FirstCrackStart,
        }
    }

    /// Short label for people: "DE" or "FC".
    #[must_use]
    pub fn short(self) -> &'static str {
        match self {
            RecipeAnchor::DryEnd => "DE",
            RecipeAnchor::FirstCrackStart => "FC",
        }
    }
}

/// Longest time a step may follow its event, seconds.
pub const MAX_EVENT_OFFSET_S: f64 = 900.0;

/// A planned actuator change, for example "air to 60 % at 160 °C".
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PlannedAction {
    pub trigger: ActionTrigger,
    pub actuator: Actuator,
    /// Percent `0.0..=100.0`, like every actuator value.
    pub value: f64,
}

/// Expected bean temperature and/or roast time of an event.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct EventTarget {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bt_c: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub roast_time_s: Option<f64>,
}

/// When to drop.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "rule", rename_all = "snake_case")]
pub enum DropRule {
    /// At a bean temperature.
    AtBt { bt_c: f64 },
    /// When the development time ratio (time since FC start / total roast time)
    /// reaches a percentage.
    AtDtr { dtr_pct: f64 },
    /// At a roast time.
    AtTime { roast_time_s: f64 },
}

/// Expected landmarks of the roast.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct EventTargets {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dry_end: Option<EventTarget>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_crack: Option<EventTarget>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub drop: Option<DropRule>,
}

/// How far a roast may deviate from the target curve and still count as
/// following it (± around the target).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ToleranceBand {
    /// Bean temperature, ± °C.
    pub bt_c: f64,
    /// Rate of rise, ± °C per minute, if checked at all.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ror_c_per_min: Option<f64>,
}

impl Default for ToleranceBand {
    /// ±5 °C bean temperature and ±2 °C/min rate of rise: a common starting
    /// point for small drum roasters, not a standard. Set per version.
    fn default() -> Self {
        ToleranceBand {
            bt_c: 5.0,
            ror_c_per_min: Some(2.0),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn target_json_shape() {
        let target = ProfileTarget {
            points: vec![TargetPoint {
                roast_time_s: 60.0,
                bt_c: 110.0,
                ror_c_per_min: Some(18.0),
            }],
            actions: vec![
                PlannedAction {
                    trigger: ActionTrigger::Bt { bt_c: 160.0 },
                    actuator: Actuator::Air,
                    value: 60.0,
                },
                PlannedAction {
                    trigger: ActionTrigger::Time {
                        roast_time_s: 300.0,
                    },
                    actuator: Actuator::Burner,
                    value: 70.0,
                },
                PlannedAction {
                    trigger: ActionTrigger::Event {
                        event: RecipeAnchor::FirstCrackStart,
                        offset_s: 30.0,
                    },
                    actuator: Actuator::Burner,
                    value: 50.0,
                },
            ],
            events: EventTargets {
                dry_end: Some(EventTarget {
                    bt_c: Some(150.0),
                    roast_time_s: Some(240.0),
                }),
                first_crack: None,
                drop: Some(DropRule::AtDtr { dtr_pct: 20.0 }),
            },
            tolerance: ToleranceBand::default(),
        };
        let json = serde_json::to_value(&target).unwrap();
        assert_eq!(json["actions"][0]["trigger"]["at"], "bt");
        assert_eq!(json["actions"][0]["actuator"], "air");
        assert_eq!(json["actions"][2]["trigger"]["at"], "event");
        assert_eq!(json["actions"][2]["trigger"]["event"], "first_crack_start");
        assert_eq!(json["actions"][2]["trigger"]["offset_s"], 30.0);
        assert_eq!(json["events"]["drop"]["rule"], "at_dtr");
        let back: ProfileTarget = serde_json::from_value(json).unwrap();
        assert_eq!(back, target);
    }

    #[test]
    fn empty_target_deserializes_with_defaults() {
        let t: ProfileTarget = serde_json::from_str("{}").unwrap();
        assert!(t.points.is_empty());
        assert_eq!(t.tolerance, ToleranceBand::default());
    }
}
