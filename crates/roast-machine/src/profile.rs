//! The machine profile schema (ADR 0006) and loading from TOML.

use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use roast_core::Actuator;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::error::ProfileError;

/// A complete description of one roaster model: how to reach it, how to talk
/// to it, what it can do and what is safe for it.
///
/// Profiles live in `machines/<id>.toml`. Use [`MachineProfile::from_toml_str`]
/// or [`MachineProfile::load`] to get a validated profile;
/// [`MachineProfile::parse_toml`] skips validation (for drafts).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MachineProfile {
    /// Stable identifier, lowercase kebab-case, equal to the file name without
    /// `.toml` (for example `skywalker-cubean-plus`).
    pub id: String,
    /// Human-readable machine name.
    pub name: String,
    /// Manufacturer.
    pub vendor: String,
    /// Model name as the manufacturer writes it.
    pub model: String,
    /// Free-text notes for people (quirks, sources, open questions).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub notes: String,
    /// Seconds between two samples.
    #[serde(serialize_with = "whole_as_int")]
    pub sampling_interval_s: f64,
    /// How the computer reaches the machine.
    pub transport: TransportConfig,
    /// The wire protocol.
    pub protocol: ProtocolConfig,
    /// Continuous actuators (percent values).
    #[serde(default)]
    pub actuators: Vec<ActuatorConfig>,
    /// On/off actuators.
    #[serde(default)]
    pub switches: Vec<SwitchConfig>,
    /// What the safety layer sets on an emergency stop or a safety reaction.
    /// Actuators not listed keep their value.
    pub safe_state: BTreeMap<Actuator, SafeValue>,
    /// Safety limits.
    pub limits: Limits,
    /// What the machine can do beyond its actuators.
    pub capabilities: MachineCapabilities,
    /// The tested behaviour when the link to the computer breaks (see
    /// `docs/safety.md`).
    pub link_loss: LinkLossBehaviour,
}

impl MachineProfile {
    /// Parses a profile from TOML without validating it. Use this for drafts
    /// that a person still has to complete.
    ///
    /// # Errors
    /// [`ProfileError::Toml`] if the text is not a well-formed profile.
    pub fn parse_toml(text: &str) -> Result<Self, ProfileError> {
        toml::from_str(text).map_err(|source| ProfileError::Toml {
            origin: "profile".into(),
            source,
        })
    }

    /// Parses and validates a profile from TOML.
    ///
    /// # Errors
    /// [`ProfileError::Toml`] for malformed TOML or unknown fields,
    /// [`ProfileError::Invalid`] if the profile breaks a rule.
    pub fn from_toml_str(text: &str) -> Result<Self, ProfileError> {
        let profile = Self::parse_toml(text)?;
        profile.validate().map_err(|source| ProfileError::Invalid {
            origin: format!("profile `{}`", profile.id),
            source,
        })?;
        Ok(profile)
    }

