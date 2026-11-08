//! The safety layer. Every actuator change passes through [`Safety`]; nothing
//! else may command the roaster (see `docs/safety.md`).
//!
//! The layer is a plain state machine without clocks or I/O: callers pass the
//! current monotonic time in seconds (`now`) and send the returned
//! [`ActuatorCommand`]s to the driver. That keeps every rule unit-testable.
//!
//! Order of checks for a request:
//! 1. the machine has the actuator and is connected;
//! 2. the emergency-stop latch;
//! 3. overheat (the burner may not be raised above the BT limit);
//! 4. arbitration: a human command always applies and suspends AI control;
//! 5. AI/autopilot: not suspended, valid lease, recent presence confirmation;
//! 6. range clamp, and for AI/autopilot the step clamp and the rate limit.

use std::collections::{BTreeMap, BTreeSet};

use roast_core::{
    Actuator, ActuatorCommand, ActuatorValues, CommandOutcome, CommandRequest, CommandSource,
    RejectReason,
};
use roast_machine::{MachineProfile, SafeValue};
use serde::{Deserialize, Serialize};

/// The overheat state ends once BT is this far below the limit, °C.
const OVERHEAT_HYSTERESIS_C: f64 = 5.0;

/// Step and rate limit of AI/autopilot commands for one actuator.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct AiStepLimit {
    /// Largest change per command, percentage points.
    pub max_step: f64,
    /// Shortest time between two changes, seconds.
    pub min_interval_s: f64,
}

/// Everything the safety layer needs to know about the machine and policy.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SafetyConfig {
    /// Range of every continuous actuator, percent.
    pub ranges: BTreeMap<Actuator, (f64, f64)>,
    /// On/off actuators.
    pub switches: BTreeSet<Actuator>,
    /// The safe state; actuators not listed keep their value.
    pub safe_state: ActuatorValues,
    /// Bean temperature at which the burner is cut, °C.
    pub max_bt_c: f64,
    /// AI/autopilot step limits per actuator.
    pub ai_limits: BTreeMap<Actuator, AiStepLimit>,
    /// How long a lease lasts without AI activity, seconds.
    pub lease_s: f64,
    /// How often a person must confirm presence while AI is in control, s.
    pub presence_interval_s: f64,
    /// Grace period after the presence interval before the safe state, s.
    pub presence_grace_s: f64,
}

impl SafetyConfig {
    /// The configuration for a machine profile with the default policy of
    /// `docs/safety.md` (lease 60 s, presence every 3 min + 30 s grace).
    #[must_use]
    pub fn from_profile(p: &MachineProfile) -> Self {
        let ranges = p
            .actuators
            .iter()
            .map(|a| (a.actuator, (a.min, a.max)))
            .collect();
        let switches = p.switches.iter().map(|s| s.actuator).collect();
        let safe_state = p
            .safe_state
            .iter()
            .filter_map(|(&a, v)| match v {
                SafeValue::Value(x) => Some((a, *x)),
                SafeValue::On => Some((a, 100.0)),
                SafeValue::Off => Some((a, 0.0)),
                SafeValue::Keep => None,
            })
            .collect();
        let l = &p.limits;
        let ai_limits = BTreeMap::from([
            (
                Actuator::Burner,
                AiStepLimit {
                    max_step: l.ai_burner_max_step,
                    min_interval_s: l.ai_burner_min_interval_s,
                },
            ),
            (
                Actuator::Air,
                AiStepLimit {
                    max_step: l.ai_air_max_step,
                    min_interval_s: l.ai_air_min_interval_s,
                },
            ),
        ]);
        SafetyConfig {
            ranges,
            switches,
            safe_state,
            max_bt_c: l.max_bt_c,
            ai_limits,
            lease_s: 60.0,
            presence_interval_s: 180.0,
            presence_grace_s: 30.0,
        }
    }
}

/// Why the safety layer acted on its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReactionKind {
    /// Emergency stop: safe state, latched.
    EmergencyStop,
    /// BT above the limit: burner cut.
    Overheat,
    /// Presence not confirmed in time while AI was in control: safe state.
    PresenceMissed,
    /// The AI stopped renewing its lease: AI control ended, actuators hold.
    LeaseExpired,
    /// The roaster link was lost: AI control suspended.
    LinkLost,
}

