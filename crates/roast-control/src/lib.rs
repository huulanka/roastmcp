//! Controllers that move the burner on their own: the preheat, the recipe
//! runner, and the autopilot that follows a target profile later (ADR 0004,
//! ADR 0015, ADR 0019).
//!
//! They only decide; every decision goes through the safety layer.

pub mod preheat;
pub mod recipe;

pub use preheat::{
    PlantEstimate, PreheatController, PreheatDecision, PreheatInput, PreheatPhase, PreheatReason,
    PreheatTuning,
};
pub use recipe::{
    Outlook, RECIPE_GRACE_S, RecipeError, RecipeInput, RecipeRun, RunStep, StepChange, StepState,
    StepView, trigger_label,
};
