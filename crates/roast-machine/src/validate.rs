//! Validation of machine profiles.
//!
//! Parsing only checks the shape of a profile; these rules check that it makes
//! sense and, above all, that its safe state is safe.

use std::collections::BTreeSet;

use roast_core::Actuator;

use crate::error::{ValidationErrors, ValidationIssue};
use crate::profile::{
    FieldRole, MachineProfile, ProtocolConfig, SafeValue, SerialConfig, Tc4Protocol,
    TransportConfig,
};

/// Bounds of the sampling interval, seconds.
pub const SAMPLING_INTERVAL_RANGE_S: (f64, f64) = (0.2, 10.0);
/// Bounds of the bean temperature limit, °C.
pub const MAX_BT_RANGE_C: (f64, f64) = (100.0, 300.0);
/// Largest burner step an AI may ever be allowed per command, percent points.
pub const AI_BURNER_STEP_CEILING: f64 = 20.0;
/// Largest air step an AI may ever be allowed per command, percent points.
pub const AI_AIR_STEP_CEILING: f64 = 50.0;

impl MachineProfile {
    /// Checks every rule and reports all broken ones together.
    ///
    /// The rules include: sane ranges for every number; exactly one `bt` field
    /// in the `READ` layout; reported actuators match `actuator:` fields; every
    /// actuator in the safe state exists; the burner's safe value is its
    /// minimum; air stays on; the drum is never turned off.
    ///
    /// # Errors
    /// [`ValidationErrors`] listing every broken rule.
    pub fn validate(&self) -> Result<(), ValidationErrors> {
        let mut c = Checker::default();
        self.check_identity(&mut c);
        match &self.transport {
            TransportConfig::Serial(serial) => check_serial(&mut c, serial, self),
        }
        match &self.protocol {
            ProtocolConfig::Tc4(tc4) => self.check_tc4(&mut c, tc4),
        }
        self.check_actuators(&mut c);
        self.check_switches(&mut c);
        self.check_safe_state(&mut c);
        self.check_limits(&mut c);
        self.check_capabilities(&mut c);
        if c.issues.is_empty() {
            Ok(())
        } else {
            Err(ValidationErrors { issues: c.issues })
        }
    }

    fn check_identity(&self, c: &mut Checker) {
        let id_ok = !self.id.is_empty()
            && self
                .id
                .chars()
                .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-')
            && !self.id.starts_with('-')
            && !self.id.ends_with('-');
        if !id_ok {
            c.push(
                "id",
                format!(
                    "`{}` is not a valid id; use lowercase letters, digits and dashes, \
                     for example `skywalker-cubean-plus`",
                    self.id
                ),
            );
        }
        for (field, value) in [
            ("name", &self.name),
            ("vendor", &self.vendor),
            ("model", &self.model),
        ] {
            if value.trim().is_empty() {
                c.push(field, "must not be empty");
            }
        }
        let (lo, hi) = SAMPLING_INTERVAL_RANGE_S;
        if !in_range(self.sampling_interval_s, lo, hi) {
            c.push(
                "sampling_interval_s",
                format!(
                    "{} s is outside {lo}..={hi} s",
                    self.sampling_interval_s
                ),
            );
        }
        if self.link_loss.link_lost.trim().is_empty() {
            c.push(
                "link_loss.link_lost",
                "describe the behaviour, or write \"not yet tested\"",
            );
        }
        if self.link_loss.software_killed.trim().is_empty() {
            c.push(
                "link_loss.software_killed",
                "describe the behaviour, or write \"not yet tested\"",
            );
        }
    }

    fn check_tc4(&self, c: &mut Checker, tc4: &Tc4Protocol) {
        if !matches!(tc4.line_ending.as_str(), "\n" | "\r\n" | "\r") {
            c.push(
                "protocol.line_ending",
                format!(
                    "{:?} is not a line ending; use \"\\n\", \"\\r\\n\" or \"\\r\"",
                    tc4.line_ending
                ),
            );
        }
        for (i, step) in tc4.init.iter().enumerate() {
            check_line(c, &format!("protocol.init[{i}].send"), &step.send);
            if step.expect_prefix.as_deref() == Some("") {
                c.push(
                    format!("protocol.init[{i}].expect_prefix"),
                    "must not be empty; leave it out to not wait for a reply",
                );
            }
        }
        check_line(c, "protocol.read.command", &tc4.read.command);

        let fields = &tc4.read.fields;
        let bt_count = fields.iter().filter(|f| **f == FieldRole::Bt).count();
        if bt_count != 1 {
            c.push(
                "protocol.read.fields",
                format!("needs exactly one \"bt\" field, found {bt_count}"),
            );
        }
        let mut seen = BTreeSet::new();
        for role in fields {
            if *role != FieldRole::Ignore && *role != FieldRole::Bt && !seen.insert(role.to_string())
            {
                c.push(
                    "protocol.read.fields",
                    format!("\"{role}\" appears more than once"),
                );
            }
        }
        if tc4.et_present && !fields.contains(&FieldRole::Et) {
            c.push(
                "protocol.et_present",
                "is true but protocol.read.fields has no \"et\" field",
            );
        }
        for role in fields {
            if let FieldRole::Actuator(a) = role {
                match self.actuator(*a) {
                    Some(cfg) if cfg.reported => {}
                    Some(_) => c.push(
                        "protocol.read.fields",
                        format!(
                            "\"{role}\" is read, but the {a} actuator is not marked reported = true"
                        ),
                    ),
                    None => c.push(
                        "protocol.read.fields",
                        format!("\"{role}\" refers to an actuator missing from [[actuators]]"),
                    ),
                }
            }
        }
        for cfg in &self.actuators {
            if cfg.reported && !fields.contains(&FieldRole::Actuator(cfg.actuator)) {
                c.push(
                    format!("actuators.{}.reported", cfg.actuator),
                    format!(
                        "is true but protocol.read.fields has no \"actuator:{}\" field",
                        cfg.actuator
                    ),
                );
            }
        }
    }

