use serde::{Deserialize, Serialize};

use crate::ActuatorValues;

/// One sampling step of a recording.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Sample {
    /// Seconds since the recording started.
    pub t: f64,
    /// Bean temperature, °C. `None` when the reading failed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bt: Option<f64>,
    /// Environment (exhaust or drum air) temperature, °C, if the machine has a probe.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub et: Option<f64>,
    /// Actuator values as reported by the machine (not as commanded), if it
    /// reports them.
    #[serde(default, skip_serializing_if = "ActuatorValues::is_empty")]
    pub actuators: ActuatorValues,
}

impl Sample {
    /// A sample with only a bean temperature.
    #[must_use]
    pub fn bt(t: f64, bt: f64) -> Self {
        Sample {
            t,
            bt: Some(bt),
            et: None,
            actuators: ActuatorValues::new(),
        }
    }
}
