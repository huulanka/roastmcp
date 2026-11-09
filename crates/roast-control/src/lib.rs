//! Controllers that move the burner on their own: the preheat now, the
//! autopilot that follows a target profile later (ADR 0004, ADR 0015).
//!
//! They only decide; every decision goes through the safety layer.

pub mod preheat;

pub use preheat::{
    PlantEstimate, PreheatController, PreheatDecision, PreheatInput, PreheatPhase, PreheatReason,
    PreheatTuning,
};
