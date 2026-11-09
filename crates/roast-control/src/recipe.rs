//! The recipe runner (ADR 0019): follows a profile's recipe through a roast.
//!
//! It tracks every step of the plan's recipe from CHARGE: when its trigger
//! comes (a roast time, a bean temperature after the turning point, or a time
//! after dry end or first crack) it becomes due, and once the machine has its
//! value it is done. While an AI holds the lease the daemon sets due steps
//! with source `Autopilot`, through the safety layer; without one they are
//! cues for the person, as before. The AI adjusts the remaining steps for this
//! roast only: move, change, skip.
//!
//! Like the preheat controller it has no clock and does no I/O: the daemon
//! passes roast times (seconds since CHARGE) and the actuator values.

use std::collections::BTreeMap;
use std::fmt;

use roast_core::{
    ActionTrigger, Actuator, ActuatorValues, EventKind, MAX_EVENT_OFFSET_S, PlannedAction,
    RecipeAnchor,
};
use serde::{Deserialize, Serialize};

/// A due step that nobody set stays due this long, then counts as passed, s.
/// A step the runner started to set is followed until the machine has it.
pub const RECIPE_GRACE_S: f64 = 30.0;
/// A value this close to the step's counts as set, percentage points.
const SET_TOLERANCE: f64 = 1.0;
/// Without a detected turning point, a temperature step may fire from this
/// roast time on, s (the turning point comes within the first 3 min).
const BT_STEP_FALLBACK_S: f64 = 180.0;
/// Roast time a step may be moved to at most, s.
const MAX_STEP_TIME_S: f64 = 1800.0;

/// Where a step is in the roast.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StepState {
    /// Its trigger has not come yet.
    Waiting,
    /// Its trigger came and the machine does not have the value yet.
    Due,
    /// The machine has the value (set by the runner, the AI or the person).
    Done,
    /// It was due longer than the grace time and nobody set it.
    Passed,
    /// Skipped for this roast.
    Skipped,
    /// A later step or a direct command on the same actuator took its place.
    Replaced,
}

impl StepState {
    /// Whether the step still has something to do.
    #[must_use]
    pub fn open(self) -> bool {
        matches!(self, StepState::Waiting | StepState::Due)
    }
}

/// One step of the recipe in this roast.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RunStep {
    /// Its position in the plan's recipe; the AI names steps by it.
    pub step: usize,
    /// The step as it runs in this roast, adjustments included.
    pub action: PlannedAction,
    /// The step as the plan has it.
    pub planned: PlannedAction,
    pub state: StepState,
    /// Roast time it became due, s since CHARGE.
    pub due_at: Option<f64>,
    /// The runner has sent it to the machine.
    pub sent: bool,
    /// Roast time before which the runner does not try again after a
    /// refusal, s since CHARGE.
    #[serde(skip)]
    retry_after: Option<f64>,
}

impl RunStep {
    /// Whether this roast changed it (moved, new value, skipped).
    #[must_use]
    pub fn adjusted(&self) -> bool {
        self.action != self.planned || self.state == StepState::Skipped
    }
}

/// What the runner looks at, once per reading.
#[derive(Debug, Clone, Copy)]
pub struct RecipeInput<'a> {
    /// Seconds since CHARGE.
    pub since_charge: f64,
    /// Latest bean temperature, °C.
    pub bt: Option<f64>,
    /// The roast's events (marked or detected), `t` in seconds since CHARGE.
    pub events: &'a [(EventKind, f64)],
    /// The actuator values the machine has.
    pub current: &'a ActuatorValues,
}

/// Where the roast expects what has not happened yet, for the times of
/// steps that are not due: from the projection or the plan.
pub struct Outlook<'a> {
    /// Expected dry end, s since CHARGE.
    pub dry_end: Option<f64>,
    /// Expected first crack start, s since CHARGE.
    pub first_crack: Option<f64>,
    /// When the bean temperature is expected to reach a value after the
    /// turning point, s since CHARGE.
    pub reaches_bt: &'a dyn Fn(f64) -> Option<f64>,
}

/// A change to one step for this roast.
#[derive(Debug, Clone, Copy, Default, PartialEq, Deserialize)]
pub struct StepChange {
    /// The step's position in the plan's recipe.
    pub step: usize,
    /// Move it by this many seconds (later is positive).
    #[serde(default)]
    pub shift_s: Option<f64>,
    /// A new value, percent.
    #[serde(default)]
    pub value: Option<f64>,
    /// Skip it (true) or take it back (false).
    #[serde(default)]
    pub skip: Option<bool>,
}

