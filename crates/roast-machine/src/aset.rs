//! Import of Artisan settings files (`.aset`) for TC4 devices into a draft
//! machine profile.
//!
//! The facts this importer relies on are documented in
//! `docs/research/artisan-and-cubean-plus.md`; no Artisan code is used
//! (ADR 0007). The result is a *draft*: every guess is reported as a warning,
//! and the draft must be reviewed by a person before it is used.
//!
//! Keys read:
//!
//! | Section | Keys | Used for |
//! |---|---|---|
//! | `Device` | `id` (19 = TC4), `arduinoETChannel`, `arduinoBTChannel`, `arduinoATChannel`, `ArduinoFILT` | device check, `CHAN;…` and `FILT;…` init |
//! | `General` | `Delay` (ms), `Mode` (C/F), `ETcurve`, `roastertype_setup` | sampling interval, units, ET presence, name |
//! | `SerialPort` | `comport`, `baudrate`, `bytesize`, `parity`, `stopbits`, `timeout` (s) | transport |
//! | `events` | `etypes` (default `Air, Drum, Damper, Burner`) | which event type is which actuator |
//! | `Sliders` | `slidercommands`, `slideractions` (1 = serial command), `slidermin`, `slidermax` | actuators; slider *i* drives event type *i* |
//! | `ExtraEventButtons` | `extraeventsactionstrings`, `extraeventsactions`, `extraeventstypes`, `extraeventslabels` | further actuators and on/off switches |
//! | `ExtraDev` | `extradevices`, `extraname1`, `extraname2` | reported actuators (device 32 = TC4 channels 5/6, names `{n}` = event type *n*) |

use std::collections::BTreeMap;

use roast_core::Actuator;

use crate::error::ImportError;
use crate::ini::QtIni;
use crate::profile::{
    ActuatorConfig, FieldRole, InitStep, Limits, LinkLossBehaviour, MachineCapabilities,
    MachineProfile, Parity, ProtocolConfig, ReadConfig, SafeValue, SerialConfig, SwitchConfig,
    Tc4Protocol, TransportConfig,
};

/// Artisan's device id of the TC4.
pub const TC4_DEVICE_ID: u32 = 19;
/// Artisan's extra device id that shows TC4 channels 5 and 6.
pub const TC4_CHANNELS_56_EXTRA_DEVICE: u32 = 32;
/// Artisan's event type names when a settings file does not list its own.
pub const DEFAULT_EVENT_TYPES: [&str; 4] = ["Air", "Drum", "Damper", "Burner"];

/// A draft profile and everything a person should review in it.
#[derive(Debug, Clone, PartialEq)]
pub struct ImportedProfile {
    /// The draft. It may not pass [`MachineProfile::validate`]; the reasons are
    /// among the warnings.
    pub profile: MachineProfile,
    /// Guesses, defaults and skipped settings, for a person to read.
    pub warnings: Vec<String>,
}

/// Builds a draft machine profile from the text of an Artisan `.aset` file
/// configured for a TC4 device.
///
/// # Errors
/// [`ImportError::NoDevice`] if the text has no `[Device] id`,
/// [`ImportError::UnsupportedDevice`] if the device is not a TC4.
pub fn import_aset(text: &str) -> Result<ImportedProfile, ImportError> {
    let ini = QtIni::parse(text);
    let device = ini
        .text("Device", "id")
        .ok_or(ImportError::NoDevice)?
        .trim();
    if device.parse::<u32>().ok() != Some(TC4_DEVICE_ID) {
        return Err(ImportError::UnsupportedDevice(device.to_string()));
    }
    let mut importer = Importer {
        ini: &ini,
        warnings: Vec::new(),
    };
    let profile = importer.build();
    Ok(ImportedProfile {
        profile,
        warnings: importer.warnings,
    })
}

struct Importer<'a> {
    ini: &'a QtIni,
    warnings: Vec<String>,
}