    fn check_actuators(&self, c: &mut Checker) {
        let mut seen = BTreeSet::new();
        for cfg in &self.actuators {
            let a = cfg.actuator;
            let field = format!("actuators.{a}");
            if !seen.insert(a) {
                c.push(&field, "is declared more than once");
            }
            if a.is_switch() {
                c.push(
                    &field,
                    format!("{a} is an on/off actuator; declare it under [[switches]]"),
                );
            }
            check_line(c, &format!("{field}.command"), &cfg.command);
            let placeholders = cfg.command.matches("{}").count();
            if placeholders != 1 {
                c.push(
                    format!("{field}.command"),
                    format!(
                        "`{}` must contain the value placeholder {{}} exactly once",
                        cfg.command
                    ),
                );
            }
            if !(cfg.min.is_finite() && cfg.max.is_finite())
                || cfg.min < 0.0
                || cfg.max > 100.0
                || cfg.min >= cfg.max
            {
                c.push(
                    &field,
                    format!(
                        "range {}..={} is not valid; need 0 <= min < max <= 100",
                        cfg.min, cfg.max
                    ),
                );
            } else if !(cfg.step.is_finite() && cfg.step > 0.0 && cfg.step <= cfg.max - cfg.min) {
                c.push(
                    format!("{field}.step"),
                    format!(
                        "{} must be positive and at most the range width {}",
                        cfg.step,
                        cfg.max - cfg.min
                    ),
                );
            }
        }
    }

    fn check_switches(&self, c: &mut Checker) {
        let mut seen = BTreeSet::new();
        for sw in &self.switches {
            let a = sw.actuator;
            let field = format!("switches.{a}");
            if !seen.insert(a) {
                c.push(&field, "is declared more than once");
            }
            if !a.is_switch() {
                c.push(
                    &field,
                    format!("{a} is a continuous actuator; declare it under [[actuators]]"),
                );
            }
            if self.actuator(a).is_some() {
                c.push(&field, "is also declared under [[actuators]]");
            }
            check_line(c, &format!("{field}.on"), &sw.on);
            check_line(c, &format!("{field}.off"), &sw.off);
            if sw.on == sw.off {
                c.push(&field, "on and off commands must differ");
            }
        }
    }

    fn check_safe_state(&self, c: &mut Checker) {
        for (actuator, value) in &self.safe_state {
            let field = format!("safe_state.{actuator}");
            if let Some(cfg) = self.actuator(*actuator) {
                match value {
                    SafeValue::Value(v) if !in_range(*v, cfg.min, cfg.max) => c.push(
                        &field,
                        format!("{v} is outside the range {}..={}", cfg.min, cfg.max),
                    ),
                    SafeValue::On | SafeValue::Off => c.push(
                        &field,
                        format!("{actuator} is continuous; use a percent value or \"keep\""),
                    ),
                    _ => {}
                }
            } else if self.switch(*actuator).is_some() {
                if let SafeValue::Value(_) = value {
                    c.push(
                        &field,
                        format!("{actuator} is a switch; use \"on\", \"off\" or \"keep\""),
                    );
                }
            } else {
                c.push(
                    &field,
                    format!("the machine has no {actuator} actuator"),
                );
            }
        }

        if let Some(burner) = self.actuator(Actuator::Burner) {
            match self.safe_state.get(&Actuator::Burner) {
                Some(SafeValue::Value(v)) if same(*v, burner.min) => {}
                Some(other) => c.push(
                    "safe_state.burner",
                    format!(
                        "is {other}; the safe state must set the burner to its minimum {}",
                        burner.min
                    ),
                ),
                None => c.push(
                    "safe_state.burner",
                    format!(
                        "is missing; the safe state must set the burner to its minimum {}",
                        burner.min
                    ),
                ),
            }
        }

        if let Some(air) = self.actuator(Actuator::Air) {
            match self.safe_state.get(&Actuator::Air) {
                Some(SafeValue::Value(v)) if *v > air.min => {}
                Some(other) => c.push(
                    "safe_state.air",
                    format!(
                        "is {other}; air must stay on in the safe state (typically {})",
                        air.max
                    ),
                ),
                None => c.push(
                    "safe_state.air",
                    format!(
                        "is missing; air must stay on in the safe state (typically {})",
                        air.max
                    ),
                ),
            }
        }

        if let Some(drum) = self.actuator(Actuator::Drum) {
            let value = self
                .safe_state
                .get(&Actuator::Drum)
                .copied()
                .unwrap_or(SafeValue::Keep);
            match value {
                SafeValue::Value(v) if v > 0.0 => {}
                SafeValue::Keep if drum.min > 0.0 => {}
                SafeValue::Keep => c.push(
                    "safe_state.drum",
                    "\"keep\" (or leaving it out) could leave the drum stopped because its \
                     minimum is 0; set a running speed",
                ),
                other => c.push(
                    "safe_state.drum",
                    format!("is {other}; the safe state must never stop the drum"),
                ),
            }
        }
    }