    /// Reads, parses and validates a profile file.
    ///
    /// # Errors
    /// [`ProfileError::Io`] if the file cannot be read, otherwise as
    /// [`MachineProfile::from_toml_str`].
    pub fn load(path: impl AsRef<Path>) -> Result<Self, ProfileError> {
        let path = path.as_ref();
        let text = std::fs::read_to_string(path).map_err(|source| ProfileError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        let profile: Self = toml::from_str(&text).map_err(|source| ProfileError::Toml {
            origin: path.display().to_string(),
            source,
        })?;
        profile.validate().map_err(|source| ProfileError::Invalid {
            origin: path.display().to_string(),
            source,
        })?;
        Ok(profile)
    }

    /// Loads every `*.toml` file of a directory (not recursive), sorted by id.
    ///
    /// Each file name must equal its profile id, which also makes ids unique.
    ///
    /// # Errors
    /// The first file that fails to load, or [`ProfileError::IdMismatch`].
    pub fn load_dir(dir: impl AsRef<Path>) -> Result<Vec<Self>, ProfileError> {
        let dir = dir.as_ref();
        let io_err = |source| ProfileError::Io {
            path: dir.to_path_buf(),
            source,
        };
        let mut paths: Vec<PathBuf> = Vec::new();
        for entry in std::fs::read_dir(dir).map_err(io_err)? {
            let path = entry.map_err(io_err)?.path();
            if path.is_file() && path.extension().is_some_and(|e| e == "toml") {
                paths.push(path);
            }
        }
        paths.sort();

        let mut profiles = Vec::with_capacity(paths.len());
        for path in paths {
            let profile = Self::load(&path)?;
            let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
            if stem != profile.id {
                return Err(ProfileError::IdMismatch {
                    path,
                    id: profile.id,
                });
            }
            profiles.push(profile);
        }
        profiles.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(profiles)
    }

    /// Serializes the profile to TOML (without comments).
    ///
    /// # Errors
    /// [`ProfileError::Serialize`] if a value cannot be represented in TOML.
    pub fn to_toml_string(&self) -> Result<String, ProfileError> {
        Ok(toml::to_string_pretty(self)?)
    }

    /// The serial settings, if the machine is reached over a serial port.
    #[must_use]
    pub fn serial(&self) -> Option<&SerialConfig> {
        match &self.transport {
            TransportConfig::Serial(s) => Some(s),
        }
    }

    /// The TC4 protocol settings, if the machine speaks TC4.
    #[must_use]
    pub fn tc4(&self) -> Option<&Tc4Protocol> {
        match &self.protocol {
            ProtocolConfig::Tc4(p) => Some(p),
        }
    }

    /// The configuration of a continuous actuator.
    #[must_use]
    pub fn actuator(&self, actuator: Actuator) -> Option<&ActuatorConfig> {
        self.actuators.iter().find(|a| a.actuator == actuator)
    }

    /// The configuration of a switch.
    #[must_use]
    pub fn switch(&self, actuator: Actuator) -> Option<&SwitchConfig> {
        self.switches.iter().find(|s| s.actuator == actuator)
    }

    /// Whether the machine has this actuator, as a continuous value or a switch.
    #[must_use]
    pub fn has_actuator(&self, actuator: Actuator) -> bool {
        self.actuator(actuator).is_some() || self.switch(actuator).is_some()
    }
}

/// How the computer reaches the machine. `kind` selects the variant.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TransportConfig {
    /// A serial port. Bluetooth Classic SPP devices appear as serial ports
    /// (`/dev/cu.<name>` on macOS, `COMx` on Windows, `/dev/rfcommX` on Linux).
    Serial(SerialConfig),
}

/// Serial port settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SerialConfig {
    /// The usual device path; the user can choose another port at connect time.
    pub port_hint: String,
    /// Baud rate, for example `115200`.
    pub baud_rate: u32,
    /// Data bits per character, 5 to 8.
    pub data_bits: u8,
    /// Parity bit.
    pub parity: Parity,
    /// Stop bits, 1 or 2.
    pub stop_bits: u8,
    /// How long to wait for a reply line, milliseconds.
    pub timeout_ms: u64,
}

/// Serial parity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Parity {
    /// No parity bit (the `N` in 8N1).
    None,
    /// Even parity.
    Even,
    /// Odd parity.
    Odd,
}

/// The wire protocol. `kind` selects the variant.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ProtocolConfig {
    /// The line-based ASCII protocol of the TC4 family (`READ`, `OT1,50` …).
    Tc4(Tc4Protocol),
}

/// Settings of the TC4 protocol.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Tc4Protocol {
    /// Appended to every line sent: `"\n"`, `"\r\n"` or `"\r"`.
    pub line_ending: String,
    /// Sent once after the port is opened, in order.
    #[serde(default)]
    pub init: Vec<InitStep>,
    /// How a sample is requested and laid out.
    pub read: ReadConfig,
    /// Whether the machine has a working environment-temperature probe. When
    /// `false`, an `et` field in the reply is ignored.
    pub et_present: bool,
}

/// One step of the initialisation sequence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InitStep {
    /// The line to send.
    pub send: String,
    /// The reply must start with this prefix (for example `"#"`). Lines that do
    /// not match are discarded. Without a prefix, no reply is awaited.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expect_prefix: Option<String>,
}

/// How to request a sample and what the comma-separated reply contains.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReadConfig {
    /// The request line, usually `READ`.
    pub command: String,
    /// The role of each reply field, in order. Further fields are ignored.
    pub fields: Vec<FieldRole>,
}