impl Importer<'_> {
    fn warn(&mut self, message: impl Into<String>) {
        self.warnings.push(message.into());
    }

    fn text(&self, section: &str, key: &str) -> Option<String> {
        self.ini
            .text(section, key)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
    }

    fn build(&mut self) -> MachineProfile {
        if !self.ini.skipped_lines().is_empty() {
            let lines = self.ini.skipped_lines().len();
            self.warn(format!(
                "{lines} unreadable line(s) in the settings file were skipped"
            ));
        }

        let name = self
            .text("General", "roastertype_setup")
            .unwrap_or_else(|| {
                self.warn("no machine name ([General] roastertype_setup); using a placeholder");
                "Imported TC4 roaster".to_string()
            });
        self.warn("vendor is not stored in .aset files; fill it in");

        let transport = self.transport();
        let sampling_interval_s = self.sampling_interval_s();
        let event_types = self.event_type_actuators();

        let mut actuators: Vec<ActuatorConfig> = Vec::new();
        self.slider_actuators(&event_types, &mut actuators);
        let switches = self.button_actuators(&event_types, &mut actuators);

        let (init, et_present) = self.init_sequence();
        let fields = self.read_fields(&event_types, &actuators);
        for cfg in &mut actuators {
            cfg.reported = fields.contains(&FieldRole::Actuator(cfg.actuator));
        }
        let reports_actuators = actuators.iter().any(|a| a.reported);

        let safe_state = self.safe_state(&actuators);

        self.warn(
            "limits are RoastMCP defaults (max_bt_c 240 °C, AI burner ≤ 5 points per 5 s, \
             AI air ≤ 10 points per 2 s); check max_bt_c against the manufacturer's manual",
        );
        self.warn(
            "automatic_drop is assumed false and link-loss behaviour is untested; run the \
             test protocol in docs/safety.md before AI control",
        );

        let mut profile = MachineProfile {
            id: slugify(&name),
            name: name.clone(),
            vendor: "unknown".into(),
            model: name,
            notes: "Draft imported from an Artisan settings file. Review every value before \
                    use; the safe state and limits are proposals."
                .into(),
            sampling_interval_s,
            transport: TransportConfig::Serial(transport),
            protocol: ProtocolConfig::Tc4(Tc4Protocol {
                line_ending: "\n".into(),
                init,
                read: ReadConfig {
                    command: "READ".into(),
                    fields,
                },
                et_present,
            }),
            actuators,
            switches,
            safe_state,
            limits: Limits {
                max_bt_c: 240.0,
                ai_burner_max_step: 10.0,
                ai_burner_min_interval_s: 10.0,
                ai_air_max_step: 20.0,
                ai_air_min_interval_s: 5.0,
            },
            capabilities: MachineCapabilities {
                automatic_drop: false,
                reports_actuators,
            },
            link_loss: LinkLossBehaviour {
                tested: false,
                link_lost: "not yet tested".into(),
                software_killed: "not yet tested".into(),
            },
        };
        profile.actuators.sort_by_key(|a| a.actuator);

        if let Err(errors) = profile.validate() {
            for issue in errors.issues {
                self.warn(format!("draft does not validate: {issue}"));
            }
        }
        profile
    }

    fn transport(&mut self) -> SerialConfig {
        let port_hint = self.text("SerialPort", "comport").unwrap_or_else(|| {
            self.warn("no serial port ([SerialPort] comport)");
            String::new()
        });
        let baud_rate = self.number("SerialPort", "baudrate", 115_200.0) as u32;
        let data_bits = self.number("SerialPort", "bytesize", 8.0) as u8;
        let parity = match self.text("SerialPort", "parity").as_deref() {
            Some("N") | None => Parity::None,
            Some("E") => Parity::Even,
            Some("O") => Parity::Odd,
            Some(other) => {
                self.warn(format!("parity `{other}` is not supported; using none"));
                Parity::None
            }
        };
        let stop_bits = match self.number("SerialPort", "stopbits", 1.0) {
            v if (v - 2.0).abs() < 1e-9 => 2,
            v if (v - 1.0).abs() < 1e-9 => 1,
            v => {
                self.warn(format!("{v} stop bits are not supported; using 1"));
                1
            }
        };
        let timeout_ms = (self.number("SerialPort", "timeout", 0.6) * 1000.0).round() as u64;
        SerialConfig {
            port_hint,
            baud_rate,
            data_bits,
            parity,
            stop_bits,
            timeout_ms,
        }
    }

    fn sampling_interval_s(&mut self) -> f64 {
        let delay_ms = self.number("General", "Delay", 1000.0);
        let s = delay_ms / 1000.0;
        if s > 1.0 {
            self.warn(format!(
                "Artisan sampled every {s} s; RoastMCP's TC4 default is 1 s, consider lowering it"
            ));
        }
        s
    }

    /// Reads a number, warning and using `default` when it is missing or bad.
    fn number(&mut self, section: &str, key: &str, default: f64) -> f64 {
        match self.text(section, key) {
            Some(raw) => raw.parse().unwrap_or_else(|_| {
                self.warn(format!(
                    "[{section}] {key} = `{raw}` is not a number; using {default}"
                ));
                default
            }),
            None => {
                self.warn(format!("[{section}] {key} is missing; using {default}"));
                default
            }
        }
    }

    /// Which actuator each Artisan event type controls.
    fn event_type_actuators(&mut self) -> Vec<Option<Actuator>> {
        let mut names = self.ini.list("events", "etypes");
        if names.len() < 4 {
            names = DEFAULT_EVENT_TYPES
                .iter()
                .map(|s| (*s).to_string())
                .collect();
        }
        names
            .iter()
            .take(4)
            .map(|n| actuator_from_name(n))
            .collect()
    }

    fn slider_actuators(
        &mut self,
        event_types: &[Option<Actuator>],
        out: &mut Vec<ActuatorConfig>,
    ) {
        let commands = self.ini.list("Sliders", "slidercommands");
        let actions = self.ini.list("Sliders", "slideractions");
        let mins = self.ini.list("Sliders", "slidermin");
        let maxs = self.ini.list("Sliders", "slidermax");

        for (i, command) in commands.iter().enumerate() {
            let command = command.trim();
            if command.is_empty() {
                continue;
            }
            if actions.get(i).map(|s| s.trim()) != Some("1") {
                self.warn(format!(
                    "slider {} (`{command}`) does not send a serial command; skipped",
                    i + 1
                ));
                continue;
            }
            let Some(actuator) = event_types.get(i).copied().flatten() else {
                self.warn(format!(
                    "slider {} sends `{command}` but its event type is not an actuator \
                     RoastMCP knows; skipped",
                    i + 1
                ));
                continue;
            };
            if !command.contains("{}") {
                self.warn(format!(
                    "slider {} sends `{command}` without a {{}} value placeholder; skipped",
                    i + 1
                ));
                continue;
            }
            let min = parse_or(mins.get(i), 0.0);
            let max = parse_or(maxs.get(i), 100.0);
            self.add_actuator(out, actuator, command, min, max, "slider");
        }
    }

    /// Adds actuators found on buttons and returns the switches found there.
    fn button_actuators(
        &mut self,
        event_types: &[Option<Actuator>],
        out: &mut Vec<ActuatorConfig>,
    ) -> Vec<SwitchConfig> {
        let section = "ExtraEventButtons";
        let commands = self.ini.list(section, "extraeventsactionstrings");
        let actions = self.ini.list(section, "extraeventsactions");
        let types = self.ini.list(section, "extraeventstypes");
        let labels = self.ini.list(section, "extraeventslabels");

        // Switch candidates by command prefix (`CLDN` for `CLDN,ON`).
        let mut candidates: BTreeMap<String, SwitchCandidate> = BTreeMap::new();

        for (i, command) in commands.iter().enumerate() {
            let command = command.trim();
            if command.is_empty() || actions.get(i).map(|s| s.trim()) != Some("1") {
                continue;
            }
            let label = labels
                .get(i)
                .map(|l| l.replace(['\n', '\t'], " ").trim().to_string())
                .unwrap_or_default();
            let shown = if label.is_empty() {
                format!("button {}", i + 1)
            } else {
                format!("button {} ({label})", i + 1)
            };
            if command.contains(';') || command.contains('(') {
                self.warn(format!(
                    "{shown} sends `{command}`, which is not a single TC4 command; skipped"
                ));
                continue;
            }

            if command.contains("{}") {
                let event_type = types.get(i).and_then(|t| t.trim().parse::<usize>().ok());
                let actuator = match event_type {
                    // 0..=3: sets event type n; 5..=8: steps event type n - 5.
                    Some(t @ 0..=3) => event_types.get(t).copied().flatten(),
                    Some(t @ 5..=8) => event_types.get(t - 5).copied().flatten(),
                    _ => actuator_from_name(&label),
                };
                match actuator {
                    Some(a) if a.is_switch() => self.warn(format!(
                        "{shown} sends `{command}` with a value to {a}, which RoastMCP \
                         treats as a switch; skipped"
                    )),
                    Some(a) => self.add_actuator(out, a, command, 0.0, 100.0, "button"),
                    None => self.warn(format!(
                        "{shown} sends `{command}` but no actuator could be determined; skipped"
                    )),
                }
                continue;
            }

            let Some((prefix, state)) = split_switch_command(command) else {
                self.warn(format!(
                    "{shown} sends `{command}`, which is not understood; skipped"
                ));
                continue;
            };
            let candidate = candidates.entry(prefix).or_default();
            match state {
                SwitchState::On => candidate.on = Some(command.to_string()),
                SwitchState::Off => candidate.off = Some(command.to_string()),
            }
            if candidate.actuator.is_none() {
                candidate.actuator = actuator_from_name(&label);
            }
        }

        let mut switches = Vec::new();
        for (prefix, candidate) in candidates {
            match candidate {
                SwitchCandidate {
                    on: Some(on),
                    off: Some(off),
                    actuator: Some(actuator),
                } if actuator.is_switch() => {
                    switches.push(SwitchConfig { actuator, on, off });
                }
                SwitchCandidate {
                    on: Some(_),
                    off: Some(_),
                    actuator,
                } => self.warn(format!(
                    "buttons switch `{prefix}` on and off, but {} is not a switch RoastMCP \
                     knows; skipped",
                    actuator.map_or_else(|| "the label".to_string(), |a| a.to_string())
                )),
                _ => self.warn(format!(
                    "`{prefix}` has a button for only one of on/off; skipped"
                )),
            }
        }
        if let Some(cooling) = switches.iter().find(|s| s.actuator == Actuator::Cooling) {
            if cooling.off.ends_with(",OF") {
                self.warn(format!(
                    "cooling is switched off with `{}`; some firmware expects `OFF`, verify on \
                     the machine",
                    cooling.off
                ));
            }
        }
        switches
    }

    fn add_actuator(
        &mut self,
        out: &mut Vec<ActuatorConfig>,
        actuator: Actuator,
        command: &str,
        min: f64,
        max: f64,
        source: &str,
    ) {
        if let Some(existing) = out.iter().find(|a| a.actuator == actuator) {
            if existing.command != command {
                self.warn(format!(
                    "{actuator}: {source} sends `{command}` but `{}` was found first; kept the first",
                    existing.command
                ));
            }
            return;
        }
        if source == "button" {
            self.warn(format!(
                "{actuator} (`{command}`) appears only on buttons; its range is assumed \
                 {min}..{max}"
            ));
        }
        out.push(ActuatorConfig {
            actuator,
            command: command.to_string(),
            min,
            max,
            step: 1.0,
            reported: false,
        });
    }

    /// `CHAN;…`, `UNITS;C` and `FILT;…`, and whether ET is present.
    fn init_sequence(&mut self) -> (Vec<InitStep>, bool) {
        let et = self.channel("arduinoETChannel");
        let mut bt = self.channel("arduinoBTChannel");
        if bt == 0 {
            self.warn("no BT channel ([Device] arduinoBTChannel); assuming channel 1");
            bt = 1;
        }
        if self.channel("arduinoATChannel") != 0 {
            self.warn("an ambient channel is configured ([Device] arduinoATChannel); not imported");
        }
        let ack = Some("#".to_string());
        let mut init = vec![InitStep {
            send: format!("CHAN;{et}{bt}00"),
            expect_prefix: ack.clone(),
        }];

        if let Some(mode) = self.text("General", "Mode") {
            if mode != "C" {
                self.warn(format!(
                    "Artisan used °{mode}; RoastMCP works in °C and requests UNITS;C"
                ));
            }
        }
        init.push(InitStep {
            send: "UNITS;C".into(),
            expect_prefix: ack.clone(),
        });

        let filters = self.ini.list("Device", "ArduinoFILT");
        if !filters.is_empty() {
            if filters.iter().all(|f| f.trim().parse::<u32>().is_ok()) {
                let joined: Vec<&str> = filters.iter().map(|f| f.trim()).collect();
                init.push(InitStep {
                    send: format!("FILT;{}", joined.join(",")),
                    expect_prefix: ack,
                });
            } else {
                self.warn(format!(
                    "[Device] ArduinoFILT = {filters:?} is not a list of numbers; no FILT step"
                ));
            }
        }

        let et_curve_shown = self.text("General", "ETcurve").as_deref() != Some("false");
        let et_present = et != 0 && et_curve_shown;
        if et != 0 && !et_curve_shown {
            self.warn(
                "Artisan hides the ET curve; assuming the machine has no working ET probe \
                 (set et_present = true if it has one)",
            );
        }
        (init, et_present)
    }

    /// A TC4 channel number 1..=4, or 0 for none.
    fn channel(&mut self, key: &str) -> u32 {
        match self.text("Device", key).as_deref() {
            None | Some("None") => 0,
            Some(raw) => match raw.parse::<u32>() {
                Ok(n @ 1..=4) => n,
                _ => {
                    self.warn(format!(
                        "[Device] {key} = `{raw}` is not a channel 1..4; ignored"
                    ));
                    0
                }
            },
        }
    }

    /// `ambient, et, bt`, then the reported actuators of TC4 channels 5/6,
    /// then the setpoint.
    fn read_fields(
        &mut self,
        event_types: &[Option<Actuator>],
        actuators: &[ActuatorConfig],
    ) -> Vec<FieldRole> {
        let mut fields = vec![FieldRole::Ambient, FieldRole::Et, FieldRole::Bt];
        let devices = self.ini.list("ExtraDev", "extradevices");
        let names1 = self.ini.list("ExtraDev", "extraname1");
        let names2 = self.ini.list("ExtraDev", "extraname2");

        let mut found = false;
        let mut others = Vec::new();
        for (i, device) in devices.iter().enumerate() {
            let device = device.trim();
            if device.parse::<u32>().ok() != Some(TC4_CHANNELS_56_EXTRA_DEVICE) {
                others.push(device.to_string());
                continue;
            }
            if found {
                continue;
            }
            found = true;
            for (channel, names) in [(5, &names1), (6, &names2)] {
                let name = names.get(i).map(|s| s.trim()).unwrap_or("");
                let actuator =
                    event_type_reference(name).and_then(|n| event_types.get(n).copied().flatten());
                match actuator {
                    Some(a) if actuators.iter().any(|c| c.actuator == a) => {
                        fields.push(FieldRole::Actuator(a));
                    }
                    _ => {
                        self.warn(format!(
                            "TC4 channel {channel} (named `{name}`) is not a known actuator; \
                             its READ field is ignored"
                        ));
                        fields.push(FieldRole::Ignore);
                    }
                }
            }
            fields.push(FieldRole::Setpoint);
        }
        if !found {
            self.warn(
                "no TC4 channel 5/6 extra device; assuming the machine does not report \
                 actuator values (fields after BT are ignored)",
            );
        }
        if !others.is_empty() {
            self.warn(format!(
                "extra devices {} are not imported",
                others.join(", ")
            ));
        }
        fields
    }

    fn safe_state(&mut self, actuators: &[ActuatorConfig]) -> BTreeMap<Actuator, SafeValue> {
        let mut safe = BTreeMap::new();
        let mut described = Vec::new();
        for cfg in actuators {
            let value = match cfg.actuator {
                Actuator::Burner => cfg.min,
                Actuator::Air | Actuator::Drum => cfg.max,
                _ => continue,
            };
            safe.insert(cfg.actuator, SafeValue::Value(value));
            described.push(format!("{} {value}", cfg.actuator));
        }
        self.warn(format!(
            "safe state proposed as {}; others keep their value — review it",
            described.join(", ")
        ));
        safe
    }
}