    fn check_limits(&self, c: &mut Checker) {
        let l = &self.limits;
        let (lo, hi) = MAX_BT_RANGE_C;
        if !(l.max_bt_c.is_finite() && l.max_bt_c > lo && l.max_bt_c <= hi) {
            c.push(
                "limits.max_bt_c",
                format!("{} °C is outside {lo} < max_bt_c <= {hi}", l.max_bt_c),
            );
        }
        if !(l.ai_burner_max_step > 0.0 && l.ai_burner_max_step <= AI_BURNER_STEP_CEILING) {
            c.push(
                "limits.ai_burner_max_step",
                format!(
                    "{} is outside 0 < step <= {AI_BURNER_STEP_CEILING}",
                    l.ai_burner_max_step
                ),
            );
        }
        if !(l.ai_air_max_step > 0.0 && l.ai_air_max_step <= AI_AIR_STEP_CEILING) {
            c.push(
                "limits.ai_air_max_step",
                format!(
                    "{} is outside 0 < step <= {AI_AIR_STEP_CEILING}",
                    l.ai_air_max_step
                ),
            );
        }
        for (field, v) in [
            ("limits.ai_burner_min_interval_s", l.ai_burner_min_interval_s),
            ("limits.ai_air_min_interval_s", l.ai_air_min_interval_s),
        ] {
            if !in_range(v, 1.0, 600.0) {
                c.push(field, format!("{v} s is outside 1..=600 s"));
            }
        }
    }

    fn check_capabilities(&self, c: &mut Checker) {
        let any_reported = self.actuators.iter().any(|a| a.reported);
        if self.capabilities.reports_actuators != any_reported {
            c.push(
                "capabilities.reports_actuators",
                format!(
                    "is {} but {} actuator is marked reported",
                    self.capabilities.reports_actuators,
                    if any_reported { "an" } else { "no" }
                ),
            );
        }
    }
}

fn check_serial(c: &mut Checker, s: &SerialConfig, profile: &MachineProfile) {
    if s.port_hint.trim().is_empty() {
        c.push("transport.port_hint", "must not be empty");
    }
    if !(300..=4_000_000).contains(&s.baud_rate) {
        c.push(
            "transport.baud_rate",
            format!("{} is outside 300..=4000000", s.baud_rate),
        );
    }
    if !(5..=8).contains(&s.data_bits) {
        c.push(
            "transport.data_bits",
            format!("{} is outside 5..=8", s.data_bits),
        );
    }
    if !matches!(s.stop_bits, 1 | 2) {
        c.push(
            "transport.stop_bits",
            format!("{} must be 1 or 2", s.stop_bits),
        );
    }
    let timeout_s = s.timeout_ms as f64 / 1000.0;
    if !(50..=10_000).contains(&s.timeout_ms) {
        c.push(
            "transport.timeout_ms",
            format!("{} is outside 50..=10000", s.timeout_ms),
        );
    } else if timeout_s >= profile.sampling_interval_s {
        c.push(
            "transport.timeout_ms",
            format!(
                "{} ms must be shorter than the sampling interval ({} s)",
                s.timeout_ms, profile.sampling_interval_s
            ),
        );
    }
}

/// A line sent to the machine must not be empty or contain line breaks.
fn check_line(c: &mut Checker, field: &str, line: &str) {
    if line.trim().is_empty() {
        c.push(field, "must not be empty");
    } else if line.contains(['\n', '\r']) {
        c.push(field, format!("{line:?} must not contain line breaks"));
    }
}

fn in_range(v: f64, lo: f64, hi: f64) -> bool {
    v.is_finite() && v >= lo && v <= hi
}

fn same(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}

#[derive(Default)]
struct Checker {
    issues: Vec<ValidationIssue>,
}

impl Checker {
    fn push(&mut self, field: impl Into<String>, message: impl Into<String>) {
        self.issues.push(ValidationIssue {
            field: field.into(),
            message: message.into(),
        });
    }
}
