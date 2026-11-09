//! Domain types shared by every part of RoastMCP, and the roast analysis that
//! only depends on them (rate of rise, phases, event detection, metrics).
//!
//! Conventions used throughout:
//! - Temperatures are degrees Celsius (`f64`). Conversion to Fahrenheit happens
//!   only at the edges (display, import/export).
//! - Times on a recording are seconds since the recording started (`t`). The
//!   roast clock shown to people starts at CHARGE; use [`Roast::roast_time`].
//! - Actuator values are percent `0.0..=100.0`; switches use `0.0` (off) and
//!   `100.0` (on).

pub mod actuator;
pub mod analysis;
pub mod command;
pub mod cupping;
pub mod design;
pub mod event;
pub mod experiment;
pub mod inventory;
pub mod maintenance;
mod num;
pub mod profile;
pub mod reference;
pub mod roast;
pub mod ror;
pub mod sample;
pub mod state;
#[cfg(test)]
mod testutil;

pub use actuator::{Actuator, ActuatorValues};
pub use analysis::{DetectorConfig, EventDetector, Phases, phases};
pub use command::{
    ActuatorCommand, CommandOutcome, CommandRequest, CommandSource, ControlLogEntry, RejectReason,
};
pub use cupping::{
    Cupping, CuppingError, CuppingScores, RestWindow, RestWindows, RoastLevel, rest_days_between,
    rest_window,
};
pub use event::{EventKind, EventOrigin, Note, RoastEvent};
pub use experiment::{ChangedVariable, Experiment, ExperimentStatus, VariableChange};
pub use inventory::{
    Altitude, Certification, Decaf, Drying, Fermentation, GreenLot, HoneyGrade, Inventory,
    InventoryError, LotDetails, Packaging, Price, Process, Species, StockMovement,
    StockMovementKind, stock_g,
};
pub use maintenance::{DueReason, MaintenanceLogEntry, MaintenanceTask};
pub use profile::{
    ActionTrigger, DropRule, EventTarget, EventTargets, PlannedAction, Profile, ProfileTarget,
    ProfileVersion, TargetPoint, ToleranceBand,
};
pub use reference::{Milestone, Projection, RefPoint, ReferenceCurve, ReferenceKind, project};
pub use roast::{
    Ambient, BatchInfo, ColorScale, ROAST_FILE_SCHEMA_VERSION, Roast, RoastDegree, RoastGoal,
    RoastOrigin, RoastPlan,
};
pub use ror::{RorConfig, RorTracker, ror_series};
pub use sample::Sample;
pub use state::RoastState;

/// Converts degrees Fahrenheit to degrees Celsius.
#[must_use]
pub fn f_to_c(f: f64) -> f64 {
    (f - 32.0) * 5.0 / 9.0
}

/// Converts degrees Celsius to degrees Fahrenheit.
#[must_use]
pub fn c_to_f(c: f64) -> f64 {
    c * 9.0 / 5.0 + 32.0
}