/// Why a change was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecipeError(pub String);

impl fmt::Display for RecipeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for RecipeError {}

/// A step as the app and the AI see it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct StepView {
    pub step: usize,
    pub actuator: Actuator,
    pub value: f64,
    /// The plan's value, when this roast changed it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub planned_value: Option<f64>,
    pub trigger: ActionTrigger,
    /// "1:30", "FC +0:30", "185 °C".
    pub label: String,
    /// When it is due or became due, s since CHARGE; `None` if not known.
    pub at_s: Option<f64>,
    /// `at_s` is an estimate (the event or temperature has not come yet).
    pub projected: bool,
    pub state: StepState,
    pub adjusted: bool,
}

/// The recipe of one roast.
#[derive(Debug, Clone, PartialEq)]
pub struct RecipeRun {
    steps: Vec<RunStep>,
}

impl RecipeRun {
    /// A run of `actions`, nothing due yet.
    #[must_use]
    pub fn new(actions: &[PlannedAction]) -> Self {
        RecipeRun {
            steps: actions
                .iter()
                .enumerate()
                .map(|(step, a)| RunStep {
                    step,
                    action: *a,
                    planned: *a,
                    state: StepState::Waiting,
                    due_at: None,
                    sent: false,
                    retry_after: None,
                })
                .collect(),
        }
    }

    /// The steps in plan order.
    #[must_use]
    pub fn steps(&self) -> &[RunStep] {
        &self.steps
    }

    /// Whether any step was changed for this roast.
    #[must_use]
    pub fn adjusted(&self) -> bool {
        self.steps.iter().any(RunStep::adjusted)
    }

    /// Takes in a reading: steps whose trigger came become due, a later due
    /// step replaces an earlier one on the same actuator, steps the machine
    /// has are done, and unset steps pass after the grace time.
    pub fn update(&mut self, input: &RecipeInput<'_>) {
        let since = input.since_charge;
        let event_t = |kind: EventKind| {
            input
                .events
                .iter()
                .filter(|(k, _)| *k == kind)
                .map(|&(_, t)| t)
                .next_back()
        };
        let after_tp = event_t(EventKind::TurningPoint).is_some() || since >= BT_STEP_FALLBACK_S;
        for s in &mut self.steps {
            if s.state != StepState::Waiting {
                continue;
            }
            let due_at = match s.action.trigger {
                ActionTrigger::Time { roast_time_s } => {
                    (since >= roast_time_s).then_some(roast_time_s)
                }
                ActionTrigger::Bt { bt_c } => {
                    (after_tp && input.bt.is_some_and(|bt| bt >= bt_c)).then_some(since)
                }
                ActionTrigger::Event { event, offset_s } => event_t(event.kind())
                    .map(|t| t + offset_s)
                    .filter(|&at| since >= at),
            };
            if let Some(at) = due_at {
                s.state = StepState::Due;
                s.due_at = Some(at);
            }
        }
        // A due step the machine has is done, whoever set it.
        for s in &mut self.steps {
            if s.state == StepState::Due
                && input
                    .current
                    .get(&s.action.actuator)
                    .is_some_and(|v| (v - s.action.value).abs() < SET_TOLERANCE)
            {
                s.state = StepState::Done;
            }
        }
        // Per actuator only the latest due step counts.
        let mut latest: BTreeMap<Actuator, (f64, usize)> = BTreeMap::new();
        for s in &self.steps {
            if let (StepState::Due, Some(at)) = (s.state, s.due_at) {
                let e = latest.entry(s.action.actuator).or_insert((at, s.step));
                if (at, s.step) > *e {
                    *e = (at, s.step);
                }
            }
        }
        for s in &mut self.steps {
            if s.state != StepState::Due {
                continue;
            }
            if latest.get(&s.action.actuator).map(|l| l.1) != Some(s.step) {
                s.state = StepState::Replaced;
            } else if !s.sent && s.due_at.is_some_and(|at| since - at > RECIPE_GRACE_S) {
                s.state = StepState::Passed;
            }
        }
    }

