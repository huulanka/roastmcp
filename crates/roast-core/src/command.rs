use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::Actuator;

/// Who asked for an actuator change. The safety layer arbitrates by source:
/// `Human` and `Safety` always win over `Autopilot` and `Ai`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CommandSource {
    /// A person, through the app (or a hardware control the machine reports).
    Human,
    /// The safety layer itself (overheat guard, presence timeout, emergency stop).
    Safety,
    /// The deterministic controller following a target profile.
    Autopilot,
    /// An AI client, through MCP.
    Ai,
}

impl CommandSource {
    /// Whether commands from this source are subject to AI limits, the AI lease
    /// and presence confirmation.
    #[must_use]
    pub fn is_automated(self) -> bool {
        matches!(self, CommandSource::Autopilot | CommandSource::Ai)
    }
}

/// A command as it is sent to a driver, after the safety layer accepted it.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ActuatorCommand {
    pub actuator: Actuator,
    /// Percent `0.0..=100.0`; switches use `0.0` and `100.0`.
    pub value: f64,
}

/// A request to change an actuator, before the safety layer decided on it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CommandRequest {
    pub actuator: Actuator,
    pub value: f64,
    pub source: CommandSource,
    /// Why the change is requested. Required in practice for `Ai` and
    /// `Autopilot`; shown in the audit log and on the curve.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// Why the safety layer did not apply a request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RejectReason {
    /// An emergency stop is latched; only humans and the safety layer may act.
    EmergencyStopLatched,
    /// A human took over; AI control stays suspended until a human resumes it.
    AiSuspendedByHuman,
    /// AI control needs a valid lease.
    NoLease,
    /// AI control needs a recent presence confirmation.
    PresenceRequired,
    /// The value is outside the actuator's range and cannot be clamped.
    OutOfRange,
    /// Too soon after the previous change from this source.
    RateLimited,
    /// The machine does not have this actuator.
    NotSupported,
    /// No roaster is connected.
    Disconnected,
    /// The bean temperature is above the machine's limit; the burner may
    /// not be raised.
    Overheat,
}

/// What the safety layer did with a request.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum CommandOutcome {
    /// Sent to the driver. `clamped_from` is set when the requested value was
    /// limited (range or step size).
    Applied {
        value: f64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        clamped_from: Option<f64>,
    },
    /// Not sent.
    Rejected { reason: RejectReason },
    /// A critical step that waits for a human confirmation.
    AwaitingConfirmation { confirmation_id: uuid::Uuid },
}

/// One entry of the audit log: every request, whether applied or not.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ControlLogEntry {
    /// Seconds since the recording started, if a recording was running.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub t: Option<f64>,
    pub at: DateTime<Utc>,
    /// The actuator's value before the request, if known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous: Option<f64>,
    pub request: CommandRequest,
    pub outcome: CommandOutcome,
}