/// What a field of the `READ` reply means.
///
/// Written in TOML as `"ambient"`, `"et"`, `"bt"`, `"setpoint"`, `"ignore"` or
/// `"actuator:<name>"` (for example `"actuator:burner"`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub enum FieldRole {
    /// Ambient (board) temperature, °C.
    Ambient,
    /// Environment temperature, °C.
    Et,
    /// Bean temperature, °C.
    Bt,
    /// The value of an actuator as the machine reports it, percent.
    Actuator(Actuator),
    /// Controller setpoint.
    Setpoint,
    /// Present but unused.
    Ignore,
}

impl fmt::Display for FieldRole {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FieldRole::Ambient => f.write_str("ambient"),
            FieldRole::Et => f.write_str("et"),
            FieldRole::Bt => f.write_str("bt"),
            FieldRole::Actuator(a) => write!(f, "actuator:{a}"),
            FieldRole::Setpoint => f.write_str("setpoint"),
            FieldRole::Ignore => f.write_str("ignore"),
        }
    }
}

impl FromStr for FieldRole {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "ambient" => Ok(FieldRole::Ambient),
            "et" => Ok(FieldRole::Et),
            "bt" => Ok(FieldRole::Bt),
            "setpoint" => Ok(FieldRole::Setpoint),
            "ignore" => Ok(FieldRole::Ignore),
            other => match other.strip_prefix("actuator:") {
                Some(name) => parse_actuator(name).map(FieldRole::Actuator).ok_or_else(|| {
                    format!(
                        "unknown actuator `{name}` in field role `{other}` (expected one of {})",
                        actuator_names()
                    )
                }),
                None => Err(format!(
                    "unknown field role `{other}` (expected ambient, et, bt, setpoint, ignore \
                     or actuator:<name>)"
                )),
            },
        }
    }
}

impl TryFrom<String> for FieldRole {
    type Error = String;

    fn try_from(s: String) -> Result<Self, Self::Error> {
        s.parse()
    }
}

impl From<FieldRole> for String {
    fn from(role: FieldRole) -> Self {
        role.to_string()
    }
}

/// Parses an actuator name (`burner`, `air`, `drum`, `cooling`, `filter`).
#[must_use]
pub fn parse_actuator(name: &str) -> Option<Actuator> {
    Actuator::ALL.into_iter().find(|a| a.as_str() == name)
}

fn actuator_names() -> String {
    Actuator::ALL
        .iter()
        .map(|a| a.as_str())
        .collect::<Vec<_>>()
        .join(", ")
}

/// A continuous actuator driven by a command template.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActuatorConfig {
    /// Which actuator.
    pub actuator: Actuator,
    /// Command template; `{}` is replaced by the integer percent value, for
    /// example `"OT1,{}"`.
    pub command: String,
    /// Lowest accepted value, percent.
    #[serde(serialize_with = "whole_as_int")]
    pub min: f64,
    /// Highest accepted value, percent.
    #[serde(serialize_with = "whole_as_int")]
    pub max: f64,
    /// Granularity offered to people and controllers, percent points. The
    /// driver sends any integer within the range.
    #[serde(serialize_with = "whole_as_int")]
    pub step: f64,
    /// Whether the machine reports the actual value in the `READ` reply (then
    /// a matching `actuator:<name>` field must exist).
    #[serde(default)]
    pub reported: bool,
    /// How people read the value, when not in percent (the drum in rpm).
    /// Commands and recordings stay in percent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display: Option<DisplayScale>,
}

impl ActuatorConfig {
    /// Renders the command for an integer value.
    #[must_use]
    pub fn render(&self, value: i64) -> String {
        self.command.replacen("{}", &value.to_string(), 1)
    }
}

/// A linear mapping from the percent value to the unit people use:
/// `shown = percent * scale + offset`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DisplayScale {
    /// Unit shown to people, for example `"rpm"`.
    pub unit: String,
    /// Display units per percent point; positive.
    pub scale: f64,
    /// Display value at 0 %.
    #[serde(default)]
    pub offset: f64,
}

impl DisplayScale {
    /// The display value for a percent value.
    #[must_use]
    pub fn to_display(&self, percent: f64) -> f64 {
        percent * self.scale + self.offset
    }

    /// The percent value for a display value.
    #[must_use]
    pub fn to_percent(&self, shown: f64) -> f64 {
        (shown - self.offset) / self.scale
    }
}