/// Something the safety layer did on its own, to be shown as an alarm and
/// logged.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SafetyReaction {
    /// What happened.
    pub kind: ReactionKind,
    /// A sentence for people.
    pub message: String,
    /// Commands to send to the roaster (already accounted for).
    pub commands: Vec<ActuatorCommand>,
}

/// A grant for one AI client to control the roaster.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Lease {
    /// Who holds it, for example the MCP client's name.
    pub holder: String,
    /// Monotonic time at which it expires, s.
    pub expires_at: f64,
}

/// A snapshot of the safety state for the UI and the AI.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SafetyStatus {
    /// The emergency stop is latched.
    pub emergency_stop: bool,
    /// A human took over; AI control is off until a human resumes it.
    pub ai_suspended: bool,
    /// Holder of the current lease.
    pub lease_holder: Option<String>,
    /// Seconds until the lease expires.
    pub lease_remaining_s: Option<f64>,
    /// Seconds until a presence confirmation is due (negative: overdue).
    /// `None` if presence was never confirmed.
    pub presence_due_in_s: Option<f64>,
    /// BT is above the limit.
    pub overheat: bool,
    /// The roaster is connected.
    pub connected: bool,
    /// The safety layer's view of the actuator values.
    pub actuators: ActuatorValues,
}

/// The safety layer's state machine.
#[derive(Debug, Clone)]
pub struct Safety {
    cfg: SafetyConfig,
    estop: bool,
    ai_suspended: bool,
    lease: Option<Lease>,
    presence_at: Option<f64>,
    last_ai_change: BTreeMap<Actuator, f64>,
    current: ActuatorValues,
    connected: bool,
    overheat: bool,
}

impl Safety {
    /// A safety layer for a disconnected machine.
    #[must_use]
    pub fn new(cfg: SafetyConfig) -> Self {
        Safety {
            cfg,
            estop: false,
            ai_suspended: false,
            lease: None,
            presence_at: None,
            last_ai_change: BTreeMap::new(),
            current: ActuatorValues::new(),
            connected: false,
            overheat: false,
        }
    }

    /// The configuration in use.
    #[must_use]
    pub fn config(&self) -> &SafetyConfig {
        &self.cfg
    }

    /// The state for display.
    #[must_use]
    pub fn status(&self, now: f64) -> SafetyStatus {
        SafetyStatus {
            emergency_stop: self.estop,
            ai_suspended: self.ai_suspended,
            lease_holder: self.lease.as_ref().map(|l| l.holder.clone()),
            lease_remaining_s: self.lease.as_ref().map(|l| l.expires_at - now),
            presence_due_in_s: self
                .presence_at
                .map(|p| p + self.cfg.presence_interval_s - now),
            overheat: self.overheat,
            connected: self.connected,
            actuators: self.current.clone(),
        }
    }

    /// Whether an AI client currently holds a valid lease.
    #[must_use]
    pub fn ai_in_control(&self, now: f64) -> bool {
        !self.estop && !self.ai_suspended && self.lease.as_ref().is_some_and(|l| l.expires_at > now)
    }

    /// Marks the roaster connected or disconnected. A lost link ends AI
    /// control.
    pub fn set_connected(&mut self, connected: bool) -> Option<SafetyReaction> {
        let was = self.connected;
        self.connected = connected;
        if was && !connected {
            let had_ai = self.lease.take().is_some();
            self.ai_suspended |= had_ai;
            return Some(SafetyReaction {
                kind: ReactionKind::LinkLost,
                message: "Roaster connection lost. Check the machine - you have control.".into(),
                commands: Vec::new(),
            });
        }
        None
    }

    /// Records actuator values the machine reported.
    pub fn observe(&mut self, reported: &ActuatorValues) {
        for (&a, &v) in reported {
            if v.is_finite() {
                self.current.insert(a, v);
            }
        }
    }