    /// The steps to set now: due, and not waiting after a refusal.
    #[must_use]
    pub fn to_apply(&self, since_charge: f64) -> Vec<&RunStep> {
        self.steps
            .iter()
            .filter(|s| s.state == StepState::Due)
            .filter(|s| s.retry_after.is_none_or(|r| since_charge >= r))
            .collect()
    }

    /// The runner sent `step` and the safety layer applied `applied` over
    /// `previous`. It is done once the machine has the step's value, or when
    /// the value did not move (the safety layer kept it, e.g. at the range's
    /// end), so it is not sent again and again; otherwise the runner follows
    /// it up while the safety layer paces the raise.
    pub fn sent(&mut self, step: usize, previous: Option<f64>, applied: f64) {
        if let Some(s) = self.steps.get_mut(step) {
            s.sent = true;
            s.retry_after = None;
            let reached = (applied - s.action.value).abs() < SET_TOLERANCE;
            let stuck = previous.is_some_and(|p| (p - applied).abs() < 1e-6);
            if (reached || stuck) && s.state == StepState::Due {
                s.state = StepState::Done;
            }
        }
    }

    /// The safety layer refused `step`: try again from `retry_at` on.
    pub fn refused(&mut self, step: usize, retry_at: f64) {
        if let Some(s) = self.steps.get_mut(step) {
            s.retry_after = Some(retry_at);
        }
    }

    /// A direct command on `actuator` takes the place of the step that is due
    /// for it; later steps still come (ADR 0019).
    pub fn overridden(&mut self, actuator: Actuator) {
        for s in &mut self.steps {
            if s.state == StepState::Due && s.action.actuator == actuator {
                s.state = StepState::Replaced;
            }
        }
    }

    /// Changes one step for this roast. Only steps that are still to come
    /// (or were due and nobody set them) can change.
    ///
    /// # Errors
    /// The step does not exist, is done, or the change does not fit it.
    pub fn adjust(&mut self, c: &StepChange) -> Result<(), RecipeError> {
        let err = |m: String| Err(RecipeError(m));
        let Some(s) = self.steps.get_mut(c.step) else {
            return err(format!("there is no step {}", c.step));
        };
        let changes = c.shift_s.is_some() || c.value.is_some();
        match (s.state, c.skip) {
            (StepState::Skipped, Some(false)) => s.state = StepState::Waiting,
            (StepState::Skipped, _) if changes => {
                return err(format!("step {} is skipped; take it back first", c.step));
            }
            (StepState::Done | StepState::Replaced, _) => {
                return err(format!(
                    "step {} is {} already",
                    c.step,
                    state_word(s.state)
                ));
            }
            _ => {}
        }
        if let Some(v) = c.value {
            if !(0.0..=100.0).contains(&v) || !v.is_finite() {
                return err("a value must be 0-100 %".to_owned());
            }
        }
        let mut action = s.action;
        if let Some(dt) = c.shift_s.filter(|d| *d != 0.0) {
            if !dt.is_finite() {
                return err("the shift must be a number of seconds".to_owned());
            }
            action.trigger = match action.trigger {
                ActionTrigger::Time { roast_time_s } => {
                    let t = roast_time_s + dt;
                    if !(0.0..=MAX_STEP_TIME_S).contains(&t) {
                        return err(format!("step {} would move outside 0:00-30:00", c.step));
                    }
                    ActionTrigger::Time { roast_time_s: t }
                }
                ActionTrigger::Event { event, offset_s } => {
                    let o = offset_s + dt;
                    if !(0.0..=MAX_EVENT_OFFSET_S).contains(&o) {
                        return err(format!(
                            "step {} would come before {} or more than 15 min after it",
                            c.step,
                            event.short()
                        ));
                    }
                    ActionTrigger::Event { event, offset_s: o }
                }
                ActionTrigger::Bt { .. } => {
                    return err(format!(
                        "step {} follows a bean temperature, not a time: change its value or skip it",
                        c.step
                    ));
                }
            };
        }
        if let Some(v) = c.value {
            action.value = v;
        }
        if action != s.action {
            s.action = action;
            // Looked at afresh with the next reading.
            if matches!(s.state, StepState::Due | StepState::Passed) {
                s.state = StepState::Waiting;
                s.due_at = None;
                s.sent = false;
                s.retry_after = None;
            }
        }
        if c.skip == Some(true) {
            s.state = StepState::Skipped;
        }
        Ok(())
    }

