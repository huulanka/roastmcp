//! From pops to cracks: the rate of pops and common sense (ADR 0021 §3–§5).

use std::collections::VecDeque;

use roast_core::EventKind;
use serde::Serialize;

use crate::pops::{Band, Pop, Sensitivity};

/// First crack: this many crack-band pops …
pub fn fc_pops(s: Sensitivity) -> usize {
    match s {
        Sensitivity::Low => 6,
        Sensitivity::Normal => 5,
        Sensitivity::High => 4,
    }
}
/// … within this many seconds.
pub const FC_WINDOW_S: f64 = 20.0;
/// A heard first crack reaches back to single pops before its series that
/// came at most this far apart …
pub const FC_LEAD_GAP_S: f64 = 15.0;
/// … and at most this far before the series, s.
pub const FC_LEAD_MAX_S: f64 = 30.0;
/// The model hears first crack only with this many crack-band pops in the
/// last [`FC_WINDOW_S`]: the sound alone (a voice, the spoon) is not enough.
pub const MODEL_MIN_POPS: usize = 2;
/// First crack is over after this long without a crack-band pop, s.
pub const FC_END_QUIET_S: f64 = 30.0;
/// Second crack: this many high-band pops …
pub const SC_POPS: usize = 8;
/// … within this many seconds.
pub const SC_WINDOW_S: f64 = 10.0;
/// Second crack only this far above first crack's temperature, °C.
pub const SC_ABOVE_FC_C: f64 = 12.0;
/// First crack is listened for this long after CHARGE at the earliest, s:
/// right after CHARGE the probe still reads the preheat's temperature, which
/// lies inside the window, and the pops of loading the beans are many.
pub const FC_MIN_AFTER_CHARGE_S: f64 = 180.0;

/// From when first crack may be heard (recording clock): three minutes after
/// CHARGE, and not before the turning point or dry end when they are known.
/// `None` before CHARGE.
#[must_use]
pub fn listen_from(
    charge: Option<f64>,
    turning_point: Option<f64>,
    dry_end: Option<f64>,
) -> Option<f64> {
    let c = charge?;
    Some(
        (c + FC_MIN_AFTER_CHARGE_S)
            .max(turning_point.unwrap_or(c))
            .max(dry_end.unwrap_or(c)),
    )
}

/// What the roast looks like when pops come in. Times are on the recording
/// clock (seconds since the recording started), like the events'.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RoastContext {
    /// Bean temperature now, °C.
    pub bt: Option<f64>,
    /// Rate of rise now, °C/min, if known: first crack comes while the bean
    /// temperature rises.
    pub ror: Option<f64>,
    /// From when first crack may be heard (see [`listen_from`]); `None`
    /// before CHARGE. Pops before it do not count, and a heard crack is
    /// never placed before it.
    pub listen_from: Option<f64>,
    /// First crack start, by anyone: `(t, bt)`.
    pub first_crack: Option<(f64, Option<f64>)>,
    /// First crack end is marked.
    pub first_crack_end: bool,
    /// Second crack start is marked.
    pub second_crack: bool,
    /// First crack is only believed between these temperatures, °C.
    pub window: (f64, f64),
}

/// A crack the detector heard.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct HeardCrack {
    pub kind: EventKind,
    /// Recording time it is placed at: the first pop of the series (for an
    /// end: the last pop).
    pub t: f64,
    /// Pops in the series that made it.
    pub pops: usize,
}

/// Counts pops and decides when a crack is heard.
#[derive(Debug, Clone)]
pub struct CrackLogic {
    sensitivity: Sensitivity,
    crack: VecDeque<f64>,
    /// Crack-band pops of the last minute, for reaching back.
    earlier: VecDeque<f64>,
    high: VecDeque<f64>,
    /// Crack-band pops since first crack started, and the latest of them …
    since_fc: usize,
    last_fc_pop: Option<f64>,
    /// … counted from this first crack (its recording time): a mark that
    /// moves, or goes and comes back, starts the count afresh.
    counted_from: Option<f64>,
    /// Kinds the person undid, or already heard: not heard again in this roast.
    suppressed: Vec<EventKind>,
    /// First crack start is the model's to hear, not the rate rule's
    /// (ADR 0021 §10).
    model_decides: bool,
}

