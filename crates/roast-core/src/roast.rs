use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{ControlLogEntry, EventKind, Note, RoastEvent, RoastLevel, Sample};

/// Version of the `.roast.json` file format (ADR 0008). Bump on incompatible
/// changes and keep a reader for every older version.
pub const ROAST_FILE_SCHEMA_VERSION: u32 = 1;

/// A complete roast: metadata, curve, events, control log and notes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Roast {
    pub id: Uuid,
    pub title: String,
    /// Wall-clock time the recording started.
    pub roasted_at: DateTime<Utc>,
    /// Identifier of the machine profile used, for example `skywalker-cubean-plus`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub machine: Option<String>,
    pub sampling_interval_s: f64,
    #[serde(default)]
    pub batch: BatchInfo,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ambient: Option<Ambient>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub roast_degree: Option<RoastDegree>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub green_lot_id: Option<Uuid>,
    /// What the roaster aimed for (ADR 0014); guidance only, the safety layer
    /// never reads it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan: Option<RoastPlan>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile_version_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub experiment_id: Option<Uuid>,
    pub origin: RoastOrigin,
    pub samples: Vec<Sample>,
    #[serde(default)]
    pub events: Vec<RoastEvent>,
    #[serde(default)]
    pub control_log: Vec<ControlLogEntry>,
    #[serde(default)]
    pub notes: Vec<Note>,
    /// What the microphone heard (ADR 0021).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sound: Option<crate::sound::RoastSound>,
}

impl Roast {
    /// The first event of the given kind.
    #[must_use]
    pub fn event(&self, kind: EventKind) -> Option<&RoastEvent> {
        self.events.iter().find(|e| e.kind == kind)
    }

    /// Recording time of CHARGE, if marked.
    #[must_use]
    pub fn charge_t(&self) -> Option<f64> {
        self.event(EventKind::Charge).map(|e| e.t)
    }

    /// Converts a recording time to roast time (seconds since CHARGE). Before
    /// CHARGE is marked, recording time is returned unchanged.
    #[must_use]
    pub fn roast_time(&self, t: f64) -> f64 {
        self.charge_t().map_or(t, |c| t - c)
    }

    /// Weight loss in percent of the green weight, if both weights are known.
    #[must_use]
    pub fn weight_loss_pct(&self) -> Option<f64> {
        self.batch.weight_loss_pct()
    }
}

/// Batch weights and the green coffee used.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct BatchInfo {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub green_weight_g: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub roasted_weight_g: Option<f64>,
    /// Free-text bean name, for roasts without a linked green lot (for example imports).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub beans: Option<String>,
    /// The roaster's batch number, as on the bag's label (for example
    /// `BRYB-002`: the lot's code and its running number).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub batch_no: Option<String>,
}

impl BatchInfo {
    /// Weight loss in percent of the green weight.
    #[must_use]
    pub fn weight_loss_pct(&self) -> Option<f64> {
        match (self.green_weight_g, self.roasted_weight_g) {
            (Some(g), Some(r)) if g > 0.0 => Some((g - r) / g * 100.0),
            _ => None,
        }
    }
}

/// Ambient conditions during the roast.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Ambient {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temperature_c: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub humidity_pct: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pressure_hpa: Option<f64>,
}

/// Scale of a roast colour measurement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ColorScale {
    Agtron,
    Lighttells,
    Other,
}

/// Measured roast colour.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RoastDegree {
    pub scale: ColorScale,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub whole_bean: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ground: Option<f64>,
}

impl RoastDegree {
    /// Readings from this value up count as a light roast.
    pub const LIGHT_FROM: f64 = 70.0;
    /// Readings from this value up (below [`Self::LIGHT_FROM`]) count as a
    /// medium roast; lower ones as dark.
    pub const MEDIUM_FROM: f64 = 50.0;