    /// Decides on a command request. Returns the outcome and, if applied, the
    /// command for the driver.
    pub fn request(
        &mut self,
        req: &CommandRequest,
        now: f64,
    ) -> (CommandOutcome, Option<ActuatorCommand>) {
        match self.decide(req, now) {
            Ok((value, clamped_from)) => {
                if req.source.is_automated() {
                    let away = self.current.get(&req.actuator).is_some_and(|&cur| {
                        cur != value && !self.toward_safe(req.actuator, cur, value)
                    });
                    if away {
                        self.last_ai_change.insert(req.actuator, now);
                    }
                    if let Some(l) = &mut self.lease {
                        l.expires_at = now + self.cfg.lease_s;
                    }
                }
                self.current.insert(req.actuator, value);
                (
                    CommandOutcome::Applied {
                        value,
                        clamped_from,
                    },
                    Some(ActuatorCommand {
                        actuator: req.actuator,
                        value,
                    }),
                )
            }
            Err(reason) => (CommandOutcome::Rejected { reason }, None),
        }
    }

    fn decide(
        &mut self,
        req: &CommandRequest,
        now: f64,
    ) -> Result<(f64, Option<f64>), RejectReason> {
        let a = req.actuator;
        let is_switch = self.cfg.switches.contains(&a);
        if !is_switch && !self.cfg.ranges.contains_key(&a) {
            return Err(RejectReason::NotSupported);
        }
        if !self.connected {
            return Err(RejectReason::Disconnected);
        }
        if !req.value.is_finite() {
            return Err(RejectReason::OutOfRange);
        }
        let (mut value, mut clamped_from) = self.clamp_to_range(a, req.value);
        if req.source == CommandSource::Safety {
            return Ok((value, clamped_from));
        }

        let current = self.current.get(&a).copied();
        let raises_burner = a == Actuator::Burner && value > current.unwrap_or(0.0);
        if self.estop && (req.source.is_automated() || raises_burner) {
            return Err(RejectReason::EmergencyStopLatched);
        }
        if self.overheat && raises_burner {
            return Err(RejectReason::Overheat);
        }

        if req.source == CommandSource::Human {
            if self.lease.take().is_some() {
                self.ai_suspended = true;
            }
            return Ok((value, clamped_from));
        }

        // AI and autopilot.
        if self.ai_suspended {
            return Err(RejectReason::AiSuspendedByHuman);
        }
        if !self.lease.as_ref().is_some_and(|l| l.expires_at > now) {
            return Err(RejectReason::NoLease);
        }
        if !self.presence_fresh(now) {
            return Err(RejectReason::PresenceRequired);
        }
        if let Some(limit) = self.cfg.ai_limits.get(&a).copied() {
            let Some(cur) = current else {
                // Without a known current value a step cannot be bounded.
                return Err(RejectReason::OutOfRange);
            };
            // Moving toward the safe state (less burner, more air) is never
            // limited: cutting heat quickly is what experienced roasters do
            // before and during first crack. Only moves away from it are
            // bounded in size and rate.
            if !self.toward_safe(a, cur, value) {
                if self
                    .last_ai_change
                    .get(&a)
                    .is_some_and(|&t| now - t < limit.min_interval_s)
                {
                    return Err(RejectReason::RateLimited);
                }
                let bounded = value.clamp(cur - limit.max_step, cur + limit.max_step);
                if bounded != value {
                    clamped_from = clamped_from.or(Some(req.value));
                    value = bounded;
                }
            }
        }
        Ok((value, clamped_from))
    }

    /// Whether `cur -> value` moves toward (or stays at) the safe-state value.
    /// Actuators without a safe value have no safe direction.
    fn toward_safe(&self, a: Actuator, cur: f64, value: f64) -> bool {
        self.cfg
            .safe_state
            .get(&a)
            .is_some_and(|&s| (value - s).abs() <= (cur - s).abs())
    }

    fn clamp_to_range(&self, a: Actuator, v: f64) -> (f64, Option<f64>) {
        if self.cfg.switches.contains(&a) {
            let on = if v > 0.0 { 100.0 } else { 0.0 };
            return (on, (on != v).then_some(v));
        }
        let (min, max) = self.cfg.ranges.get(&a).copied().unwrap_or((0.0, 100.0));
        let c = v.clamp(min, max);
        (c, (c != v).then_some(v))
    }