impl CrackLogic {
    #[must_use]
    pub fn new(sensitivity: Sensitivity) -> Self {
        CrackLogic {
            sensitivity,
            crack: VecDeque::new(),
            earlier: VecDeque::new(),
            high: VecDeque::new(),
            since_fc: 0,
            last_fc_pop: None,
            counted_from: None,
            suppressed: Vec::new(),
            model_decides: false,
        }
    }

    /// Hands first crack start to the model; end and second crack stay
    /// with the rules.
    pub fn set_model_decides(&mut self, yes: bool) {
        self.model_decides = yes;
    }

    /// The model says first crack has been going on since `at` (recording
    /// clock). Heard under the same common sense as the rule: after CHARGE
    /// and dry end, inside the window, once; and only with at least
    /// [`MODEL_MIN_POPS`] crack-band pops in the last 20 s, placed at the
    /// first of them (reaching back like the rule).
    pub fn model_heard(&mut self, at: f64, ctx: &RoastContext) -> Option<HeardCrack> {
        if !self.may_hear_first_crack(ctx) {
            return None;
        }
        let after = ctx.listen_from?;
        let series: Vec<f64> = self.crack.iter().copied().filter(|&t| t >= after).collect();
        if series.len() < MODEL_MIN_POPS || series.last().is_none_or(|&last| last < at) {
            return None;
        }
        let t = self.reach_back(series[0], after);
        self.since_fc = series.len();
        self.last_fc_pop = series.last().copied();
        self.counted_from = Some(t);
        self.suppress(EventKind::FirstCrackStart);
        Some(HeardCrack {
            kind: EventKind::FirstCrackStart,
            t,
            pops: series.len(),
        })
    }

    pub fn set_sensitivity(&mut self, s: Sensitivity) {
        self.sensitivity = s;
    }

    /// The person undid a heard mark: that kind is not heard again.
    pub fn suppress(&mut self, kind: EventKind) {
        if !self.suppressed.contains(&kind) {
            self.suppressed.push(kind);
        }
    }

    /// Crack-band pops within the last 20 s.
    #[must_use]
    pub fn crack_rate(&self) -> usize {
        self.crack.len()
    }

    /// High-band pops within the last 10 s.
    #[must_use]
    pub fn high_rate(&self) -> usize {
        self.high.len()
    }

    /// Takes a pop (its `t` on the recording clock) and says whether a
    /// crack starts with it.
    pub fn pop(&mut self, pop: Pop, ctx: &RoastContext) -> Option<HeardCrack> {
        self.forget(pop.t);
        match pop.band {
            Band::Crack => {
                self.crack.push_back(pop.t);
                self.earlier.push_back(pop.t);
                self.sync_first_crack(ctx);
                if let Some((fc_t, _)) = ctx.first_crack
                    && pop.t >= fc_t
                {
                    self.since_fc += 1;
                    self.last_fc_pop = Some(pop.t);
                }
                self.first_crack(ctx)
            }
            Band::High => {
                self.high.push_back(pop.t);
                self.second_crack(ctx)
            }
        }
    }

    /// Called now and then without a pop: says whether first crack ended.
    pub fn tick(&mut self, now: f64, ctx: &RoastContext) -> Option<HeardCrack> {
        self.forget(now);
        self.sync_first_crack(ctx);
        let last = self.last_fc_pop?;
        if ctx.first_crack.is_none_or(|(fc_t, _)| last < fc_t)
            || ctx.first_crack_end
            || self.suppressed.contains(&EventKind::FirstCrackEnd)
            || self.since_fc < fc_pops(self.sensitivity)
            || now - last < FC_END_QUIET_S
        {
            return None;
        }
        let pops = self.since_fc;
        self.since_fc = 0;
        self.last_fc_pop = None;
        self.suppress(EventKind::FirstCrackEnd);
        Some(HeardCrack {
            kind: EventKind::FirstCrackEnd,
            t: last,
            pops,
        })
    }