/// An on/off actuator driven by two fixed commands.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SwitchConfig {
    /// Which actuator (a switch kind, for example `cooling`).
    pub actuator: Actuator,
    /// The line that switches it on.
    pub on: String,
    /// The line that switches it off.
    pub off: String,
}

/// The value an actuator takes in the safe state.
///
/// Written in TOML as a number (percent), `"on"`, `"off"` or `"keep"`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SafeValue {
    /// Set a continuous actuator to this percent value.
    Value(f64),
    /// Switch on.
    On,
    /// Switch off.
    Off,
    /// Leave as it is.
    Keep,
}

impl fmt::Display for SafeValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SafeValue::Value(v) => write!(f, "{v}"),
            SafeValue::On => f.write_str("on"),
            SafeValue::Off => f.write_str("off"),
            SafeValue::Keep => f.write_str("keep"),
        }
    }
}

impl Serialize for SafeValue {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            SafeValue::Value(v) => whole_as_int(v, s),
            SafeValue::On => s.serialize_str("on"),
            SafeValue::Off => s.serialize_str("off"),
            SafeValue::Keep => s.serialize_str("keep"),
        }
    }
}

impl<'de> Deserialize<'de> for SafeValue {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct Visitor;

        impl serde::de::Visitor<'_> for Visitor {
            type Value = SafeValue;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(r#"a percent value, "on", "off" or "keep""#)
            }

            fn visit_i64<E: serde::de::Error>(self, v: i64) -> Result<SafeValue, E> {
                Ok(SafeValue::Value(v as f64))
            }

            fn visit_u64<E: serde::de::Error>(self, v: u64) -> Result<SafeValue, E> {
                Ok(SafeValue::Value(v as f64))
            }

            fn visit_f64<E: serde::de::Error>(self, v: f64) -> Result<SafeValue, E> {
                Ok(SafeValue::Value(v))
            }

            fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<SafeValue, E> {
                match v {
                    "on" => Ok(SafeValue::On),
                    "off" => Ok(SafeValue::Off),
                    "keep" => Ok(SafeValue::Keep),
                    other => Err(E::invalid_value(serde::de::Unexpected::Str(other), &self)),
                }
            }
        }

        d.deserialize_any(Visitor)
    }
}

/// Safety limits of the machine.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Limits {
    /// Bean temperature at which the safety layer cuts the burner, °C.
    #[serde(serialize_with = "whole_as_int")]
    pub max_bt_c: f64,
    /// Largest burner change per AI/autopilot command, percent points.
    #[serde(serialize_with = "whole_as_int")]
    pub ai_burner_max_step: f64,
    /// Shortest time between two AI/autopilot burner changes, seconds.
    #[serde(serialize_with = "whole_as_int")]
    pub ai_burner_min_interval_s: f64,
    /// Largest air change per AI/autopilot command, percent points.
    #[serde(serialize_with = "whole_as_int")]
    pub ai_air_max_step: f64,
    /// Shortest time between two AI/autopilot air changes, seconds.
    #[serde(serialize_with = "whole_as_int")]
    pub ai_air_min_interval_s: f64,
}

/// What the machine can do beyond its actuators.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MachineCapabilities {
    /// Whether the software can drop the beans (otherwise a person does it and
    /// the app shows a "drop now" cue).
    pub automatic_drop: bool,
    /// Whether the machine reports actual actuator values in its samples.
    pub reports_actuators: bool,
}

/// The tested behaviour of the machine when the link breaks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LinkLossBehaviour {
    /// Whether the test protocol in `docs/safety.md` was run on real hardware.
    pub tested: bool,
    /// What the machine does when the link is lost (burner, fan, drum, display).
    pub link_lost: String,
    /// What the machine does when the software is killed but the link stays up.
    pub software_killed: String,
}

/// Serializes whole numbers as TOML integers (`100` instead of `100.0`).
fn whole_as_int<S: Serializer>(v: &f64, s: S) -> Result<S::Ok, S::Error> {
    let whole = v.is_finite() && v.fract() == 0.0 && v.abs() < 1e15;
    if whole {
        s.serialize_i64(*v as i64)
    } else {
        s.serialize_f64(*v)
    }
}
