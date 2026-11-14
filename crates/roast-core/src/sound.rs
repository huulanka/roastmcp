//! What the microphone heard during a roast (ADR 0021): the pops, and the
//! level of the two bands once a second. No sound itself.

use serde::{Deserialize, Serialize};

/// Which band a pop was heard in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PopBand {
    /// 0.4–4 kHz: first crack.
    Crack,
    /// 6–16 kHz: second crack.
    High,
}

/// One pop on the recording clock.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct SoundPop {
    /// Seconds since the recording started.
    pub t: f64,
    pub band: PopBand,
    /// How far it rose above the band's noise, dB.
    pub db: f32,
}

/// The bands' level at a moment, dBFS.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct SoundLevel {
    pub t: f64,
    pub crack_db: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub high_db: Option<f32>,
}

/// What the microphone heard during a roast.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RoastSound {
    /// The input device.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device: Option<String>,
    #[serde(default)]
    pub pops: Vec<SoundPop>,
    /// One level a second.
    #[serde(default)]
    pub levels: Vec<SoundLevel>,
}