    fn presence_fresh(&self, now: f64) -> bool {
        self.presence_at
            .is_some_and(|p| now - p <= self.cfg.presence_interval_s + self.cfg.presence_grace_s)
    }

    fn safe_state_commands(&mut self) -> Vec<ActuatorCommand> {
        let cmds: Vec<ActuatorCommand> = self
            .cfg
            .safe_state
            .iter()
            .map(|(&actuator, &value)| ActuatorCommand { actuator, value })
            .collect();
        for c in &cmds {
            self.current.insert(c.actuator, c.value);
        }
        cmds
    }

    /// Emergency stop: latches, ends AI control, returns the safe state.
    /// Works whether or not the roaster is connected (the commands are sent
    /// if it is).
    pub fn emergency_stop(&mut self, reason: &str) -> SafetyReaction {
        self.estop = true;
        self.ai_suspended = true;
        self.lease = None;
        SafetyReaction {
            kind: ReactionKind::EmergencyStop,
            message: format!("Emergency stop ({reason}): burner off, air high, drum running."),
            commands: self.safe_state_commands(),
        }
    }

    /// Releases the emergency-stop latch. Only a person may call this (from
    /// the app); AI control stays suspended until resumed.
    pub fn reset_emergency_stop(&mut self) {
        self.estop = false;
    }

    /// A person confirmed being at the roaster.
    pub fn confirm_presence(&mut self, now: f64) {
        self.presence_at = Some(now);
    }

    /// A person allows AI control again after taking over.
    pub fn resume_ai(&mut self) {
        self.ai_suspended = false;
    }

    /// Grants or renews a lease. Needs a recent presence confirmation.
    pub fn acquire_lease(&mut self, holder: &str, now: f64) -> Result<Lease, RejectReason> {
        if self.estop {
            return Err(RejectReason::EmergencyStopLatched);
        }
        if self.ai_suspended {
            return Err(RejectReason::AiSuspendedByHuman);
        }
        if !self.connected {
            return Err(RejectReason::Disconnected);
        }
        if !self.presence_fresh(now) {
            return Err(RejectReason::PresenceRequired);
        }
        let lease = Lease {
            holder: holder.to_owned(),
            expires_at: now + self.cfg.lease_s,
        };
        self.lease = Some(lease.clone());
        Ok(lease)
    }

    /// Gives up the lease (AI finished); actuators hold.
    pub fn release_lease(&mut self) {
        self.lease = None;
    }

