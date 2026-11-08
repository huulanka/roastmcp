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
pub mod command;
pub mod event;
pub mod roast;
pub mod sample;
pub mod state;

pub use actuator::{Actuator, ActuatorValues};
pub use command::{
    ActuatorCommand, CommandOutcome, CommandRequest, CommandSource, ControlLogEntry, RejectReason,
};
pub use event::{EventKind, EventOrigin, Note, RoastEvent};
pub use roast::{
    Ambient, BatchInfo, ColorScale, ROAST_FILE_SCHEMA_VERSION, Roast, RoastDegree, RoastOrigin,
};
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
