use std::fmt;

use serde::{Deserialize, Serialize};

/// The landmarks of a roast.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    /// Beans go into the preheated drum. Starts the roast clock.
    Charge,
    /// Lowest bean temperature after CHARGE.
    TurningPoint,
    /// End of the drying phase (colour change to yellow).
    DryEnd,
    FirstCrackStart,
    FirstCrackEnd,
    SecondCrackStart,
    SecondCrackEnd,
    /// Beans leave the drum.
    Drop,
    /// Cooling finished.
    CoolEnd,
}

impl EventKind {
    /// Every event kind in roast order.
    pub const ALL: [EventKind; 9] = [
        EventKind::Charge,
        EventKind::TurningPoint,
        EventKind::DryEnd,
        EventKind::FirstCrackStart,
        EventKind::FirstCrackEnd,
        EventKind::SecondCrackStart,
        EventKind::SecondCrackEnd,
        EventKind::Drop,
        EventKind::CoolEnd,
    ];

    /// Stable identifier, identical to the serialized form.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            EventKind::Charge => "charge",
            EventKind::TurningPoint => "turning_point",
            EventKind::DryEnd => "dry_end",
            EventKind::FirstCrackStart => "first_crack_start",
            EventKind::FirstCrackEnd => "first_crack_end",
            EventKind::SecondCrackStart => "second_crack_start",
            EventKind::SecondCrackEnd => "second_crack_end",
            EventKind::Drop => "drop",
            EventKind::CoolEnd => "cool_end",
        }
    }
}

impl fmt::Display for EventKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Who or what placed an event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventOrigin {
    Human,
    /// Detected from the temperature curve.
    Auto,
    /// Detected by a sensor-based detector (for example first crack from audio).
    Detector,
    Ai,
    /// Taken over from an imported file.
    Import,
}

/// An event on a recording.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RoastEvent {
    /// Seconds since the recording started.
    pub t: f64,
    pub kind: EventKind,
    pub origin: EventOrigin,
    /// Bean temperature at the event, if known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bt: Option<f64>,
}

/// A free-text observation on a recording.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Note {
    /// Seconds since the recording started; `None` for notes about the whole roast.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub t: Option<f64>,
    pub text: String,
    pub origin: EventOrigin,
}