    /// Keeps the count of pops since first crack on the first crack that is
    /// marked now. In BRYB-003 a heard first crack before CHARGE was undone
    /// but its pops stayed counted; when the person marked first crack eight
    /// minutes later, its "end" was set at once, before the start.
    fn sync_first_crack(&mut self, ctx: &RoastContext) {
        let fc = ctx.first_crack.map(|(t, _)| t);
        if fc == self.counted_from {
            return;
        }
        self.counted_from = fc;
        self.since_fc = 0;
        self.last_fc_pop = None;
        if let Some(fc) = fc {
            for &p in self.earlier.iter().filter(|&&p| p >= fc) {
                self.since_fc += 1;
                self.last_fc_pop = Some(p);
            }
        }
    }

    fn forget(&mut self, now: f64) {
        while self.crack.front().is_some_and(|&t| now - t > FC_WINDOW_S) {
            self.crack.pop_front();
        }
        while self
            .earlier
            .front()
            .is_some_and(|&t| now - t > FC_WINDOW_S + FC_LEAD_MAX_S + FC_LEAD_GAP_S)
        {
            self.earlier.pop_front();
        }
        while self.high.front().is_some_and(|&t| now - t > SC_WINDOW_S) {
            self.high.pop_front();
        }
    }

    fn may_hear_first_crack(&self, ctx: &RoastContext) -> bool {
        ctx.first_crack.is_none()
            && ctx.listen_from.is_some()
            && ctx.ror.is_none_or(|r| r > 0.0)
            && !self.suppressed.contains(&EventKind::FirstCrackStart)
            && ctx
                .bt
                .is_some_and(|bt| bt >= ctx.window.0 && bt <= ctx.window.1)
    }

    /// The series often begins with a few single pops: reach back to them.
    fn reach_back(&self, first: f64, after: f64) -> f64 {
        let mut t = first;
        for &p in self
            .earlier
            .iter()
            .rev()
            .filter(|&&p| p < first && p >= after)
        {
            if t - p > FC_LEAD_GAP_S || first - p > FC_LEAD_MAX_S {
                break;
            }
            t = p;
        }
        t
    }

    fn first_crack(&mut self, ctx: &RoastContext) -> Option<HeardCrack> {
        if self.model_decides || !self.may_hear_first_crack(ctx) {
            return None;
        }
        let after = ctx.listen_from?;
        let series: Vec<f64> = self.crack.iter().copied().filter(|&t| t >= after).collect();
        if series.len() < fc_pops(self.sensitivity) {
            return None;
        }
        let t = self.reach_back(series[0], after);
        self.since_fc = series.len();
        self.last_fc_pop = series.last().copied();
        self.counted_from = Some(t);
        self.suppress(EventKind::FirstCrackStart);
        Some(HeardCrack {
            kind: EventKind::FirstCrackStart,
            t,
            pops: series.len(),
        })
    }