    /// When a step is due or became due, s since CHARGE, and whether that
    /// is an estimate.
    #[must_use]
    pub fn time_of(
        &self,
        s: &RunStep,
        events: &[(EventKind, f64)],
        outlook: &Outlook<'_>,
    ) -> (Option<f64>, bool) {
        if let Some(at) = s.due_at {
            return (Some(at), false);
        }
        match s.action.trigger {
            ActionTrigger::Time { roast_time_s } => (Some(roast_time_s), false),
            ActionTrigger::Bt { bt_c } => ((outlook.reaches_bt)(bt_c), true),
            ActionTrigger::Event { event, offset_s } => {
                let happened = events
                    .iter()
                    .filter(|(k, _)| *k == event.kind())
                    .map(|&(_, t)| t)
                    .next_back();
                if let Some(t) = happened {
                    return (Some(t + offset_s), false);
                }
                let expected = match event {
                    RecipeAnchor::DryEnd => outlook.dry_end,
                    RecipeAnchor::FirstCrackStart => outlook.first_crack,
                };
                (expected.map(|t| t + offset_s), true)
            }
        }
    }

    /// Every step with its time, for the app and the AI.
    #[must_use]
    pub fn view(&self, events: &[(EventKind, f64)], outlook: &Outlook<'_>) -> Vec<StepView> {
        self.steps
            .iter()
            .map(|s| {
                let (at_s, projected) = self.time_of(s, events, outlook);
                StepView {
                    step: s.step,
                    actuator: s.action.actuator,
                    value: s.action.value,
                    planned_value: (s.action.value != s.planned.value).then_some(s.planned.value),
                    trigger: s.action.trigger,
                    label: trigger_label(&s.action.trigger),
                    at_s,
                    projected,
                    state: s.state,
                    adjusted: s.adjusted(),
                }
            })
            .collect()
    }

    /// The next steps to act on: those due whose value the machine does not
    /// have yet, else the earliest steps still to come (together when they
    /// share a time). Steps whose value the machine already has are left out.
    #[must_use]
    pub fn next(
        &self,
        events: &[(EventKind, f64)],
        outlook: &Outlook<'_>,
        current: &ActuatorValues,
    ) -> Vec<StepView> {
        let pending = |v: &StepView| {
            v.state.open()
                && current
                    .get(&v.actuator)
                    .is_none_or(|c| (c - v.value).abs() >= SET_TOLERANCE)
        };
        let views: Vec<StepView> = self
            .view(events, outlook)
            .into_iter()
            .filter(pending)
            .collect();
        let due: Vec<StepView> = views
            .iter()
            .filter(|v| v.state == StepState::Due)
            .cloned()
            .collect();
        if !due.is_empty() {
            return due;
        }
        let key = |v: &StepView| v.at_s.unwrap_or(f64::INFINITY);
        let Some(first) = views.iter().map(key).min_by(f64::total_cmp) else {
            return Vec::new();
        };
        views
            .into_iter()
            .filter(|v| {
                (key(v) - first).abs() < 0.5 || (first.is_infinite() && key(v).is_infinite())
            })
            .collect()
    }
}

fn state_word(s: StepState) -> &'static str {
    match s {
        StepState::Waiting => "waiting",
        StepState::Due => "due",
        StepState::Done => "done",
        StepState::Passed => "passed",
        StepState::Skipped => "skipped",
        StepState::Replaced => "replaced",
    }
}

/// m:ss
#[must_use]
pub fn clock(s: f64) -> String {
    // Roast times are minutes, not years.
    #[allow(clippy::cast_possible_truncation)]
    let s = s.round().max(0.0) as i64;
    format!("{}:{:02}", s / 60, s % 60)
}

