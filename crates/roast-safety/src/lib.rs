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
//! 4. arbitration: a human command always applies; on the takeover actuators
//!    (burner, air, drum by default) it also suspends AI control, while
//!    filter and cooling leave the AI in control (ADR 0019);
//! 5. AI/autopilot: not suspended, valid lease; an AI may not move away from
//!    the safe state while a presence confirmation is overdue (ADR 0023);
//! 6. range clamp, and for AI/autopilot the step clamp and the rate limit
//!    (except the autopilot's CHARGE step in the first 10 s after CHARGE).

use std::collections::{BTreeMap, BTreeSet};

use roast_core::{
    Actuator, ActuatorCommand, ActuatorValues, CommandOutcome, CommandRequest, CommandSource,
    RejectReason,
};
use roast_machine::{MachineProfile, SafeValue};
use serde::{Deserialize, Serialize};

/// The overheat state ends once BT is this far below the limit, °C.
const OVERHEAT_HYSTERESIS_C: f64 = 5.0;
/// For this long after CHARGE the autopilot may set the recipe's CHARGE
/// step at once, without step and rate limits (ADR 0019), s.
pub const CHARGE_JUMP_S: f64 = 10.0;
/// A reported actuator value further than this from the value set counts as
/// a deviation, percentage points. The Cubean+ ramps or quantises by a few
/// points (21.7 % for 25 %), which stays inside.
pub const MISMATCH_TOLERANCE: f64 = 8.0;
/// How long a deviation must last before the safety layer reacts, s. Gives
/// a machine that ramps time to arrive (four readings on the Cubean+).
pub const MISMATCH_AFTER_S: f64 = 6.0;
/// How often the value set is sent again while the machine deviates, and how
/// long after the last resend the deviation counts as not followed, s.
pub const MISMATCH_RESEND_EVERY_S: f64 = 6.0;
/// How often the value set is sent again before the safety layer gives up
/// and asks the person to take over.
pub const MISMATCH_RESENDS: u32 = 2;
/// While a presence confirmation is overdue, the alarm repeats this often, s.
pub const PRESENCE_REMIND_S: f64 = 30.0;

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
    /// The longer presence interval while the preheat holds the lease before
    /// CHARGE (empty drum, bounded by its own limits; ADR 0018), s.
    pub preheat_presence_interval_s: f64,
    /// Actuators on which a person's command ends AI control (ADR 0019).
    pub takeover: BTreeSet<Actuator>,
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
            preheat_presence_interval_s: l
                .preheat_presence_interval_s
                .unwrap_or(DEFAULT_PREHEAT_PRESENCE_INTERVAL_S),
            takeover: l.takeover_actuators().into_iter().collect(),
        }
    }
}

/// Presence interval while preheating before CHARGE when the machine profile
/// sets none, s.
pub const DEFAULT_PREHEAT_PRESENCE_INTERVAL_S: f64 = 600.0;

/// Why the safety layer acted on its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReactionKind {
    /// Emergency stop: safe state, latched.
    EmergencyStop,
    /// BT above the limit: burner cut.
    Overheat,
    /// Presence not confirmed in time while AI was in control: a loud alarm,
    /// repeated; control goes on, and an AI may not raise the heat until a
    /// person confirms (ADR 0023). No safe state.
    PresenceMissed,
    /// The roaster reports a burner or air value other than the one set
    /// (ADR 0023): the value set is sent again, a few times, then the person
    /// is asked to take over.
    ActuatorMismatch,
    /// The AI stopped renewing its lease: AI control ended, actuators hold.
    LeaseExpired,
    /// The roaster link was lost: AI control suspended.
    LinkLost,
    /// The roaster (re)connected: the safe state is set, because a machine may
    /// start at its own defaults (the Cubean+ heats at 100 % on connect).
    Connected,
    /// The roast runs away from what beans do (no beans in the drum, a false
    /// CHARGE): safe state, automation ended. Not latched.
    Runaway,
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
    /// The safety layer's view of the actuator values: what the machine
    /// reports where it reports them, else what was set.
    pub actuators: ActuatorValues,
    /// Presence is asked for while someone holds control (the person can
    /// switch the questions off in the app; ADR 0023).
    pub presence_checks: bool,
}