    fn second_crack(&mut self, ctx: &RoastContext) -> Option<HeardCrack> {
        if ctx.second_crack || self.suppressed.contains(&EventKind::SecondCrackStart) {
            return None;
        }
        let (fc_t, fc_bt) = ctx.first_crack?;
        let bt = ctx.bt?;
        let floor = fc_bt.unwrap_or(ctx.window.0) + SC_ABOVE_FC_C;
        if bt < floor {
            return None;
        }
        let series: Vec<f64> = self.high.iter().copied().filter(|&t| t > fc_t).collect();
        if series.len() < SC_POPS {
            return None;
        }
        self.suppress(EventKind::SecondCrackStart);
        Some(HeardCrack {
            kind: EventKind::SecondCrackStart,
            t: series[0],
            pops: series.len(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx(bt: f64) -> RoastContext {
        RoastContext {
            bt: Some(bt),
            ror: Some(8.0),
            listen_from: Some(250.0),
            first_crack: None,
            first_crack_end: false,
            second_crack: false,
            window: (170.0, 205.0),
        }
    }

    fn pop(t: f64, band: Band) -> Pop {
        Pop {
            t,
            band,
            strength_db: 6.0,
        }
    }

    #[test]
    fn five_pops_in_20_s_inside_the_window_are_first_crack_at_the_first_pop() {
        let mut l = CrackLogic::new(Sensitivity::Normal);
        let c = ctx(194.0);
        let mut heard = None;
        for t in [460.0, 466.0, 470.0, 473.0, 476.0] {
            heard = l.pop(pop(t, Band::Crack), &c).or(heard);
        }
        let h = heard.unwrap_or_else(|| panic!("not heard"));
        assert_eq!(h.kind, EventKind::FirstCrackStart);
        assert_eq!(h.t, 460.0);
        assert_eq!(h.pops, 5);
    }

    #[test]
    fn single_pops_just_before_the_series_belong_to_it() {
        let mut l = CrackLogic::new(Sensitivity::Normal);
        let c = ctx(194.0);
        let mut heard = None;
        // A lone pop long before, two up to 15 s apart before the series.
        for t in [400.0, 444.0, 459.0, 470.0, 472.0, 474.0, 476.0, 478.0] {
            heard = l.pop(pop(t, Band::Crack), &c).or(heard);
        }
        assert_eq!(heard.map(|h| h.t), Some(444.0));
    }

    #[test]
    fn sparse_pops_are_not_first_crack() {
        let mut l = CrackLogic::new(Sensitivity::Normal);
        let c = ctx(194.0);
        for i in 0..20 {
            assert!(
                l.pop(pop(400.0 + f64::from(i) * 6.0, Band::Crack), &c)
                    .is_none()
            );
        }
    }

    #[test]
    fn outside_the_window_before_it_may_listen_or_while_falling_nothing_is_heard() {
        let pops = [460.0, 462.0, 464.0, 466.0, 468.0, 470.0];
        let run = |c: RoastContext| {
            let mut l = CrackLogic::new(Sensitivity::Normal);
            pops.iter()
                .filter_map(|&t| l.pop(pop(t, Band::Crack), &c))
                .count()
        };
        assert_eq!(run(ctx(160.0)), 0, "too cold");
        assert_eq!(run(ctx(210.0)), 0, "too hot");
        assert_eq!(
            run(RoastContext {
                listen_from: None,
                ..ctx(190.0)
            }),
            0,
            "before CHARGE"
        );
        assert_eq!(
            run(RoastContext {
                listen_from: Some(465.0),
                ..ctx(190.0)
            }),
            0,
            "only three pops after it may listen"
        );
        assert_eq!(
            run(RoastContext {
                ror: Some(-20.0),
                ..ctx(190.0)
            }),
            0,
            "the bean temperature falls"
        );
        assert_eq!(run(ctx(190.0)), 1);
    }

    #[test]
    fn it_listens_from_three_minutes_after_charge_and_after_turning_point_and_dry_end() {
        assert_eq!(listen_from(None, None, None), None);
        assert_eq!(listen_from(Some(200.0), None, None), Some(380.0));
        assert_eq!(
            listen_from(Some(200.0), Some(270.0), Some(400.0)),
            Some(400.0)
        );
        assert_eq!(
            listen_from(Some(200.0), Some(270.0), Some(330.0)),
            Some(380.0)
        );
    }

    #[test]
    fn the_pops_of_loading_the_beans_are_not_first_crack() {
        // BRYB-003: many pops in the preheat while the beans were weighed and
        // loaded; CHARGE at 204 s with the probe still at 200 °C, inside the
        // window. The rule fired at once and placed first crack 39 s before
        // CHARGE.
        let mut l = CrackLogic::new(Sensitivity::Normal);
        let charge = 204.0;
        let c = RoastContext {
            listen_from: listen_from(Some(charge), None, None),
            ror: None,
            ..ctx(200.0)
        };
        let mut heard = Vec::new();
        for i in 0..40 {
            let t = 160.0 + f64::from(i) * 1.5;
            heard.extend(l.pop(pop(t, Band::Crack), &c));
        }
        assert!(heard.is_empty(), "{heard:?}");
        // Pops from before it may listen never start a later series either.
        let later = RoastContext {
            bt: Some(194.0),
            ror: Some(7.0),
            ..c
        };
        for t in [390.0, 392.0, 394.0, 396.0, 398.0] {
            heard.extend(l.pop(pop(t, Band::Crack), &later));
        }
        assert_eq!(heard.iter().map(|h| h.t).collect::<Vec<_>>(), vec![390.0]);
    }

    #[test]
    fn an_undone_first_crack_leaves_no_end_behind_for_the_next_mark() {
        // BRYB-003: the heard first crack was undone, its pops stayed
        // counted, and when the person marked first crack at 717 s the "end"
        // was set at once at 214.5 s.
        let mut l = CrackLogic::new(Sensitivity::Normal);
        let mut c = ctx(194.0);
        for i in 0..8 {
            l.pop(pop(300.0 + f64::from(i), Band::Crack), &c);
        }
        // Undone: the mark is gone and not heard again.
        l.suppress(EventKind::FirstCrackStart);
        c.first_crack = None;
        assert!(l.tick(400.0, &c).is_none());
        // The person marks first crack much later.
        c.first_crack = Some((717.0, Some(194.9)));
        assert!(l.tick(717.5, &c).is_none(), "no end before the start");
        for i in 0..6 {
            l.pop(pop(720.0 + f64::from(i) * 4.0, Band::Crack), &c);
        }
        assert!(l.tick(760.0, &c).is_none());
        let end = l.tick(771.0, &c).unwrap_or_else(|| panic!("no end"));
        assert_eq!(end.t, 740.0);
    }

    #[test]
    fn undo_keeps_it_from_coming_back() {
        let mut l = CrackLogic::new(Sensitivity::Normal);
        l.suppress(EventKind::FirstCrackStart);
        let c = ctx(194.0);
        for i in 0..10 {
            assert!(l.pop(pop(460.0 + f64::from(i), Band::Crack), &c).is_none());
        }
    }

    #[test]
    fn first_crack_ends_after_30_quiet_seconds_at_its_last_pop() {
        let mut l = CrackLogic::new(Sensitivity::Normal);
        let mut c = ctx(194.0);
        for i in 0..5 {
            l.pop(pop(460.0 + f64::from(i), Band::Crack), &c);
        }
        c.first_crack = Some((460.0, Some(194.0)));
        for i in 0..15 {
            l.pop(pop(470.0 + f64::from(i) * 3.0, Band::Crack), &c);
        }
        assert!(l.tick(530.0, &c).is_none());
        let end = l.tick(543.0, &c).unwrap_or_else(|| panic!("no end"));
        assert_eq!(end.kind, EventKind::FirstCrackEnd);
        assert_eq!(end.t, 512.0);
        assert!(l.tick(560.0, &c).is_none(), "once");
    }

    #[test]
    fn the_model_decides_first_crack_under_the_same_common_sense() {
        let mut l = CrackLogic::new(Sensitivity::Normal);
        l.set_model_decides(true);
        let c = ctx(194.0);
        for t in [452.0, 466.0, 470.0, 473.0, 476.0, 478.0] {
            assert!(l.pop(pop(t, Band::Crack), &c).is_none(), "not the rule's");
        }
        assert!(l.model_heard(465.0, &ctx(165.0)).is_none(), "too cold");
        let h = l
            .model_heard(465.0, &c)
            .unwrap_or_else(|| panic!("not heard"));
        // From the first pop after the model's start, reaching back to 452.
        assert_eq!(
            (h.kind, h.t, h.pops),
            (EventKind::FirstCrackStart, 452.0, 5)
        );
        assert!(l.model_heard(470.0, &c).is_none(), "once");
    }

    #[test]
    fn second_crack_needs_a_dense_series_well_above_first_crack() {
        let mut c = ctx(205.0);
        c.first_crack = Some((460.0, Some(195.0)));
        let run = |c: RoastContext| {
            let mut l = CrackLogic::new(Sensitivity::Normal);
            (0..12)
                .filter_map(|i| l.pop(pop(600.0 + f64::from(i) * 0.7, Band::High), &c))
                .next()
        };
        assert!(run(c).is_none(), "205 °C is only 10 °C above first crack");
        let h = run(RoastContext {
            bt: Some(209.0),
            ..c
        })
        .unwrap_or_else(|| panic!("not heard"));
        assert_eq!(h.kind, EventKind::SecondCrackStart);
        assert_eq!(h.t, 600.0);
        assert!(
            run(RoastContext {
                first_crack: None,
                bt: Some(220.0),
                ..c
            })
            .is_none(),
            "not before first crack"
        );
    }
}