/// A trigger for people: "1:30", "FC +0:30", "185 °C".
#[must_use]
pub fn trigger_label(t: &ActionTrigger) -> String {
    match *t {
        ActionTrigger::Time { roast_time_s } => clock(roast_time_s),
        ActionTrigger::Bt { bt_c } => format!("{bt_c:.0} °C"),
        ActionTrigger::Event { event, offset_s } => {
            format!("{} +{}", event.short(), clock(offset_s))
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    fn at(t: f64, actuator: Actuator, value: f64) -> PlannedAction {
        PlannedAction {
            trigger: ActionTrigger::Time { roast_time_s: t },
            actuator,
            value,
        }
    }

    fn after_fc(offset_s: f64, value: f64) -> PlannedAction {
        PlannedAction {
            trigger: ActionTrigger::Event {
                event: RecipeAnchor::FirstCrackStart,
                offset_s,
            },
            actuator: Actuator::Burner,
            value,
        }
    }

    fn recipe() -> Vec<PlannedAction> {
        vec![
            at(0.0, Actuator::Burner, 85.0),
            at(0.0, Actuator::Air, 20.0),
            at(90.0, Actuator::Burner, 90.0),
            PlannedAction {
                trigger: ActionTrigger::Bt { bt_c: 170.0 },
                actuator: Actuator::Air,
                value: 40.0,
            },
            after_fc(0.0, 60.0),
            after_fc(45.0, 45.0),
        ]
    }

    fn values(burner: f64, air: f64) -> ActuatorValues {
        ActuatorValues::from([(Actuator::Burner, burner), (Actuator::Air, air)])
    }

    fn update(
        run: &mut RecipeRun,
        since: f64,
        bt: f64,
        events: &[(EventKind, f64)],
        cur: &ActuatorValues,
    ) {
        run.update(&RecipeInput {
            since_charge: since,
            bt: Some(bt),
            events,
            current: cur,
        });
    }

    fn state(run: &RecipeRun, step: usize) -> StepState {
        run.steps()[step].state
    }

    fn no_bt(_: f64) -> Option<f64> {
        None
    }

    fn outlook(first_crack: Option<f64>) -> Outlook<'static> {
        Outlook {
            dry_end: None,
            first_crack,
            reaches_bt: &no_bt,
        }
    }

    #[test]
    fn steps_come_due_at_their_time_and_are_done_once_set() {
        let mut run = RecipeRun::new(&recipe());
        let cur = values(36.0, 20.0);
        update(&mut run, 0.5, 190.0, &[], &cur);
        assert_eq!(state(&run, 0), StepState::Due);
        assert_eq!(state(&run, 1), StepState::Done, "air is at 20 already");
        assert_eq!(state(&run, 2), StepState::Waiting);
        let apply: Vec<usize> = run.to_apply(0.5).iter().map(|s| s.step).collect();
        assert_eq!(apply, [0]);
        run.sent(0, Some(36.0), 46.0);
        // Ramping: still due beyond the grace, because the runner is on it.
        update(&mut run, 40.0, 110.0, &[], &values(66.0, 20.0));
        assert_eq!(state(&run, 0), StepState::Due);
        update(&mut run, 50.0, 105.0, &[], &values(85.0, 20.0));
        assert_eq!(state(&run, 0), StepState::Done);
    }

    #[test]
    fn an_unset_step_passes_after_the_grace_and_a_later_one_replaces_it() {
        let mut run = RecipeRun::new(&recipe());
        update(&mut run, 0.0, 190.0, &[], &values(36.0, 20.0));
        update(&mut run, 31.0, 100.0, &[], &values(36.0, 20.0));
        assert_eq!(state(&run, 0), StepState::Passed);
        // An AI taking over at 2:10 does not apply 0:00 or 1:30 late.
        let mut run = RecipeRun::new(&recipe());
        update(&mut run, 130.0, 130.0, &[], &values(80.0, 20.0));
        assert_eq!(state(&run, 0), StepState::Replaced);
        assert_eq!(state(&run, 2), StepState::Passed);
        assert!(run.to_apply(130.0).is_empty());
    }

    #[test]
    fn a_step_after_first_crack_waits_for_it() {
        let mut run = RecipeRun::new(&recipe());
        let cur = values(90.0, 40.0);
        update(
            &mut run,
            500.0,
            190.0,
            &[(EventKind::TurningPoint, 70.0)],
            &cur,
        );
        assert_eq!(state(&run, 4), StepState::Waiting);
        // Not marked yet: the time is the projection's.
        let next = run.next(&[], &outlook(Some(540.0)), &cur);
        assert_eq!(next[0].step, 4);
        assert_eq!((next[0].at_s, next[0].projected), (Some(540.0), true));
        assert_eq!(next[0].label, "FC +0:00");
        let events = [
            (EventKind::TurningPoint, 70.0),
            (EventKind::FirstCrackStart, 552.0),
        ];
        update(&mut run, 553.0, 194.0, &events, &cur);
        assert_eq!(state(&run, 4), StepState::Due);
        assert_eq!(state(&run, 5), StepState::Waiting);
        let next = run.next(&events, &outlook(Some(540.0)), &cur);
        assert_eq!(next[0].at_s, Some(552.0));
        update(&mut run, 597.0, 200.0, &events, &values(60.0, 40.0));
        assert_eq!(state(&run, 4), StepState::Done);
        assert_eq!(state(&run, 5), StepState::Due);
    }

    #[test]
    fn a_temperature_step_needs_the_turning_point() {
        let mut run = RecipeRun::new(&recipe());
        let cur = values(90.0, 20.0);
        // Right after CHARGE the probe still reads the hot drum.
        update(&mut run, 5.0, 185.0, &[], &cur);
        assert_eq!(state(&run, 3), StepState::Waiting);
        update(
            &mut run,
            300.0,
            171.0,
            &[(EventKind::TurningPoint, 70.0)],
            &cur,
        );
        assert_eq!(state(&run, 3), StepState::Due);
    }

    #[test]
    fn a_direct_command_replaces_the_due_step_but_not_later_ones() {
        let mut run = RecipeRun::new(&recipe());
        update(&mut run, 0.0, 190.0, &[], &values(36.0, 20.0));
        run.sent(0, Some(36.0), 46.0);
        run.overridden(Actuator::Burner);
        assert_eq!(state(&run, 0), StepState::Replaced);
        update(&mut run, 91.0, 120.0, &[], &values(70.0, 20.0));
        assert_eq!(state(&run, 2), StepState::Due);
    }

    #[test]
    fn the_ai_moves_changes_and_skips_steps_for_this_roast() {
        let mut run = RecipeRun::new(&recipe());
        let change = |step, shift_s, value, skip| StepChange {
            step,
            shift_s,
            value,
            skip,
        };
        run.adjust(&change(2, Some(15.0), Some(88.0), None))
            .unwrap();
        assert_eq!(
            run.steps()[2].action.trigger,
            ActionTrigger::Time {
                roast_time_s: 105.0
            }
        );
        assert!(run.steps()[2].adjusted() && run.adjusted());
        run.adjust(&change(5, Some(-15.0), None, None)).unwrap();
        assert_eq!(
            run.steps()[5].action.trigger,
            ActionTrigger::Event {
                event: RecipeAnchor::FirstCrackStart,
                offset_s: 30.0
            }
        );
        assert!(run.adjust(&change(4, Some(-10.0), None, None)).is_err());
        assert!(run.adjust(&change(3, Some(10.0), None, None)).is_err());
        assert!(run.adjust(&change(3, None, Some(120.0), None)).is_err());
        assert!(run.adjust(&change(9, None, None, Some(true))).is_err());
        run.adjust(&change(4, None, None, Some(true))).unwrap();
        assert_eq!(state(&run, 4), StepState::Skipped);
        let events = [
            (EventKind::TurningPoint, 70.0),
            (EventKind::FirstCrackStart, 552.0),
        ];
        update(&mut run, 560.0, 195.0, &events, &values(90.0, 40.0));
        assert_eq!(state(&run, 4), StepState::Skipped);
        // A done step stays as it was.
        update(&mut run, 0.0, 190.0, &[], &values(85.0, 20.0));
        assert!(run.adjust(&change(0, None, Some(80.0), None)).is_err());
    }

    #[test]
    fn a_step_kept_at_its_value_counts_as_done() {
        let mut run = RecipeRun::new(&[at(0.0, Actuator::Burner, 100.0)]);
        update(&mut run, 0.0, 190.0, &[], &values(95.0, 20.0));
        run.sent(0, Some(95.0), 95.0);
        assert_eq!(state(&run, 0), StepState::Done);
    }

    #[test]
    fn a_refused_step_waits_before_the_next_try() {
        let mut run = RecipeRun::new(&recipe());
        update(&mut run, 0.0, 190.0, &[], &values(36.0, 20.0));
        run.refused(0, 5.0);
        assert!(run.to_apply(2.0).is_empty());
        assert_eq!(run.to_apply(5.0).len(), 1);
    }

    #[test]
    fn labels_read_like_the_recipe() {
        assert_eq!(
            trigger_label(&ActionTrigger::Time { roast_time_s: 90.0 }),
            "1:30"
        );
        assert_eq!(trigger_label(&after_fc(45.0, 0.0).trigger), "FC +0:45");
        assert_eq!(trigger_label(&ActionTrigger::Bt { bt_c: 185.0 }), "185 °C");
    }
}
