use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{ControlLogEntry, EventKind, Note, RoastEvent, Sample};

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
            },
            ambient: None,
            roast_degree: None,
            green_lot_id: None,
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