#[derive(Default)]
struct SwitchCandidate {
    on: Option<String>,
    off: Option<String>,
    actuator: Option<Actuator>,
}

enum SwitchState {
    On,
    Off,
}

/// Splits `CLDN,ON` into (`CLDN`, on).
fn split_switch_command(command: &str) -> Option<(String, SwitchState)> {
    let (prefix, state) = command.rsplit_once(',')?;
    let state = match state.trim().to_ascii_uppercase().as_str() {
        "ON" | "1" => SwitchState::On,
        "OF" | "OFF" | "0" => SwitchState::Off,
        _ => return None,
    };
    Some((prefix.trim().to_string(), state))
}

/// Parses `{3}` into 3.
fn event_type_reference(name: &str) -> Option<usize> {
    name.strip_prefix('{')?
        .strip_suffix('}')?
        .trim()
        .parse()
        .ok()
}

/// Guesses an actuator from an event type name or button label.
#[must_use]
pub fn actuator_from_name(name: &str) -> Option<Actuator> {
    let n = name.to_lowercase();
    if n.contains("filter") {
        Some(Actuator::Filter)
    } else if n.contains("cool") {
        Some(Actuator::Cooling)
    } else if ["burner", "heat", "power", "gas"]
        .iter()
        .any(|k| n.contains(k))
    {
        Some(Actuator::Burner)
    } else if n.contains("drum") {
        Some(Actuator::Drum)
    } else if n.contains("air") || n.contains("fan") {
        Some(Actuator::Air)
    } else {
        None
    }
}