    /// Periodic checks: overheat, lease expiry, presence. Call at least once
    /// per second with the latest bean temperature.
    pub fn tick(&mut self, now: f64, bt: Option<f64>) -> Vec<SafetyReaction> {
        let mut out = Vec::new();

        if let Some(bt) = bt.filter(|b| b.is_finite()) {
            let burner_on = self.current.get(&Actuator::Burner).is_none_or(|&b| b > 0.0);
            let over = bt >= self.cfg.max_bt_c
                || (self.overheat && bt >= self.cfg.max_bt_c - OVERHEAT_HYSTERESIS_C);
            if over && (!self.overheat || burner_on) {
                let first = !self.overheat;
                self.overheat = true;
                if self.lease.take().is_some() {
                    self.ai_suspended = true;
                }
                let burner = self
                    .cfg
                    .safe_state
                    .get(&Actuator::Burner)
                    .copied()
                    .unwrap_or(0.0);
                self.current.insert(Actuator::Burner, burner);
                out.push(SafetyReaction {
                    kind: ReactionKind::Overheat,
                    message: if first {
                        format!(
                            "Bean temperature {bt:.0} °C reached the limit of {:.0} °C: burner cut.",
                            self.cfg.max_bt_c
                        )
                    } else {
                        "Burner cut again: still above the temperature limit.".into()
                    },
                    commands: vec![ActuatorCommand {
                        actuator: Actuator::Burner,
                        value: burner,
                    }],
                });
            } else if self.overheat && bt < self.cfg.max_bt_c - OVERHEAT_HYSTERESIS_C {
                self.overheat = false;
            }
        }

        if let Some(lease) = &self.lease {
            if !self.presence_fresh(now) {
                self.lease = None;
                self.ai_suspended = true;
                out.push(SafetyReaction {
                    kind: ReactionKind::PresenceMissed,
                    message: "Presence was not confirmed in time: safe state, AI control ended."
                        .into(),
                    commands: self.safe_state_commands(),
                });
            } else if lease.expires_at <= now {
                let holder = lease.holder.clone();
                self.lease = None;
                out.push(SafetyReaction {
                    kind: ReactionKind::LeaseExpired,
                    message: format!("AI ({holder}) disconnected - you have control."),
                    commands: Vec::new(),
                });
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> SafetyConfig {
        SafetyConfig {
            ranges: BTreeMap::from([
                (Actuator::Burner, (0.0, 100.0)),
                (Actuator::Air, (0.0, 100.0)),
                (Actuator::Drum, (80.0, 100.0)),
            ]),
            switches: BTreeSet::from([Actuator::Cooling]),
            safe_state: BTreeMap::from([
                (Actuator::Burner, 0.0),
                (Actuator::Air, 100.0),
                (Actuator::Drum, 100.0),
            ]),
            max_bt_c: 240.0,
            ai_limits: BTreeMap::from([
                (
                    Actuator::Burner,
                    AiStepLimit {
                        max_step: 5.0,
                        min_interval_s: 5.0,
                    },
                ),
                (
                    Actuator::Air,
                    AiStepLimit {
                        max_step: 10.0,
                        min_interval_s: 2.0,
                    },
                ),
            ]),
            lease_s: 60.0,
            presence_interval_s: 180.0,
            presence_grace_s: 30.0,
        }
    }

    fn connected() -> Safety {
        let mut s = Safety::new(cfg());
        s.set_connected(true);
        s.observe(&BTreeMap::from([
            (Actuator::Burner, 50.0),
            (Actuator::Air, 40.0),
        ]));
        s
    }

    fn req(actuator: Actuator, value: f64, source: CommandSource) -> CommandRequest {
        CommandRequest {
            actuator,
            value,
            source,
            reason: None,
        }
    }

    fn applied(o: &CommandOutcome) -> Option<f64> {
        match o {
            CommandOutcome::Applied { value, .. } => Some(*value),
            _ => None,
        }
    }

    fn rejected(o: &CommandOutcome) -> Option<RejectReason> {
        match o {
            CommandOutcome::Rejected { reason } => Some(*reason),
            _ => None,
        }
    }

    fn with_ai(now: f64) -> Safety {
        let mut s = connected();
        s.confirm_presence(now);
        s.acquire_lease("claude", now).unwrap();
        s
    }

    #[test]
    fn human_commands_clamp_to_range_only() {
        let mut s = connected();
        let (o, c) = s.request(&req(Actuator::Burner, 100.0, CommandSource::Human), 0.0);
        assert_eq!(applied(&o), Some(100.0));
        assert_eq!(c.unwrap().value, 100.0);
        let (o, _) = s.request(&req(Actuator::Drum, 10.0, CommandSource::Human), 0.0);
        assert_eq!(
            o,
            CommandOutcome::Applied {
                value: 80.0,
                clamped_from: Some(10.0)
            }
        );
    }

    #[test]
    fn nothing_applies_while_disconnected() {
        let mut s = Safety::new(cfg());
        let (o, c) = s.request(&req(Actuator::Burner, 10.0, CommandSource::Human), 0.0);
        assert_eq!(rejected(&o), Some(RejectReason::Disconnected));
        assert!(c.is_none());
    }

    #[test]
    fn unknown_actuators_are_rejected() {
        let mut s = connected();
        let (o, _) = s.request(&req(Actuator::Filter, 10.0, CommandSource::Human), 0.0);
        assert_eq!(rejected(&o), Some(RejectReason::NotSupported));
    }

    #[test]
    fn ai_needs_lease_and_presence() {
        let mut s = connected();
        let (o, _) = s.request(&req(Actuator::Burner, 52.0, CommandSource::Ai), 0.0);
        assert_eq!(rejected(&o), Some(RejectReason::NoLease));
        assert_eq!(
            s.acquire_lease("claude", 0.0),
            Err(RejectReason::PresenceRequired)
        );
        s.confirm_presence(0.0);
        s.acquire_lease("claude", 0.0).unwrap();
        let (o, _) = s.request(&req(Actuator::Burner, 52.0, CommandSource::Ai), 1.0);
        assert_eq!(applied(&o), Some(52.0));
    }

    #[test]
    fn ai_raises_are_bounded_but_cuts_are_free() {
        let mut s = with_ai(0.0);
        // Raising the burner (away from safe): one bounded step per interval.
        let (o, _) = s.request(&req(Actuator::Burner, 90.0, CommandSource::Ai), 0.0);
        assert_eq!(
            o,
            CommandOutcome::Applied {
                value: 55.0,
                clamped_from: Some(90.0)
            }
        );
        let (o, _) = s.request(&req(Actuator::Burner, 60.0, CommandSource::Ai), 3.0);
        assert_eq!(rejected(&o), Some(RejectReason::RateLimited));
        let (o, _) = s.request(&req(Actuator::Burner, 60.0, CommandSource::Ai), 5.0);
        assert_eq!(applied(&o), Some(60.0));
        // Cutting the burner (toward safe): any size, right away.
        let (o, _) = s.request(&req(Actuator::Burner, 20.0, CommandSource::Ai), 5.5);
        assert_eq!(applied(&o), Some(20.0));
        let (o, _) = s.request(&req(Actuator::Burner, 0.0, CommandSource::Ai), 6.0);
        assert_eq!(applied(&o), Some(0.0));
        // More air is toward safe; less air is bounded.
        let (o, _) = s.request(&req(Actuator::Air, 100.0, CommandSource::Ai), 6.0);
        assert_eq!(applied(&o), Some(100.0));
        let (o, _) = s.request(&req(Actuator::Air, 20.0, CommandSource::Ai), 6.5);
        assert_eq!(applied(&o), Some(90.0));
    }

    #[test]
    fn human_takes_over_and_suspends_ai() {
        let mut s = with_ai(0.0);
        let (o, _) = s.request(&req(Actuator::Burner, 30.0, CommandSource::Human), 1.0);
        assert_eq!(applied(&o), Some(30.0));
        let (o, _) = s.request(&req(Actuator::Burner, 32.0, CommandSource::Ai), 10.0);
        assert_eq!(rejected(&o), Some(RejectReason::AiSuspendedByHuman));
        assert_eq!(
            s.acquire_lease("claude", 10.0),
            Err(RejectReason::AiSuspendedByHuman)
        );
        s.resume_ai();
        s.acquire_lease("claude", 10.0).unwrap();
        let (o, _) = s.request(&req(Actuator::Burner, 32.0, CommandSource::Ai), 11.0);
        assert_eq!(applied(&o), Some(32.0));
    }

    #[test]
    fn emergency_stop_latches_and_sets_safe_state() {
        let mut s = with_ai(0.0);
        let r = s.emergency_stop("button");
        assert_eq!(r.kind, ReactionKind::EmergencyStop);
        let burner = r
            .commands
            .iter()
            .find(|c| c.actuator == Actuator::Burner)
            .unwrap();
        assert_eq!(burner.value, 0.0);
        let air = r
            .commands
            .iter()
            .find(|c| c.actuator == Actuator::Air)
            .unwrap();
        assert_eq!(air.value, 100.0);
        assert!(
            r.commands
                .iter()
                .any(|c| c.actuator == Actuator::Drum && c.value == 100.0)
        );

        let (o, _) = s.request(&req(Actuator::Burner, 5.0, CommandSource::Ai), 1.0);
        assert_eq!(rejected(&o), Some(RejectReason::EmergencyStopLatched));
        let (o, _) = s.request(&req(Actuator::Burner, 5.0, CommandSource::Human), 1.0);
        assert_eq!(rejected(&o), Some(RejectReason::EmergencyStopLatched));
        // A person may still move air and drum while latched.
        let (o, _) = s.request(&req(Actuator::Air, 60.0, CommandSource::Human), 1.0);
        assert_eq!(applied(&o), Some(60.0));
        s.reset_emergency_stop();
        let (o, _) = s.request(&req(Actuator::Burner, 5.0, CommandSource::Human), 2.0);
        assert_eq!(applied(&o), Some(5.0));
        // AI stays suspended until resumed.
        assert!(!s.ai_in_control(2.0));
    }

    #[test]
    fn overheat_cuts_the_burner_and_blocks_raising_it() {
        let mut s = connected();
        let r = s.tick(0.0, Some(241.0));
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].kind, ReactionKind::Overheat);
        assert_eq!(r[0].commands[0].value, 0.0);
        assert!(
            s.tick(1.0, Some(241.0)).is_empty(),
            "no repeat while burner is off"
        );
        let (o, _) = s.request(&req(Actuator::Burner, 20.0, CommandSource::Human), 1.0);
        assert_eq!(rejected(&o), Some(RejectReason::Overheat));
        // The machine reports the burner on again (panel): cut again.
        s.observe(&BTreeMap::from([(Actuator::Burner, 30.0)]));
        assert_eq!(s.tick(2.0, Some(239.0)).len(), 1);
        assert!(s.tick(3.0, Some(234.0)).is_empty());
        let (o, _) = s.request(&req(Actuator::Burner, 20.0, CommandSource::Human), 4.0);
        assert_eq!(applied(&o), Some(20.0));
    }

    #[test]
    fn lease_expiry_holds_actuators() {
        let mut s = with_ai(0.0);
        assert!(s.tick(59.0, Some(150.0)).is_empty());
        let r = s.tick(60.0, Some(150.0));
        assert_eq!(r[0].kind, ReactionKind::LeaseExpired);
        assert!(r[0].commands.is_empty());
        assert!(!s.ai_in_control(61.0));
    }

    #[test]
    fn ai_activity_renews_the_lease() {
        let mut s = with_ai(0.0);
        s.request(&req(Actuator::Air, 45.0, CommandSource::Ai), 50.0);
        assert!(s.tick(100.0, None).is_empty());
        assert!(s.ai_in_control(100.0));
    }

    #[test]
    fn missed_presence_goes_to_safe_state() {
        let mut s = with_ai(0.0);
        for t in [50.0, 100.0, 150.0, 200.0] {
            s.request(&req(Actuator::Air, 40.0 + t / 50.0, CommandSource::Ai), t);
            assert!(s.tick(t, None).is_empty(), "at {t}");
        }
        let r = s.tick(211.0, None);
        assert_eq!(r[0].kind, ReactionKind::PresenceMissed);
        assert!(
            r[0].commands
                .iter()
                .any(|c| c.actuator == Actuator::Burner && c.value == 0.0)
        );
        assert!(!s.ai_in_control(212.0));
    }

    #[test]
    fn link_loss_suspends_ai() {
        let mut s = with_ai(0.0);
        let r = s.set_connected(false).unwrap();
        assert_eq!(r.kind, ReactionKind::LinkLost);
        s.set_connected(true);
        assert!(!s.ai_in_control(1.0));
    }

    #[test]
    fn switches_are_on_or_off() {
        let mut s = connected();
        let (o, _) = s.request(&req(Actuator::Cooling, 1.0, CommandSource::Human), 0.0);
        assert_eq!(applied(&o), Some(100.0));
    }

    #[test]
    fn cubean_profile_gives_the_documented_safe_state() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../machines/skywalker-cubean-plus.toml"
        );
        let p = MachineProfile::load(path).unwrap();
        let c = SafetyConfig::from_profile(&p);
        assert_eq!(c.safe_state.get(&Actuator::Burner), Some(&0.0));
        assert_eq!(c.safe_state.get(&Actuator::Air), Some(&100.0));
        assert_eq!(c.safe_state.get(&Actuator::Drum), Some(&100.0));
        assert_eq!(c.ranges.get(&Actuator::Drum), Some(&(80.0, 100.0)));
        assert!(c.switches.contains(&Actuator::Cooling));
    }
}
