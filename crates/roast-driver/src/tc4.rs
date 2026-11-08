//! The generic TC4 driver: line-based ASCII, driven entirely by the machine
//! profile (init lines, READ reply layout, actuator command templates).

use std::time::Duration;

use roast_core::{ActuatorCommand, ActuatorValues};
use roast_machine::{FieldRole, MachineProfile, ProtocolConfig, Tc4Protocol, TransportConfig};

use crate::transport::{LineIo, SerialLineIo};
use crate::{DriverError, Reading, RoasterDriver};

/// Lines read while waiting for a reply before giving up on it.
const MAX_LINES_PER_REPLY: usize = 8;

type Opener<L> = Box<dyn FnMut() -> std::io::Result<L> + Send>;

/// A TC4-family machine on a line transport.
pub struct Tc4Driver<L: LineIo> {
    profile: MachineProfile,
    proto: Tc4Protocol,
    timeout: Duration,
    open: Opener<L>,
    io: Option<L>,
}

impl Tc4Driver<SerialLineIo> {
    /// A driver for the profile's serial machine on `port`.
    pub fn serial(profile: MachineProfile, port: String) -> Result<Self, DriverError> {
        let cfg = match &profile.transport {
            TransportConfig::Serial(s) => s.clone(),
        };
        Self::with_opener(profile, Box::new(move || SerialLineIo::open(&port, &cfg)))
    }
}

impl<L: LineIo> Tc4Driver<L> {
    /// A driver that opens its transport with `open` (tests pass a fake).
    pub fn with_opener(profile: MachineProfile, open: Opener<L>) -> Result<Self, DriverError> {
        let proto = match &profile.protocol {
            ProtocolConfig::Tc4(p) => p.clone(),
        };
        let timeout = match &profile.transport {
            TransportConfig::Serial(s) => Duration::from_millis(s.timeout_ms),
        };
        Ok(Tc4Driver {
            profile,
            proto,
            timeout,
            open,
            io: None,
        })
    }

    fn io(&mut self) -> Result<&mut L, DriverError> {
        self.io.as_mut().ok_or_else(|| {
            DriverError::Io(std::io::Error::new(
                std::io::ErrorKind::NotConnected,
                "not connected",
            ))
        })
    }

    fn send(&mut self, line: &str) -> Result<(), DriverError> {
        let ending = self.proto.line_ending.clone();
        let io = self.io()?;
        io.write_line(line, &ending).map_err(DriverError::Io)
    }
}

impl<L: LineIo> RoasterDriver for Tc4Driver<L> {
    fn connect(&mut self) -> Result<Vec<String>, DriverError> {
        self.io = Some((self.open)()?);
        let mut warnings = Vec::new();
        for step in self.proto.init.clone() {
            self.send(&step.send)?;
            if let Some(prefix) = &step.expect_prefix {
                let timeout = self.timeout;
                let io = self.io()?;
                let mut acknowledged = false;
                for _ in 0..MAX_LINES_PER_REPLY {
                    match io.read_line(timeout)? {
                        Some(line) if line.trim_start().starts_with(prefix.as_str()) => {
                            acknowledged = true;
                            break;
                        }
                        Some(_) => {}
                        None => break,
                    }
                }
                if !acknowledged {
                    warnings.push(format!("no acknowledgement for {:?}", step.send));
                }
            }
        }
        Ok(warnings)
    }

    fn read(&mut self) -> Result<Reading, DriverError> {
        let command = self.proto.read.command.clone();
        let timeout = self.timeout;
        {
            let io = self.io()?;
            io.clear_input()?;
        }
        self.send(&command)?;
        let fields = self.proto.read.fields.clone();
        let et_present = self.proto.et_present;
        let io = self.io()?;
        let mut last = String::new();
        for _ in 0..MAX_LINES_PER_REPLY {
            match io.read_line(timeout)? {
                Some(line) => {
                    if let Some(mut r) = parse_read_reply(&line, &fields) {
                        if !et_present {
                            r.et = None;
                        }
                        return Ok(r);
                    }
                    last = line;
                }
                None => break,
            }
        }
        if last.is_empty() {
            Err(DriverError::Timeout(
                u64::try_from(timeout.as_millis()).unwrap_or(u64::MAX),
            ))
        } else {
            Err(DriverError::BadReply(last))
        }
    }

    fn apply(&mut self, cmd: &ActuatorCommand) -> Result<(), DriverError> {
        let line = if let Some(a) = self.profile.actuator(cmd.actuator) {
            let v = cmd.value.round().clamp(a.min, a.max);
            // Bounded to the actuator range above, so the cast is exact.
            #[allow(clippy::cast_possible_truncation)]
            a.render(v as i64)
        } else if let Some(s) = self.profile.switch(cmd.actuator) {
            if cmd.value > 0.0 {
                s.on.clone()
            } else {
                s.off.clone()
            }
        } else {
            return Err(DriverError::NoActuator(cmd.actuator.to_string()));
        };
        self.send(&line)
    }

    fn close(&mut self) {
        self.io = None;
    }
}

