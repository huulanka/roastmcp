//! Controllers that move the burner on their own: the preheat, the recipe
//! runner, and the autopilot's trim that keeps the roast on the profile's
//! curve between the recipe's steps (ADR 0004, ADR 0015, ADR 0019, ADR 0020).
//!
//! They only decide; every decision goes through the safety layer.

pub mod preheat;
pub mod recipe;
pub mod trim;

pub use preheat::{
    PlantEstimate, PreheatController, PreheatDecision, PreheatInput, PreheatPhase, PreheatReason,
    PreheatTuning,
};
pub use recipe::{
    Outlook, RECIPE_GRACE_S, RecipeError, RecipeInput, RecipeRun, RunStep, StepChange, StepState,
    StepView, trigger_label,
};
pub use trim::{Trim, TrimDecision, TrimHold, TrimInput, TrimTuning};