    /// The rough roast level the measured colour stands for, on the Agtron
    /// "Gourmet" scale (higher is lighter), which Lighttells meters report
    /// too. The ground reading wins over the whole-bean one. `None` without
    /// a reading or on another scale.
    #[must_use]
    pub fn level(&self) -> Option<RoastLevel> {
        if self.scale == ColorScale::Other {
            return None;
        }
        let value = self.ground.or(self.whole_bean).filter(|v| v.is_finite())?;
        Some(if value >= Self::LIGHT_FROM {
            RoastLevel::Light
        } else if value >= Self::MEDIUM_FROM {
            RoastLevel::Medium
        } else {
            RoastLevel::Dark
        })
    }
}

/// What a roast is meant for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RoastGoal {
    Espresso,
    Filter,
    /// For espresso and filter alike.
    Omni,
    Other,
}

/// The plan of a roast: its goal, the reference roast it follows and the two
/// targets that matter while roasting (ADR 0014). Every part is optional.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RoastPlan {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub goal: Option<RoastGoal>,
    /// The saved roast whose curve is the plan line.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reference_id: Option<Uuid>,
    /// Bean temperature to drop at, °C.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub drop_bt_c: Option<f64>,
    /// Development time ratio to drop at: time from first crack to DROP in
    /// percent of the time from CHARGE to DROP.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dtr_pct: Option<f64>,
    /// The profile version followed; its target curve is the plan line and
    /// its recipe is announced during the roast. Wins over `reference_id`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile_version_id: Option<Uuid>,
}

impl RoastPlan {
    /// Whether nothing is set.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self == &RoastPlan::default()
    }
}

/// How a roast came into the archive.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RoastOrigin {
    /// Recorded live by RoastMCP.
    Recorded,
    /// Recorded in the simulator.
    Simulated,
    /// Imported from an Artisan `.alog` file.
    ImportedAlog {
        file_name: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        artisan_version: Option<String>,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::EventOrigin;

    #[test]
    fn a_measured_colour_gives_the_roast_level() {
        let degree = |scale, whole_bean, ground| RoastDegree {
            scale,
            whole_bean,
            ground,
        };
        let level = |d: RoastDegree| d.level();
        assert_eq!(
            level(degree(ColorScale::Agtron, Some(78.0), None)),
            Some(RoastLevel::Light)
        );
        // The ground reading wins over the whole bean.
        assert_eq!(
            level(degree(ColorScale::Lighttells, Some(48.0), Some(62.0))),
            Some(RoastLevel::Medium)
        );
        assert_eq!(
            level(degree(ColorScale::Agtron, Some(49.9), None)),
            Some(RoastLevel::Dark)
        );
        assert_eq!(level(degree(ColorScale::Agtron, None, None)), None);
        assert_eq!(level(degree(ColorScale::Other, Some(80.0), None)), None);
    }

    fn roast() -> Roast {
        Roast {
            id: Uuid::nil(),
            title: "test".into(),
            roasted_at: DateTime::<Utc>::UNIX_EPOCH,
            machine: None,
            sampling_interval_s: 1.0,
            batch: BatchInfo {
                green_weight_g: Some(200.0),
                roasted_weight_g: Some(170.0),
                beans: None,
                batch_no: None,
            },
            ambient: None,
            roast_degree: None,
            green_lot_id: None,
            plan: None,
            profile_version_id: None,
            experiment_id: None,
            origin: RoastOrigin::Recorded,
            samples: vec![Sample::bt(0.0, 200.0)],
            events: vec![RoastEvent {
                t: 30.0,
                kind: EventKind::Charge,
                origin: EventOrigin::Human,
                bt: Some(199.0),
            }],
            control_log: vec![],
            notes: vec![],
            sound: None,
        }
    }

    #[test]
    fn roast_time_starts_at_charge() {
        assert_eq!(roast().roast_time(90.0), 60.0);
    }

    #[test]
    fn weight_loss() {
        let loss = roast().weight_loss_pct().unwrap();
        assert!((loss - 15.0).abs() < 1e-9);
    }

    #[test]
    fn json_round_trip() {
        let r = roast();
        let json = serde_json::to_string(&r).unwrap();
        let back: Roast = serde_json::from_str(&json).unwrap();
        assert_eq!(r, back);
    }
}
