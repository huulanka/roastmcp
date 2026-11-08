use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Serialize};

/// The roles an actuator can play on a roaster. A machine profile declares
/// which of them a machine has and how to drive them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Actuator {
    /// Heat input (gas burner or electric heater), percent of maximum.
    Burner,
    /// Airflow through the drum (fan), percent.
    Air,
    /// Drum speed, percent of maximum speed.
    Drum,
    /// Bean cooling (cooling tray stirrer and fan). A switch.
    Cooling,
    /// Smoke or chaff filter fan, percent.
    Filter,
}

impl Actuator {
    /// Every actuator role, in display order.
    pub const ALL: [Actuator; 5] = [
        Actuator::Burner,
        Actuator::Air,
        Actuator::Drum,
        Actuator::Cooling,
        Actuator::Filter,
    ];

    /// Whether the actuator is an on/off switch rather than a continuous value.
    #[must_use]
    pub fn is_switch(self) -> bool {
        matches!(self, Actuator::Cooling)
    }

    /// Stable identifier, identical to the serialized form.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Actuator::Burner => "burner",
            Actuator::Air => "air",
            Actuator::Drum => "drum",
            Actuator::Cooling => "cooling",
            Actuator::Filter => "filter",
        }
    }
}

impl fmt::Display for Actuator {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Actuator values, for example as reported by a machine in a sample.
pub type ActuatorValues = BTreeMap<Actuator, f64>;
