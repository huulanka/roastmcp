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
/// First crack is over after this long without a crack-band pop, s.
pub const FC_END_QUIET_S: f64 = 30.0;
/// Second crack: this many high-band pops …
pub const SC_POPS: usize = 8;
/// … within this many seconds.
pub const SC_WINDOW_S: f64 = 10.0;
/// Second crack only this far above first crack's temperature, °C.
pub const SC_ABOVE_FC_C: f64 = 12.0;

/// What the roast looks like when pops come in. Times are on the recording
/// clock (seconds since the recording started), like the events'.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RoastContext {
    /// Bean temperature now, °C.
    pub bt: Option<f64>,
    /// CHARGE has been marked.
    pub charged: bool,
    /// Dry end, if marked or detected.
    pub dry_end: Option<f64>,
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
    high: VecDeque<f64>,
    /// Crack-band pops since first crack started, and the latest of them.
    since_fc: usize,
    last_fc_pop: Option<f64>,
    /// Kinds the person undid, or already heard: not heard again in this roast.
    suppressed: Vec<EventKind>,
}

impl CrackLogic {
    #[must_use]
    pub fn new(sensitivity: Sensitivity) -> Self {
        CrackLogic {
            sensitivity,
            crack: VecDeque::new(),
            high: VecDeque::new(),
            since_fc: 0,
            last_fc_pop: None,
            suppressed: Vec::new(),
        }
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
        let last = self.last_fc_pop?;
        if ctx.first_crack.is_none()
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

    fn forget(&mut self, now: f64) {
        while self.crack.front().is_some_and(|&t| now - t > FC_WINDOW_S) {
            self.crack.pop_front();
        }
        while self.high.front().is_some_and(|&t| now - t > SC_WINDOW_S) {
            self.high.pop_front();
        }
    }

    fn first_crack(&mut self, ctx: &RoastContext) -> Option<HeardCrack> {
        if ctx.first_crack.is_some()
            || !ctx.charged
            || self.suppressed.contains(&EventKind::FirstCrackStart)
        {
            return None;
        }
        let bt = ctx.bt?;
        if bt < ctx.window.0 || bt > ctx.window.1 {
            return None;
        }
        let after = ctx.dry_end.unwrap_or(f64::NEG_INFINITY);
        let series: Vec<f64> = self.crack.iter().copied().filter(|&t| t >= after).collect();
        if series.len() < fc_pops(self.sensitivity) {
            return None;
        }
        let t = series[0];
        self.since_fc = series.len();
        self.last_fc_pop = series.last().copied();
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
            charged: true,
            dry_end: Some(250.0),
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
    fn outside_the_window_before_charge_or_dry_end_nothing_is_heard() {
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
                charged: false,
                ..ctx(190.0)
            }),
            0
        );
        assert_eq!(
            run(RoastContext {
                dry_end: Some(465.0),
                ..ctx(190.0)
            }),
            0,
            "only three pops after dry end"
        );
        assert_eq!(run(ctx(190.0)), 1);
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
