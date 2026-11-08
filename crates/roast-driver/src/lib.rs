//! Roaster drivers: the [`RoasterDriver`] trait, the generic TC4 driver for
//! serial and Bluetooth-SPP machines, and a simulator.
//!
//! Drivers are synchronous and run on a dedicated thread owned by the daemon.
//! They never decide anything: the daemon sends them only commands the safety
//! layer has approved.

mod simulator;
mod tc4;
mod transport;

pub use simulator::{SimulatedMachine, simulator_profile};
pub use tc4::{Tc4Driver, parse_read_reply};
pub use transport::{LineIo, ScriptedIo, SerialLineIo, list_ports};

use roast_core::{ActuatorCommand, ActuatorValues, EventKind};

/// One reading from the machine.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Reading {
    /// Board (ambient) temperature, °C.
    pub ambient_c: Option<f64>,
    /// Bean temperature, °C.
    pub bt: Option<f64>,
    /// Environment temperature, °C.
    pub et: Option<f64>,
    /// Actuator values the machine reported.
    pub actuators: ActuatorValues,
}

/// Errors of a driver.
#[derive(Debug, thiserror::Error)]
pub enum DriverError {
    /// The port could not be opened or failed.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    /// The machine did not answer in time.
    #[error("no reply from the roaster within {0} ms")]
    Timeout(u64),
    /// The reply could not be understood.
    #[error("unexpected reply: {0:?}")]
    BadReply(String),
    /// The profile does not describe a machine this driver can run.
    #[error("unsupported machine profile: {0}")]
    Unsupported(String),
    /// The machine has no such actuator.
    #[error("the machine has no actuator {0}")]
    NoActuator(String),
}

/// A connection to one roaster.
pub trait RoasterDriver: Send {
    /// Opens the connection and initialises the machine. Returns warnings
    /// (for example init steps that were not acknowledged).
    fn connect(&mut self) -> Result<Vec<String>, DriverError>;
    /// Requests and returns one reading.
    fn read(&mut self) -> Result<Reading, DriverError>;
    /// Sends one approved command.
    fn apply(&mut self, cmd: &ActuatorCommand) -> Result<(), DriverError>;
    /// A roast event was marked (the simulator charges and drops beans).
    fn on_event(&mut self, _kind: EventKind) {}
    /// Closes the connection.
    fn close(&mut self);
}