/// A reported actuator value that does not match the value set.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Mismatch {
    /// When the deviation was first seen, s.
    since: f64,
    /// Resends so far.
    resends: u32,
    /// When the value was last sent again, s.
    last_resend: Option<f64>,
}

/// The safety layer's state machine.
#[derive(Debug, Clone)]
pub struct Safety {
    cfg: SafetyConfig,
    estop: bool,
    ai_suspended: bool,
    lease: Option<Lease>,
    presence_at: Option<f64>,
    /// The lease holder that gets the longer preheat presence interval.
    preheat_holder: Option<String>,
    last_ai_change: BTreeMap<Actuator, f64>,
    current: ActuatorValues,
    /// The values commanded through this layer, to compare with what the
    /// machine reports.
    setpoint: ActuatorValues,
    mismatch: BTreeMap<Actuator, Mismatch>,
    /// Whether presence is asked for at all (ADR 0023).
    presence_checks: bool,
    /// When the overdue-presence alarm last sounded.
    presence_alarm_at: Option<f64>,
    connected: bool,
    overheat: bool,
    /// Until when the autopilot may jump to the CHARGE step.
    charge_jump_until: Option<f64>,
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
            preheat_holder: None,
            last_ai_change: BTreeMap::new(),
            current: ActuatorValues::new(),
            setpoint: ActuatorValues::new(),
            mismatch: BTreeMap::new(),
            presence_checks: true,
            presence_alarm_at: None,
            connected: false,
            overheat: false,
            charge_jump_until: None,
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
            // Switched off, nothing is due: no gongs, no prompt.
            presence_due_in_s: self
                .presence_at
                .filter(|_| self.presence_checks)
                .map(|p| p + self.presence_interval(self.lease_holder()) - now),
            overheat: self.overheat,
            connected: self.connected,
            actuators: self.current.clone(),
            presence_checks: self.presence_checks,
        }
    }

    /// Switches the presence questions on or off (only a person, in the app;
    /// ADR 0023). Off, nobody is asked while control runs; an AI that takes
    /// control still needs one confirmation first.
    pub fn set_presence_checks(&mut self, on: bool) {
        self.presence_checks = on;
        self.presence_alarm_at = None;
    }

    /// Whether presence is asked for while control runs.
    #[must_use]
    pub fn presence_checks(&self) -> bool {
        self.presence_checks
    }

    /// Whether an AI client currently holds a valid lease.
    #[must_use]
    pub fn ai_in_control(&self, now: f64) -> bool {
        !self.estop && !self.ai_suspended && self.lease.as_ref().is_some_and(|l| l.expires_at > now)
    }

    /// Marks the roaster connected or disconnected. A lost link ends AI
    /// control; a new connection gets the safe state, whatever the machine
    /// started with.
    pub fn set_connected(&mut self, connected: bool) -> Option<SafetyReaction> {
        let was = self.connected;
        self.connected = connected;
        if !was && connected {
            return Some(SafetyReaction {
                kind: ReactionKind::Connected,
                message: "Burner off, air high, drum running until you change them.".into(),
                commands: self.safe_state_commands(),
            });
        }
        if was && !connected {
            self.mismatch.clear();
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

    /// Records the values the machine reported and compares them with the
    /// values set (ADR 0023). A deviation of more than
    /// [`MISMATCH_TOLERANCE`] for [`MISMATCH_AFTER_S`] gets the value set sent
    /// again, up to [`MISMATCH_RESENDS`] times every
    /// [`MISMATCH_RESEND_EVERY_S`]; then the person is asked to take over and
    /// the reported value counts as the value set. A value toward the safe
    /// state is always sent again; one away from it not during an emergency
    /// stop or an overheat, when the machine's lower heat is the safer side.
    pub fn follow(&mut self, reported: &ActuatorValues, now: f64) -> Option<SafetyReaction> {
        self.observe(reported);
        if !self.connected {
            self.mismatch.clear();
            return None;
        }
        let mut resend = Vec::new();
        let mut gave_up = Vec::new();
        for (&a, &v) in reported {
            let Some(&set) = self.setpoint.get(&a) else {
                continue;
            };
            if !v.is_finite() || (v - set).abs() <= MISMATCH_TOLERANCE {
                self.mismatch.remove(&a);
                continue;
            }
            let may_resend = self.toward_safe(a, v, set) || !(self.estop || self.overheat);
            let m = self.mismatch.entry(a).or_insert(Mismatch {
                since: now,
                resends: 0,
                last_resend: None,
            });
            if now - m.since < MISMATCH_AFTER_S {
                continue;
            }
            let since_resend = m.last_resend.map_or(f64::INFINITY, |t| now - t);
            if since_resend < MISMATCH_RESEND_EVERY_S {
                continue;
            }
            if may_resend && m.resends < MISMATCH_RESENDS {
                m.resends += 1;
                m.last_resend = Some(now);
                resend.push((a, v, set));
            } else {
                gave_up.push((a, v, set));
            }
        }
        let list = |items: &[(Actuator, f64, f64)], which: fn(&(Actuator, f64, f64)) -> f64| {
            items
                .iter()
                .map(|x| format!("{} {:.0} %", x.0, which(x)))
                .collect::<Vec<_>>()
                .join(" and ")
        };
        if !gave_up.is_empty() {
            for &(a, v, _) in &gave_up {
                self.mismatch.remove(&a);
                self.setpoint.insert(a, v);
            }
            return Some(SafetyReaction {
                kind: ReactionKind::ActuatorMismatch,
                message: format!(
                    "The roaster does not follow: it reports {}, set was {}. Check the roaster and take over.",
                    list(&gave_up, |x| x.1),
                    list(&gave_up, |x| x.2),
                ),
                commands: Vec::new(),
            });
        }
        if resend.is_empty() {
            return None;
        }
        let commands = resend
            .iter()
            .map(|&(actuator, _, value)| {
                self.current.insert(actuator, value);
                ActuatorCommand { actuator, value }
            })
            .collect();
        Some(SafetyReaction {
            kind: ReactionKind::ActuatorMismatch,
            message: format!(
                "The roaster reports {}, but {} was set: sent again.",
                list(&resend, |x| x.1),
                list(&resend, |x| x.2),
            ),
            commands,
        })
    }

    /// A value commanded through this layer: the new view and the value the
    /// machine should report from now on.
    fn set(&mut self, a: Actuator, v: f64) {
        self.current.insert(a, v);
        self.setpoint.insert(a, v);
        self.mismatch.remove(&a);
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
                }
                // The lease is the AI's sign of life: only its own commands
                // renew it, not the autopilot acting under it (ADR 0019).
                if req.source == CommandSource::Ai
                    && let Some(l) = &mut self.lease
                {
                    l.expires_at = now + self.cfg.lease_s;
                }
                self.set(req.actuator, value);
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
            if self.cfg.takeover.contains(&a) && self.lease.take().is_some() {
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
        // An overdue confirmation ends nothing (ADR 0023): the autopilot,
        // which a person switched on, goes on; an AI may still cut heat and
        // add air, but not move away from the safe state.
        let away = current.is_some_and(|cur| !self.toward_safe(a, cur, value));
        if req.source == CommandSource::Ai
            && self.presence_checks
            && away
            && !self.presence_fresh(now, self.lease_holder())
        {
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
            if !self.toward_safe(a, cur, value) && !self.charge_jump(req.source, now) {
                if self.rate_limited(a, value, req.source, now) {
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

    /// Whether an automated request from `source` to set `a` to `value` now
    /// would be refused as too soon after the previous move away from the
    /// safe state. Moves toward the safe state, actuators without limits and
    /// the autopilot's CHARGE step never are.
    #[must_use]
    pub fn rate_limited(&self, a: Actuator, value: f64, source: CommandSource, now: f64) -> bool {
        if self.charge_jump(source, now) {
            return false;
        }
        let Some(limit) = self.cfg.ai_limits.get(&a) else {
            return false;
        };
        let away = self
            .current
            .get(&a)
            .is_some_and(|&cur| !self.toward_safe(a, cur, value));
        away && self
            .last_ai_change
            .get(&a)
            .is_some_and(|&t| now - t < limit.min_interval_s)
    }

    /// Whether the autopilot may set its CHARGE step without step limits.
    fn charge_jump(&self, source: CommandSource, now: f64) -> bool {
        source == CommandSource::Autopilot && self.charge_jump_until.is_some_and(|u| now < u)
    }

    /// CHARGE: for the next [`CHARGE_JUMP_S`] seconds the autopilot may set
    /// the recipe's CHARGE step at once. The cold beans take the heat; a ramp
    /// from the holding power would take most of a minute (ADR 0019). AI
    /// commands keep their limits, and range, overheat and presence still
    /// apply.
    pub fn allow_charge_jump(&mut self, now: f64) {
        self.charge_jump_until = Some(now + CHARGE_JUMP_S);
    }

    /// Whether a person's command on `a` ends AI control.
    #[must_use]
    pub fn ends_ai_control(&self, a: Actuator) -> bool {
        self.cfg.takeover.contains(&a)
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

    fn lease_holder(&self) -> Option<&str> {
        self.lease.as_ref().map(|l| l.holder.as_str())
    }

    /// How often `holder` needs a presence confirmation: the longer preheat
    /// interval for the preheat before CHARGE, the normal one for anyone else.
    fn presence_interval(&self, holder: Option<&str>) -> f64 {
        if holder.is_some() && holder == self.preheat_holder.as_deref() {
            self.cfg.preheat_presence_interval_s
        } else {
            self.cfg.presence_interval_s
        }
    }

    fn presence_fresh(&self, now: f64, holder: Option<&str>) -> bool {
        self.presence_at
            .is_some_and(|p| now - p <= self.presence_interval(holder) + self.cfg.presence_grace_s)
    }

    /// Gives the lease holder `holder` the longer preheat presence interval
    /// (ADR 0018), or nobody. The daemon names its preheat while it runs
    /// before CHARGE; any other holder keeps the normal interval, so an AI
    /// that takes over needs a recent confirmation of its own.
    pub fn set_preheat_holder(&mut self, holder: Option<&str>) {
        self.preheat_holder = holder.map(str::to_owned);
    }

    fn safe_state_commands(&mut self) -> Vec<ActuatorCommand> {
        let cmds: Vec<ActuatorCommand> = self
            .cfg
            .safe_state
            .iter()
            .map(|(&actuator, &value)| ActuatorCommand { actuator, value })
            .collect();
        for c in &cmds {
            self.set(c.actuator, c.value);
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

    /// The roast runs away (far above its plan, or heading for the
    /// temperature limit): the safe state, and automation ends until a
    /// person allows it again. Unlike the emergency stop it does not latch.
    pub fn runaway(&mut self, reason: &str) -> SafetyReaction {
        if self.lease.take().is_some() {
            self.ai_suspended = true;
        }
        let commands = self.safe_state_commands();
        SafetyReaction {
            kind: ReactionKind::Runaway,
            message: format!("{reason}: burner off, air high, drum running."),
            commands,
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
        self.presence_alarm_at = None;
    }

    /// A person allows AI control again after taking over.
    pub fn resume_ai(&mut self) {
        self.ai_suspended = false;
    }

    /// Grants or renews a lease. A new grant needs a recent presence
    /// confirmation, whether or not the questions are switched on: taking
    /// control is a critical step. Renewing does not (ADR 0023).
    pub fn acquire_lease(&mut self, holder: &str, now: f64) -> Result<Lease, RejectReason> {
        let renewing = self
            .lease
            .as_ref()
            .is_some_and(|l| l.holder == holder && l.expires_at > now);
        self.grant(holder, now, !renewing)
    }

    /// Grants or renews a lease for the daemon's own automation (preheat,
    /// autopilot). A person started it in the app, which confirmed presence;
    /// a confirmation missed since does not stop it (ADR 0023).
    pub fn acquire_own_lease(&mut self, holder: &str, now: f64) -> Result<Lease, RejectReason> {
        self.grant(holder, now, false)
    }

    fn grant(
        &mut self,
        holder: &str,
        now: f64,
        needs_presence: bool,
    ) -> Result<Lease, RejectReason> {
        if self.estop {
            return Err(RejectReason::EmergencyStopLatched);
        }
        if self.ai_suspended {
            return Err(RejectReason::AiSuspendedByHuman);
        }
        if !self.connected {
            return Err(RejectReason::Disconnected);
        }
        if needs_presence && !self.presence_fresh(now, Some(holder)) {
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
                self.set(Actuator::Burner, burner);
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
            // A missed confirmation is a loud alarm, repeated, never the end
            // of a roast or a preheat (ADR 0023).
            if self.presence_checks
                && !self.presence_fresh(now, Some(&lease.holder))
                && self
                    .presence_alarm_at
                    .is_none_or(|t| now - t >= PRESENCE_REMIND_S)
            {
                self.presence_alarm_at = Some(now);
                out.push(SafetyReaction {
                    kind: ReactionKind::PresenceMissed,
                    message: "Presence not confirmed: the roast goes on. Press Space or “I'm at the roaster”; until then an AI may not raise the heat."
                        .into(),
                    commands: Vec::new(),
                });
            }
        }
        if let Some(lease) = &self.lease {
            if lease.expires_at <= now {
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
            preheat_presence_interval_s: 600.0,
            takeover: BTreeSet::from([Actuator::Burner, Actuator::Air, Actuator::Drum]),
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
    fn a_persons_filter_and_cooling_leave_the_ai_in_control() {
        let mut c = cfg();
        c.ranges.insert(Actuator::Filter, (0.0, 100.0));
        let mut s = Safety::new(c);
        s.set_connected(true);
        s.observe(&BTreeMap::from([(Actuator::Burner, 50.0)]));
        s.confirm_presence(0.0);
        s.acquire_lease("claude", 0.0).unwrap();
        for a in [Actuator::Filter, Actuator::Cooling] {
            let (o, _) = s.request(&req(a, 100.0, CommandSource::Human), 1.0);
            assert_eq!(applied(&o), Some(100.0));
            assert!(s.ai_in_control(1.0), "{a} ended AI control");
            assert!(!s.ends_ai_control(a));
        }
        assert!(s.ends_ai_control(Actuator::Drum));
        let (o, _) = s.request(&req(Actuator::Drum, 90.0, CommandSource::Human), 2.0);
        assert_eq!(applied(&o), Some(90.0));
        assert!(!s.ai_in_control(2.0));
        assert!(s.status(2.0).ai_suspended);
    }

    #[test]
    fn the_autopilot_may_jump_to_the_charge_step_for_10_s() {
        let mut s = with_ai(0.0);
        s.observe(&BTreeMap::from([(Actuator::Burner, 36.0)]));
        // Before CHARGE: the usual 5-point steps.
        let (o, _) = s.request(&req(Actuator::Burner, 85.0, CommandSource::Autopilot), 1.0);
        assert_eq!(applied(&o), Some(41.0));
        s.allow_charge_jump(2.0);
        // The AI keeps its limits inside the window.
        let (o, _) = s.request(&req(Actuator::Burner, 85.0, CommandSource::Ai), 7.0);
        assert_eq!(applied(&o), Some(46.0));
        let (o, _) = s.request(&req(Actuator::Burner, 85.0, CommandSource::Autopilot), 8.0);
        assert_eq!(applied(&o), Some(85.0));
        // After 10 s the limits are back.
        s.observe(&BTreeMap::from([(Actuator::Burner, 40.0)]));
        let (o, _) = s.request(&req(Actuator::Burner, 85.0, CommandSource::Autopilot), 20.0);
        assert_eq!(applied(&o), Some(45.0));
    }

    #[test]
    fn only_the_ais_own_commands_renew_its_lease() {
        let mut s = with_ai(0.0);
        s.request(&req(Actuator::Air, 45.0, CommandSource::Autopilot), 50.0);
        let r = s.tick(60.0, None);
        assert_eq!(r[0].kind, ReactionKind::LeaseExpired);
    }

    #[test]
    fn the_rate_limit_can_be_asked_before_requesting() {
        let mut s = with_ai(0.0);
        let ai = CommandSource::Ai;
        assert!(!s.rate_limited(Actuator::Burner, 55.0, ai, 0.0));
        s.request(&req(Actuator::Burner, 55.0, ai), 0.0);
        assert!(s.rate_limited(Actuator::Burner, 60.0, ai, 3.0));
        assert!(
            !s.rate_limited(Actuator::Burner, 30.0, ai, 3.0),
            "cuts are free"
        );
        assert!(!s.rate_limited(Actuator::Burner, 60.0, ai, 5.0));
        s.allow_charge_jump(3.0);
        assert!(!s.rate_limited(Actuator::Burner, 90.0, CommandSource::Autopilot, 4.0));
        assert!(s.rate_limited(Actuator::Burner, 90.0, ai, 4.0));
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
    fn a_runaway_roast_gets_the_safe_state_and_ends_ai_control_without_latching() {
        let mut s = with_ai(0.0);
        let r = s.runaway("BT 189 °C, 89 °C above the plan");
        assert_eq!(r.kind, ReactionKind::Runaway);
        assert!(
            r.commands
                .iter()
                .any(|c| c.actuator == Actuator::Burner && c.value == 0.0)
        );
        assert!(!s.ai_in_control(1.0));
        assert!(s.status(1.0).ai_suspended);
        assert!(!s.status(1.0).emergency_stop, "not latched");
        // A person may go on at once.
        let (o, _) = s.request(&req(Actuator::Burner, 30.0, CommandSource::Human), 2.0);
        assert!(applied(&o).is_some());
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
    fn missed_presence_is_a_repeated_alarm_and_ends_nothing() {
        let mut s = with_ai(0.0);
        for t in [50.0, 100.0, 150.0, 200.0] {
            s.request(&req(Actuator::Air, 40.0 + t / 50.0, CommandSource::Ai), t);
            assert!(s.tick(t, None).is_empty(), "at {t}");
        }
        let r = s.tick(211.0, None);
        assert_eq!(r[0].kind, ReactionKind::PresenceMissed);
        assert!(r[0].commands.is_empty(), "no safe state");
        assert!(s.ai_in_control(212.0));
        assert!(!s.status(212.0).ai_suspended);
        // The alarm repeats every 30 s, not every tick.
        assert!(s.tick(220.0, None).is_empty());
        let r = s.tick(241.0, None);
        assert_eq!(r[0].kind, ReactionKind::PresenceMissed);
        // The AI keeps its lease, may cut heat and add air, but not raise heat.
        s.acquire_lease("claude", 242.0).unwrap();
        let (o, _) = s.request(&req(Actuator::Burner, 40.0, CommandSource::Ai), 243.0);
        assert_eq!(applied(&o), Some(40.0));
        let (o, _) = s.request(&req(Actuator::Burner, 45.0, CommandSource::Ai), 250.0);
        assert_eq!(rejected(&o), Some(RejectReason::PresenceRequired));
        // The autopilot, which a person switched on, goes on.
        let (o, _) = s.request(
            &req(Actuator::Burner, 45.0, CommandSource::Autopilot),
            251.0,
        );
        assert_eq!(applied(&o), Some(45.0));
        // A confirmation ends the alarm and lets the AI raise again.
        s.confirm_presence(260.0);
        assert!(s.tick(261.0, None).is_empty());
        let (o, _) = s.request(&req(Actuator::Burner, 50.0, CommandSource::Ai), 262.0);
        assert_eq!(applied(&o), Some(50.0));
    }

    #[test]
    fn presence_questions_can_be_switched_off_but_not_for_taking_control() {
        let mut s = with_ai(0.0);
        s.set_presence_checks(false);
        assert_eq!(s.status(0.0).presence_due_in_s, None);
        for t in [50.0, 100.0, 150.0, 200.0, 250.0, 300.0] {
            s.acquire_lease("claude", t).unwrap();
            assert!(s.tick(t, None).is_empty(), "at {t}");
        }
        let (o, _) = s.request(&req(Actuator::Burner, 55.0, CommandSource::Ai), 301.0);
        assert_eq!(applied(&o), Some(55.0));
        // Another AI taking control still needs a person's confirmation.
        s.release_lease();
        assert_eq!(
            s.acquire_lease("other", 302.0).unwrap_err(),
            RejectReason::PresenceRequired
        );
        // The daemon's own automation was started by a person.
        s.acquire_own_lease("autopilot", 303.0).unwrap();
    }

    #[test]
    fn a_deviating_machine_gets_the_value_again_then_the_person_is_asked() {
        let mut s = with_ai(0.0);
        s.request(&req(Actuator::Burner, 55.0, CommandSource::Ai), 1.0);
        s.request(&req(Actuator::Air, 30.0, CommandSource::Ai), 1.0);
        let fell_back = BTreeMap::from([(Actuator::Burner, 0.0), (Actuator::Air, 100.0)]);
        // A few seconds are the machine's to arrive.
        assert!(s.follow(&fell_back, 2.0).is_none());
        assert!(s.follow(&fell_back, 7.0).is_none());
        let r = s.follow(&fell_back, 8.0).unwrap();
        assert_eq!(r.kind, ReactionKind::ActuatorMismatch);
        assert_eq!(
            r.commands,
            vec![
                ActuatorCommand {
                    actuator: Actuator::Burner,
                    value: 55.0
                },
                ActuatorCommand {
                    actuator: Actuator::Air,
                    value: 30.0
                },
            ]
        );
        assert!(r.message.contains("burner 0 %"), "{}", r.message);
        assert!(s.follow(&fell_back, 10.0).is_none());
        assert_eq!(s.follow(&fell_back, 14.0).unwrap().commands.len(), 2);
        // After two resends: no more commands, the person is asked.
        let r = s.follow(&fell_back, 20.0).unwrap();
        assert!(r.commands.is_empty());
        assert!(r.message.contains("does not follow"), "{}", r.message);
        // The reported value counts as set now; quiet from here.
        assert!(s.follow(&fell_back, 40.0).is_none());
        // Nobody's control ended.
        assert!(s.ai_in_control(40.0));
    }

    #[test]
    fn a_machine_that_follows_or_ramps_is_left_alone() {
        let mut s = with_ai(0.0);
        s.request(&req(Actuator::Burner, 25.0, CommandSource::Human), 1.0);
        for t in 2..30 {
            let r = s.follow(&BTreeMap::from([(Actuator::Burner, 21.7)]), f64::from(t));
            assert!(r.is_none(), "at {t}");
        }
        // A new value starts the clock again.
        s.request(&req(Actuator::Burner, 80.0, CommandSource::Human), 30.0);
        assert!(
            s.follow(&BTreeMap::from([(Actuator::Burner, 40.0)]), 31.0)
                .is_none()
        );
        assert!(
            s.follow(&BTreeMap::from([(Actuator::Burner, 78.0)]), 35.0)
                .is_none()
        );
        assert!(
            s.follow(&BTreeMap::from([(Actuator::Burner, 78.0)]), 60.0)
                .is_none()
        );
    }

    #[test]
    fn heat_is_not_sent_again_over_an_emergency_stop() {
        let mut s = with_ai(0.0);
        s.request(&req(Actuator::Burner, 55.0, CommandSource::Ai), 1.0);
        s.emergency_stop("test");
        // The machine keeps heating after the stop: burner 0 is sent again.
        let heating = BTreeMap::from([(Actuator::Burner, 55.0), (Actuator::Air, 100.0)]);
        s.follow(&heating, 2.0);
        let r = s.follow(&heating, 9.0).unwrap();
        assert_eq!(
            r.commands,
            vec![ActuatorCommand {
                actuator: Actuator::Burner,
                value: 0.0
            }]
        );
        // A machine colder than set during the stop is never pushed up.
        s.reset_emergency_stop();
        s.emergency_stop("again");
        let mut c = s.clone();
        c.setpoint.insert(Actuator::Burner, 30.0);
        let cold = BTreeMap::from([(Actuator::Burner, 0.0)]);
        c.follow(&cold, 20.0);
        let r = c.follow(&cold, 27.0).unwrap();
        assert!(r.commands.is_empty());
    }

    #[test]
    fn the_preheat_gets_the_longer_presence_interval_and_nobody_else() {
        let mut s = connected();
        s.set_preheat_holder(Some("preheat"));
        s.confirm_presence(0.0);
        s.acquire_lease("preheat", 0.0).unwrap();
        assert_eq!(s.status(0.0).presence_due_in_s, Some(600.0));
        // Renewed by its own activity, the preheat runs past 3.5 min.
        for t in [50.0, 100.0, 150.0, 200.0, 250.0, 300.0] {
            s.acquire_lease("preheat", t).unwrap();
            assert!(s.tick(t, None).is_empty(), "at {t}");
        }
        // An AI that takes over needs a confirmation within the normal interval.
        assert_eq!(
            s.acquire_lease("claude", 300.0).unwrap_err(),
            RejectReason::PresenceRequired
        );
        // And the preheat itself still misses it after 10 min + grace: an
        // alarm, and the preheat holds on.
        s.acquire_lease("preheat", 600.0).unwrap();
        let r = s.tick(631.0, None);
        assert_eq!(r[0].kind, ReactionKind::PresenceMissed);
        assert!(r[0].commands.is_empty());
        assert_eq!(s.status(631.0).lease_holder.as_deref(), Some("preheat"));
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
    fn connecting_sets_the_safe_state() {
        let mut s = Safety::new(cfg());
        // The machine reports its own start values (Cubean+: burner 100 %).
        s.observe(&BTreeMap::from([
            (Actuator::Burner, 100.0),
            (Actuator::Air, 20.0),
        ]));
        let r = s.set_connected(true).unwrap();
        assert_eq!(r.kind, ReactionKind::Connected);
        assert!(
            r.commands
                .iter()
                .any(|c| c.actuator == Actuator::Burner && c.value == 0.0)
        );
        assert_eq!(s.status(0.0).actuators[&Actuator::Burner], 0.0);
        // Only the transition acts; staying connected does not.
        assert!(s.set_connected(true).is_none());
        s.set_connected(false);
        assert_eq!(
            s.set_connected(true).map(|r| r.kind),
            Some(ReactionKind::Connected)
        );
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