/// Parses a READ reply such as `21.4,0.0,187.3,60,40,0` by the field roles.
/// Returns `None` for lines that are not a numeric reply (acknowledgements
/// starting with `#`, echoes, garbage) or that lack a bean temperature field.
#[must_use]
pub fn parse_read_reply(line: &str, fields: &[FieldRole]) -> Option<Reading> {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') {
        return None;
    }
    let parts: Vec<&str> = line.split(',').map(str::trim).collect();
    let bt_index = fields.iter().position(|f| *f == FieldRole::Bt)?;
    if parts.len() <= bt_index {
        return None;
    }
    let mut values = Vec::with_capacity(parts.len());
    for p in &parts {
        if p.is_empty() {
            values.push(None);
        } else {
            let v: f64 = p.parse().ok()?;
            values.push((v.is_finite() && v != -1.0).then_some(v));
        }
    }
    let mut r = Reading {
        actuators: ActuatorValues::new(),
        ..Reading::default()
    };
    for (role, v) in fields.iter().zip(values) {
        match role {
            FieldRole::Ambient => r.ambient_c = v,
            FieldRole::Et => r.et = v,
            FieldRole::Bt => r.bt = v,
            FieldRole::Actuator(a) => {
                if let Some(v) = v {
                    r.actuators.insert(*a, v);
                }
            }
            FieldRole::Setpoint | FieldRole::Ignore => {}
        }
    }
    // A bean temperature of exactly 0 means "no probe" on TC4 boards.
    if r.bt == Some(0.0) {
        r.bt = None;
    }
    Some(r)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::ScriptedIo;
    use roast_core::Actuator;

    fn cubean() -> MachineProfile {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../machines/skywalker-cubean-plus.toml"
        );
        MachineProfile::load(path).unwrap()
    }

    fn driver(script: ScriptedIo) -> Tc4Driver<ScriptedIo> {
        let mut script = Some(script);
        Tc4Driver::with_opener(
            cubean(),
            Box::new(move || Ok(script.take().unwrap_or_default())),
        )
        .unwrap()
    }

    #[test]
    fn parses_the_cubean_reply() {
        let p = cubean();
        let ProtocolConfig::Tc4(proto) = &p.protocol;
        let r = parse_read_reply("21.4,0.0,187.3,60,40,0", &proto.read.fields).unwrap();
        assert_eq!(r.ambient_c, Some(21.4));
        assert_eq!(r.bt, Some(187.3));
        assert_eq!(r.actuators.get(&Actuator::Burner), Some(&60.0));
        assert_eq!(r.actuators.get(&Actuator::Air), Some(&40.0));
        assert!(parse_read_reply("#OK", &proto.read.fields).is_none());
        assert!(parse_read_reply("21.4,abc", &proto.read.fields).is_none());
        assert!(parse_read_reply("21.4,0.0", &proto.read.fields).is_none());
    }

    #[test]
    fn connect_sends_init_and_reads() {
        // The Cubean+ never acknowledges setup commands, so none is waited for.
        let script = ScriptedIo::default().reply("READ", "22.0,0.0,150.5,55,30,0");
        let mut d = driver(script);
        let warnings = d.connect().unwrap();
        assert!(warnings.is_empty(), "{warnings:?}");
        let r = d.read().unwrap();
        assert_eq!(r.bt, Some(150.5));
        assert_eq!(r.et, None, "the Cubean+ has no ET probe");
        d.apply(&ActuatorCommand {
            actuator: Actuator::Burner,
            value: 62.4,
        })
        .unwrap();
        d.apply(&ActuatorCommand {
            actuator: Actuator::Drum,
            value: 10.0,
        })
        .unwrap();
        d.apply(&ActuatorCommand {
            actuator: Actuator::Cooling,
            value: 0.0,
        })
        .unwrap();
        let written = &d.io.as_ref().unwrap().written;
        assert_eq!(
            written,
            &[
                "CHAN;2100",
                "UNITS;C",
                "FILT;95,95,95,95",
                "READ",
                "OT1,62",
                "OT3,80",
                "CLDN,OF"
            ]
        );
    }

    #[test]
    fn read_times_out_without_reply() {
        let mut d = driver(ScriptedIo::default());
        d.connect().unwrap();
        assert!(matches!(d.read(), Err(DriverError::Timeout(600))));
    }

    #[test]
    fn warns_when_an_expected_acknowledgement_is_missing() {
        let mut profile = cubean();
        let ProtocolConfig::Tc4(proto) = &mut profile.protocol;
        for step in &mut proto.init {
            step.expect_prefix = Some("#".into());
        }
        let script = ScriptedIo::default()
            .reply("CHAN", "#OK")
            .reply("UNITS", "#OK");
        let mut script = Some(script);
        let mut d = Tc4Driver::with_opener(
            profile,
            Box::new(move || Ok(script.take().unwrap_or_default())),
        )
        .unwrap();
        assert_eq!(
            d.connect().unwrap(),
            vec!["no acknowledgement for \"FILT;95,95,95,95\""]
        );
    }
}