fn parse_or(raw: Option<&String>, default: f64) -> f64 {
    raw.and_then(|s| s.trim().parse().ok()).unwrap_or(default)
}

/// Turns a machine name into a profile id: `Skywalker Series - Cubean +` →
/// `skywalker-series-cubean-plus`.
#[must_use]
pub fn slugify(name: &str) -> String {
    let mut out = String::new();
    for c in name.replace('+', " plus ").chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    let slug = out.trim_matches('-').to_string();
    if slug.is_empty() {
        "imported-tc4-roaster".into()
    } else {
        slug
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugs() {
        assert_eq!(
            slugify("Skywalker Series - Cubean +"),
            "skywalker-series-cubean-plus"
        );
        assert_eq!(slugify("  "), "imported-tc4-roaster");
        assert_eq!(slugify("Kaleido M2"), "kaleido-m2");
    }

    #[test]
    fn names_to_actuators() {
        assert_eq!(actuator_from_name("Burner"), Some(Actuator::Burner));
        assert_eq!(actuator_from_name("Heater"), Some(Actuator::Burner));
        assert_eq!(actuator_from_name("Air"), Some(Actuator::Air));
        assert_eq!(actuator_from_name("Filter fan"), Some(Actuator::Filter));
        assert_eq!(
            actuator_from_name("\\1 Beans Cooling"),
            Some(Actuator::Cooling)
        );
        assert_eq!(actuator_from_name("Damper"), None);
    }

    #[test]
    fn switch_commands() {
        assert!(
            matches!(split_switch_command("CLDN,ON"), Some((p, SwitchState::On)) if p == "CLDN")
        );
        assert!(matches!(
            split_switch_command("CLDN,OF"),
            Some((_, SwitchState::Off))
        ));
        assert!(matches!(
            split_switch_command("CLDN,OFF"),
            Some((_, SwitchState::Off))
        ));
        assert!(split_switch_command("PID;ON").is_none());
    }

    #[test]
    fn event_type_references() {
        assert_eq!(event_type_reference("{3}"), Some(3));
        assert_eq!(event_type_reference("SV"), None);
    }
}
