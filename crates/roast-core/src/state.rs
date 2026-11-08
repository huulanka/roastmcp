use serde::{Deserialize, Serialize};

/// Where a session is in its life cycle.
///
/// `Disconnected → Connected → Recording → Charged → Dropped → Cooling → Finished`
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RoastState {
    /// No roaster connected.
    Disconnected,
    /// Connected and monitoring (for example preheating); nothing is recorded.
    Connected,
    /// Recording, waiting for CHARGE.
    Recording,
    /// Beans are in the drum.
    Charged,
    /// Beans have left the drum; recording continues.
    Dropped,
    /// Cooling is running.
    Cooling,
    /// The recording is complete and can be saved.
    Finished,
}

impl RoastState {
    /// Whether samples are being recorded.
    #[must_use]
    pub fn is_recording(self) -> bool {
        matches!(
            self,
            RoastState::Recording | RoastState::Charged | RoastState::Dropped | RoastState::Cooling
        )
    }

    /// Whether beans may be in the drum and hot.
    #[must_use]
    pub fn beans_in_drum(self) -> bool {
        matches!(self, RoastState::Charged)
    }
}
