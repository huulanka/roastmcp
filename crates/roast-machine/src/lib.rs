//! Machine profiles and the Artisan `.aset` importer.
//!
//! A [`MachineProfile`] describes a roaster completely — transport,
//! protocol, actuators, safe state, limits and tested link-loss behaviour — so
//! that generic drivers can run it without machine-specific code
//! ([ADR 0006](../../../docs/adr/0006-machine-profiles.md)). Profiles are TOML
//! files in `machines/`; `machines/skywalker-cubean-plus.toml` is the commented
//! template.
//!
//! ```no_run
//! use roast_machine::MachineProfile;
//!
//! let profiles = MachineProfile::load_dir("machines")?;
//! for p in &profiles {
//!     println!("{}: {}", p.id, p.name);
//! }
//! # Ok::<(), roast_machine::ProfileError>(())
//! ```
//!
//! [`import_aset`] turns an Artisan settings file for a TC4 device into a draft
//! profile with a list of warnings for a person to review.

pub mod aset;
pub mod error;
pub mod ini;
pub mod profile;
pub mod validate;

pub use aset::{ImportedProfile, import_aset};
pub use error::{ImportError, ProfileError, ValidationErrors, ValidationIssue};
pub use ini::{IniValue, QtIni};
pub use profile::{
    ActuatorConfig, FieldRole, InitStep, LinkLossBehaviour, Limits, MachineCapabilities,
    MachineProfile, Parity, ProtocolConfig, ReadConfig, SafeValue, SerialConfig, SwitchConfig,
    Tc4Protocol, TransportConfig, parse_actuator,
};
