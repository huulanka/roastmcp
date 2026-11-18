//! The autopilot's fine control between the recipe's steps (ADR 0020).
//!
//! The recipe says when the burner goes where; the trim keeps the bean
//! temperature on the plan's curve around it. It works the way a careful
//! roaster does (step, wait, look): it looks where the bean temperature
//! will be in a minute, and only when that is outside a dead band around
//! the plan - and the roast really is off the plan now, for more than a
//! reading or two - does it move the burner by a few points; then it waits
//! until the sluggish machine has answered before it looks again. It never
//! moves far from the recipe, does not add heat close to first crack, and
//! gives a correction back to the recipe once the roast is on the plan again.
//!
//! It only decides; the daemon sends the step through the safety layer.

use std::collections::VecDeque;

use serde::Serialize;

/// How the trim behaves.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TrimTuning {
    /// Dead band around the plan, °C: a predicted deviation inside it is left
    /// alone.
    pub band_c: f64,
    /// Once out of the band, the prediction counts as back in only below
    /// this, °C (hysteresis).
    pub band_in_c: f64,
    /// How far ahead the deviation from the plan is predicted from the
    /// difference in the rate of rise, s (about the time a burner change
    /// needs to show).
    pub horizon_s: f64,
    /// The rate-of-rise part of the prediction adds at most this, °C: a
    /// noisy rate of rise alone must not push a roast out of the band.
    pub max_ror_term_c: f64,
    /// The deviation now is the median over this many seconds of readings, s:
    /// one stray reading does not count.
    pub deviation_window_s: f64,
    /// Before it adds heat, the roast must be at least this far below the
    /// plan now (the median), °C.
    pub min_deviation_c: f64,
    /// Before it takes heat away, at least this far above, °C: cutting heat
    /// is the cheaper mistake (toward the safe state, and too much energy
    /// late in the roast is the usual one).
    pub min_deviation_cut_c: f64,
    /// Readings in a row the prediction must stay out of the band.
    pub confirm_readings: u32,
    /// Over how long a deviation is to be made up, s.
    pub catch_up_s: f64,
    /// Wait after a correction before the next one, s: until the sluggish
    /// machine has answered the last.
    pub wait_s: f64,
    /// Wait after any other burner change (a recipe step, a command), s.
    /// Short, because the plan's curve expects the recipe's steps.
    pub settle_s: f64,
    /// Largest single correction, burner points.
    pub max_step: f64,
    /// How far all corrections together may move away from the recipe,
    /// burner points.
    pub max_offset: f64,
    /// How far the corrections carry over to the recipe's later steps, burner
    /// points (see [`Trim::carried`]).
    pub max_carry: f64,
    /// No more heat this close before first crack, s: the development runs on
    /// the recipe's steps; the trim may still take heat away.
    pub no_raise_before_fc_s: f64,
    /// The roast counts as back on the plan when the deviation (median) is
    /// within this, °C …
    pub on_plan_c: f64,
    /// … and the rate of rise within this, °C/min …
    pub on_plan_ror: f64,
    /// … for this long, s. Then half the correction goes back to the recipe.
    pub ease_after_s: f64,
    /// No corrections before this roast time, s: the probe first has to cool
    /// down to the beans and turn.
    pub start_s: f64,
    /// The trim never takes the burner below this, %.
    pub min_burner: f64,
    /// How much the rate of rise changes per burner point with beans in the
    /// drum, °C/min per %.
    pub ror_per_pct: f64,
}

impl Default for TrimTuning {
    fn default() -> Self {
        TrimTuning {
            band_c: 3.0,
            band_in_c: 2.0,
            horizon_s: 90.0,
            max_ror_term_c: 2.0,
            deviation_window_s: 15.0,
            min_deviation_c: 1.5,
            min_deviation_cut_c: 1.0,
            confirm_readings: 3,
            catch_up_s: 120.0,
            wait_s: 90.0,
            settle_s: 20.0,
            max_step: 5.0,
            max_offset: 10.0,
            max_carry: 5.0,
            no_raise_before_fc_s: 60.0,
            on_plan_c: 0.5,
            on_plan_ror: 1.0,
            ease_after_s: 60.0,
            start_s: 150.0,
            min_burner: 10.0,
            ror_per_pct: 0.25,
        }
    }
}

/// What the trim looks at, once per reading.
#[derive(Debug, Clone, Copy)]
pub struct TrimInput {
    /// Seconds since CHARGE.
    pub since_charge: f64,
    /// Bean temperature, °C.
    pub bt: f64,
    /// Rate of rise, °C/min.
    pub ror: f64,
    /// The plan's bean temperature at this roast time, °C.
    pub plan_bt: f64,
    /// The plan's rate of rise at this roast time, °C/min.
    pub plan_ror: f64,
    /// The burner the machine has, %.
    pub burner: f64,
    /// Seconds since the burner last changed (by anyone).
    pub since_burner_change: f64,
    /// Seconds since the trim's last correction, if it made one.
    pub since_correction: Option<f64>,
    /// Seconds until first crack: marked (then negative), else expected or
    /// planned; `None` when nothing says.
    pub fc_in_s: Option<f64>,
}

/// Why the trim leaves the burner alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TrimHold {
    /// Too early in the roast.
    Early,
    /// Waiting for the last correction (or burner change) to show.
    Settling,
    /// The bean temperature heads for the plan.
    InBand,
    /// The prediction is out of the band, but the roast is not far enough
    /// off the plan now: the rate of rise alone does not decide.
    SmallDeviation,
    /// Out of the band, not yet for long enough.
    Confirming,
    /// It would add heat, but first crack is near or past.
    NearFirstCrack,
    /// It would correct, but is as far from the recipe as it may go (or at
    /// the end of the burner's range).
    AtLimit,
}

/// What the trim decided.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TrimDecision {
    /// Leave the burner alone.
    Hold(TrimHold),
    /// Set the burner to `burner`.
    Step {
        /// The new burner value, %.
        burner: f64,
        /// The change, burner points.
        delta: f64,
        /// Where the bean temperature heads relative to the plan, °C
        /// (positive: above).
        predicted_error_c: f64,
    },
    /// The roast is back on the plan: give `delta` points of the corrections
    /// back to the recipe's later steps. Nothing is sent now.
    Ease {
        /// The change of the offset, points (toward 0).
        delta: f64,
    },
}

/// The numbers behind one decision, for the log.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct TrimWhy {
    /// Bean temperature minus plan at this reading, °C.
    pub deviation_c: f64,
    /// The median of that over the deviation window, °C.
    pub deviation_median_c: f64,
    /// Rate of rise and the plan's, °C/min.
    pub ror: f64,
    pub plan_ror: f64,
    /// Where the bean temperature heads relative to the plan, °C.
    pub predicted_c: f64,
    /// Corrections so far, points away from the recipe.
    pub offset: f64,
}

/// The trim of one roast.
#[derive(Debug, Clone, PartialEq)]
pub struct Trim {
    tuning: TrimTuning,
    /// All corrections so far, burner points away from the recipe.
    offset: f64,
    /// Recent deviations from the plan: (roast time, °C).
    deviations: VecDeque<(f64, f64)>,
    /// Readings in a row out of the band, and on which side.
    out_count: u32,
    out_side: f64,
    /// Since when the roast is on the plan, s since CHARGE.
    on_plan_since: Option<f64>,
    /// The numbers of the last decision.
    why: Option<TrimWhy>,
}

impl Trim {
    /// A fresh trim.
    #[must_use]
    pub fn new(tuning: TrimTuning) -> Self {
        Trim::resumed(tuning, 0.0)
    }

    /// A trim that takes up a roast it already corrected by `offset` points:
    /// those corrections still count toward how far it may go.
    #[must_use]
    pub fn resumed(tuning: TrimTuning, offset: f64) -> Self {
        Trim {
            tuning,
            offset,
            deviations: VecDeque::new(),
            out_count: 0,
            out_side: 0.0,
            on_plan_since: None,
            why: None,
        }
    }

    /// Its tuning.
    #[must_use]
    pub fn tuning(&self) -> &TrimTuning {
        &self.tuning
    }

    /// How far the corrections so far moved away from the recipe, points.
    #[must_use]
    pub fn offset(&self) -> f64 {
        self.offset
    }

    /// How far the recipe's later steps (before the development) are moved:
    /// the corrections, at most [`TrimTuning::max_carry`].
    #[must_use]
    pub fn carried(&self) -> f64 {
        self.offset
            .clamp(-self.tuning.max_carry, self.tuning.max_carry)
    }

    /// The numbers behind the last decision.
    #[must_use]
    pub fn why(&self) -> Option<TrimWhy> {
        self.why
    }

    /// Where the bean temperature heads relative to the plan, °C: how far
    /// off it is now, plus what the difference in the rate of rise adds over
    /// the horizon (at most [`TrimTuning::max_ror_term_c`]).
    #[must_use]
    pub fn predicted_error(&self, deviation: f64, input: &TrimInput) -> f64 {
        let t = &self.tuning;
        let ror_term = ((input.ror - input.plan_ror) * t.horizon_s / 60.0)
            .clamp(-t.max_ror_term_c, t.max_ror_term_c);
        deviation + ror_term
    }

    fn median_deviation(&mut self, input: &TrimInput) -> f64 {
        let now = input.since_charge;
        self.deviations.push_back((now, input.bt - input.plan_bt));
        while self
            .deviations
            .front()
            .is_some_and(|&(t, _)| t < now - self.tuning.deviation_window_s)
        {
            self.deviations.pop_front();
        }
        let mut v: Vec<f64> = self.deviations.iter().map(|&(_, d)| d).collect();
        v.sort_by(f64::total_cmp);
        let n = v.len();
        if n % 2 == 1 {
            v[n / 2]
        } else {
            f64::midpoint(v[n / 2 - 1], v[n / 2])
        }
    }

    /// Decides for this reading; call it once per reading. A
    /// [`TrimDecision::Step`] or [`TrimDecision::Ease`] counts only once
    /// [`Trim::applied`] reports it.
    pub fn decide(&mut self, input: &TrimInput) -> TrimDecision {
        let t = self.tuning;
        let median = self.median_deviation(input);
        let error = self.predicted_error(median, input);
        self.why = Some(TrimWhy {
            deviation_c: input.bt - input.plan_bt,
            deviation_median_c: median,
            ror: input.ror,
            plan_ror: input.plan_ror,
            predicted_c: error,
            offset: self.offset,
        });

        // Out of the band for a few readings in a row, on one side; back in
        // only well inside it.
        let side = error.signum();
        if error.abs() >= t.band_c && (self.out_count == 0 || side == self.out_side) {
            self.out_count += 1;
            self.out_side = side;
        } else if error.abs() < t.band_in_c || side != self.out_side {
            self.out_count = 0;
        }
        // Back on the plan: the correction goes back to the recipe, half at
        // a time.
        let on_plan =
            median.abs() <= t.on_plan_c && (input.ror - input.plan_ror).abs() <= t.on_plan_ror;
        if !on_plan {
            self.on_plan_since = None;
        } else if self.on_plan_since.is_none() {
            self.on_plan_since = Some(input.since_charge);
        }

        if input.since_charge < t.start_s {
            return TrimDecision::Hold(TrimHold::Early);
        }
        if input.since_burner_change < t.settle_s
            || input.since_correction.is_some_and(|s| s < t.wait_s)
        {
            return TrimDecision::Hold(TrimHold::Settling);
        }
        if self.offset.abs() >= 1.0
            && self
                .on_plan_since
                .is_some_and(|since| input.since_charge - since >= t.ease_after_s)
        {
            let half = (self.offset / 2.0).round();
            let delta = if half.abs() < 1.0 {
                -self.offset
            } else {
                -half
            };
            return TrimDecision::Ease { delta };
        }
        if error.abs() < t.band_c {
            return TrimDecision::Hold(TrimHold::InBand);
        }
        let min_deviation = if side > 0.0 {
            t.min_deviation_cut_c
        } else {
            t.min_deviation_c
        };
        if median.abs() < min_deviation || median.signum() != side {
            return TrimDecision::Hold(TrimHold::SmallDeviation);
        }
        if self.out_count < t.confirm_readings {
            return TrimDecision::Hold(TrimHold::Confirming);
        }
        // Make the deviation up over the catch-up time: that needs this much
        // more (or less) rate of rise, and so many burner points.
        let ror_change = -error / (t.catch_up_s / 60.0);
        let wanted = (ror_change / t.ror_per_pct).clamp(-t.max_step, t.max_step);
        if wanted > 0.0 && input.fc_in_s.is_some_and(|s| s < t.no_raise_before_fc_s) {
            return TrimDecision::Hold(TrimHold::NearFirstCrack);
        }
        let within_offset = wanted.clamp(-t.max_offset - self.offset, t.max_offset - self.offset);
        let burner = (input.burner + within_offset)
            .round()
            .clamp(t.min_burner.min(input.burner), 100.0);
        let delta = burner - input.burner;
        if delta.abs() < 1.0 {
            return TrimDecision::Hold(TrimHold::AtLimit);
        }
        TrimDecision::Step {
            burner,
            delta,
            predicted_error_c: error,
        }
    }

    /// The machine took a correction of `delta` points, or the recipe took
    /// back an eased one.
    pub fn applied(&mut self, delta: f64) {
        self.offset += delta;
        self.out_count = 0;
        self.on_plan_since = None;
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use roast_core::{Actuator, EventKind};
    use roast_driver::{RoasterDriver, SimulatedMachine};

    /// A reading `bt_off` °C and `ror_off` °C/min away from the plan.
    fn input(bt_off: f64, ror_off: f64) -> TrimInput {
        TrimInput {
            since_charge: 300.0,
            bt: 150.0 + bt_off,
            ror: 10.0 + ror_off,
            plan_bt: 150.0,
            plan_ror: 10.0,
            burner: 80.0,
            since_burner_change: 200.0,
            since_correction: None,
            fc_in_s: None,
        }
    }

    /// A fresh trim's decision after the same reading came `n` times, 1.5 s
    /// apart.
    fn after(n: usize, input: &TrimInput) -> TrimDecision {
        after_with(&mut Trim::new(TrimTuning::default()), n, input)
    }

    fn after_with(trim: &mut Trim, n: usize, input: &TrimInput) -> TrimDecision {
        let mut d = TrimDecision::Hold(TrimHold::Early);
        for i in 0..n {
            d = trim.decide(&TrimInput {
                since_charge: input.since_charge + i as f64 * 1.5,
                ..*input
            });
        }
        d
    }

    #[test]
    fn it_leaves_the_burner_alone_inside_the_band_and_while_waiting() {
        // 2 °C above the plan and rising like it: inside the band.
        assert_eq!(
            after(3, &input(2.0, 0.0)),
            TrimDecision::Hold(TrimHold::InBand)
        );
        // 4 °C below now, but catching up by itself: left alone.
        assert_eq!(
            after(3, &input(-4.0, 1.0)),
            TrimDecision::Hold(TrimHold::InBand)
        );
        // On the plan now and rising too slowly: the rate of rise alone adds
        // at most 2 °C, so that waits until the roast really falls behind.
        assert_eq!(
            after(3, &input(0.0, -2.5)),
            TrimDecision::Hold(TrimHold::InBand)
        );
        assert!(matches!(
            after(3, &input(-2.0, -2.5)),
            TrimDecision::Step { .. }
        ));
        let off = input(-4.0, -1.0);
        // Out of the band for one or two readings is not enough.
        assert_eq!(after(1, &off), TrimDecision::Hold(TrimHold::Confirming));
        assert_eq!(after(2, &off), TrimDecision::Hold(TrimHold::Confirming));
        assert!(matches!(after(3, &off), TrimDecision::Step { .. }));
        assert_eq!(
            after(
                3,
                &TrimInput {
                    since_charge: 100.0,
                    ..off
                }
            ),
            TrimDecision::Hold(TrimHold::Early)
        );
        // Right after a recipe step, and until its own last correction shows.
        assert_eq!(
            after(
                3,
                &TrimInput {
                    since_burner_change: 10.0,
                    ..off
                }
            ),
            TrimDecision::Hold(TrimHold::Settling)
        );
        assert_eq!(
            after(
                3,
                &TrimInput {
                    since_correction: Some(60.0),
                    ..off
                }
            ),
            TrimDecision::Hold(TrimHold::Settling)
        );
        assert!(matches!(
            after(
                3,
                &TrimInput {
                    since_correction: Some(95.0),
                    ..off
                }
            ),
            TrimDecision::Step { .. }
        ));
    }

    #[test]
    fn a_correction_is_a_few_points_and_stays_near_the_recipe() {
        let mut trim = Trim::new(TrimTuning::default());
        // Heading well below the plan: more heat, but only one step's worth.
        let TrimDecision::Step {
            burner,
            delta,
            predicted_error_c,
        } = after_with(&mut trim, 3, &input(-4.0, -2.0))
        else {
            panic!("no step");
        };
        assert_eq!((burner, delta), (85.0, 5.0));
        assert!(
            (predicted_error_c + 6.0).abs() < 1e-9,
            "{predicted_error_c}"
        );
        trim.applied(5.0);
        trim.applied(5.0);
        // Ten points from the recipe: no further, and only five carry over.
        assert_eq!(
            after_with(&mut trim, 3, &input(-4.0, -2.0)),
            TrimDecision::Hold(TrimHold::AtLimit)
        );
        assert_eq!(trim.carried(), 5.0);
        // Switched off and on again in the same roast: still no further.
        let mut again = Trim::resumed(TrimTuning::default(), trim.offset());
        assert_eq!(
            after_with(&mut again, 3, &input(-4.0, -2.0)),
            TrimDecision::Hold(TrimHold::AtLimit)
        );
        // The other way is still open.
        let TrimDecision::Step { delta, .. } = after(3, &input(4.0, 0.0)) else {
            panic!("no step");
        };
        assert_eq!(delta, -5.0);
        // Never below the lowest burner.
        let TrimDecision::Step { delta, .. } = after(
            3,
            &TrimInput {
                burner: 12.0,
                ..input(4.0, 0.0)
            },
        ) else {
            panic!("no step");
        };
        assert_eq!(delta, -2.0);
    }

    /// BRYB-003 (2026-11-18), 2:51-4:00 after CHARGE: roast time, bean
    /// temperature, the plan's, the rate of rise (20 s) and the plan's. The
    /// roast ran a steady 1 °C below the plan; one reading at 3:18 fell to
    /// 1.9 °C below and pulled the rate of rise down with it.
    const STRAY: [(f64, f64, f64, f64, f64); 44] = [
        (170.9, 128.7, 129.8, 18.7, 18.8),
        (173.9, 129.4, 130.7, 18.1, 18.8),
        (175.5, 130.2, 131.1, 18.3, 18.8),
        (177.0, 130.6, 131.6, 18.5, 18.8),
        (178.5, 131.1, 132.0, 18.7, 18.8),
        (180.0, 131.2, 132.4, 18.3, 18.7),
        (181.5, 131.9, 132.8, 18.2, 18.6),
        (183.0, 132.2, 133.3, 18.1, 18.4),
        (184.5, 132.5, 133.7, 17.8, 18.1),
        (186.0, 132.9, 134.2, 17.1, 17.9),
        (187.5, 133.6, 134.6, 17.3, 17.7),
        (189.0, 133.9, 134.9, 17.2, 17.5),
        (190.5, 134.4, 135.3, 17.2, 17.3),
        (192.0, 134.6, 135.8, 16.9, 17.1),
        (193.5, 135.1, 136.2, 16.8, 17.0),
        (195.0, 135.4, 136.6, 16.3, 17.0),
        (196.5, 135.7, 137.0, 16.1, 16.9),
        (198.0, 135.6, 137.5, 15.4, 16.8),
        (199.5, 136.9, 137.9, 15.9, 16.7),
        (201.0, 137.1, 138.2, 15.8, 16.7),
        (204.0, 138.0, 139.1, 16.1, 16.5),
        (205.5, 138.4, 139.5, 16.2, 16.4),
        (207.0, 138.7, 139.9, 16.0, 16.4),
        (208.5, 139.4, 140.3, 16.6, 16.4),
        (209.9, 139.7, 140.7, 16.9, 16.3),
        (211.4, 140.1, 141.0, 17.4, 16.2),
        (212.9, 140.5, 141.4, 17.5, 16.2),
        (214.5, 140.7, 141.8, 17.4, 16.1),
        (216.0, 141.3, 142.2, 17.5, 15.9),
        (217.5, 141.7, 142.6, 17.3, 15.8),
        (219.0, 142.0, 142.9, 16.2, 15.8),
        (220.5, 142.4, 143.3, 16.2, 15.7),
        (222.0, 142.7, 143.7, 15.8, 15.6),
        (223.4, 143.1, 144.1, 15.7, 15.5),
        (225.0, 143.4, 144.6, 15.4, 15.4),
        (226.5, 143.9, 144.9, 15.3, 15.4),
        (228.0, 144.1, 145.3, 14.8, 15.3),
        (229.5, 144.5, 145.7, 14.7, 15.2),
        (230.9, 144.9, 146.0, 14.7, 15.2),
        (234.0, 145.4, 146.7, 14.3, 15.1),
        (235.4, 146.3, 147.1, 14.5, 15.0),
        (236.9, 146.4, 147.4, 14.6, 14.8),
        (238.4, 146.8, 147.7, 14.8, 14.6),
        (239.9, 147.0, 148.0, 14.7, 14.4),
    ];

    /// BRYB-003, 8:36-9:05: after first crack the roast ran 1-1.5 °C above
    /// the plan and rose 2 °C/min faster.
    const HOT: [(f64, f64, f64, f64, f64); 19] = [
        (516.0, 195.2, 194.5, 6.8, 5.8),
        (517.5, 195.5, 194.6, 6.8, 5.6),
        (518.9, 195.7, 194.8, 6.8, 5.4),
        (520.4, 195.7, 194.9, 6.6, 5.2),
        (521.9, 196.1, 195.1, 7.0, 5.0),
        (523.5, 196.3, 195.2, 7.2, 4.9),
        (525.0, 196.4, 195.3, 7.2, 4.8),
        (526.5, 196.4, 195.4, 7.1, 4.8),
        (528.0, 196.5, 195.5, 7.2, 4.8),
        (529.5, 196.8, 195.6, 7.2, 4.9),
        (531.0, 197.0, 195.7, 7.1, 4.9),
        (534.0, 197.3, 196.0, 7.1, 4.8),
        (535.5, 197.5, 196.1, 6.7, 4.7),
        (537.0, 197.7, 196.2, 6.6, 4.7),
        (538.5, 197.6, 196.3, 6.3, 4.6),
        (540.0, 197.9, 196.4, 6.3, 4.4),
        (541.5, 198.1, 196.5, 6.2, 4.4),
        (543.0, 198.2, 196.7, 6.2, 4.4),
        (544.5, 198.2, 196.8, 6.2, 4.4),
    ];

    fn replay(readings: &[(f64, f64, f64, f64, f64)], fc_at: Option<f64>) -> Vec<TrimDecision> {
        let mut trim = Trim::new(TrimTuning::default());
        readings
            .iter()
            .map(|&(t, bt, plan_bt, ror, plan_ror)| {
                trim.decide(&TrimInput {
                    since_charge: t,
                    bt,
                    ror,
                    plan_bt,
                    plan_ror,
                    burner: 85.0,
                    since_burner_change: 100.0,
                    since_correction: None,
                    fc_in_s: fc_at.map(|fc| fc - t),
                })
            })
            .collect()
    }

    #[test]
    fn one_stray_reading_does_not_start_a_correction() {
        // The rule before: deviation now plus the rate-of-rise difference
        // over 90 s, 3 °C band. At 3:18 that said 4 °C below and added 5
        // points of burner for the rest of the roast.
        let (_, bt, plan_bt, ror, plan_ror) = STRAY[17];
        let old = (bt - plan_bt) + (ror - plan_ror) * 1.5;
        assert!(old < -3.9, "{old}");
        // Now: the median over 15 s is about 1 °C below and one reading is
        // not a trend.
        let decisions = replay(&STRAY, Some(505.0));
        assert!(
            decisions.iter().all(|d| matches!(d, TrimDecision::Hold(_))),
            "{decisions:?}"
        );
    }

    #[test]
    fn too_much_energy_after_first_crack_is_still_taken_away() {
        let decisions = replay(&HOT, Some(513.0));
        let cut = decisions.iter().find_map(|d| match d {
            TrimDecision::Step { delta, .. } => Some(*delta),
            _ => None,
        });
        assert_eq!(cut, Some(-5.0), "{decisions:?}");
    }

    #[test]
    fn no_heat_is_added_close_to_first_crack() {
        let behind = input(-4.0, -1.0);
        let near = TrimInput {
            fc_in_s: Some(40.0),
            ..behind
        };
        assert_eq!(
            after(3, &near),
            TrimDecision::Hold(TrimHold::NearFirstCrack)
        );
        let after_fc = TrimInput {
            fc_in_s: Some(-30.0),
            ..behind
        };
        assert_eq!(
            after(3, &after_fc),
            TrimDecision::Hold(TrimHold::NearFirstCrack)
        );
        // Taking heat away still works there.
        assert!(matches!(
            after(3, &TrimInput {
                fc_in_s: Some(-30.0),
                ..input(4.0, 0.0)
            }),
            TrimDecision::Step { delta, .. } if delta < 0.0
        ));
        // Far from it, heat comes.
        assert!(matches!(
            after(3, &TrimInput {
                fc_in_s: Some(200.0),
                ..behind
            }),
            TrimDecision::Step { delta, .. } if delta > 0.0
        ));
    }

    #[test]
    fn back_on_the_plan_the_correction_goes_back_to_the_recipe_by_halves() {
        let mut trim = Trim::new(TrimTuning::default());
        trim.applied(5.0);
        let on_plan = TrimInput {
            since_correction: Some(200.0),
            ..input(0.2, 0.3)
        };
        let mut eased = Vec::new();
        for i in 0..400 {
            let d = trim.decide(&TrimInput {
                since_charge: 300.0 + f64::from(i) * 1.5,
                ..on_plan
            });
            if let TrimDecision::Ease { delta } = d {
                trim.applied(delta);
                eased.push((f64::from(i) * 1.5, delta));
            }
        }
        let deltas: Vec<f64> = eased.iter().map(|e| e.1).collect();
        assert_eq!(deltas, vec![-3.0, -1.0, -1.0], "{eased:?}");
        assert_eq!(trim.offset(), 0.0);
        // Not before a minute on the plan.
        assert!(eased[0].0 >= 60.0, "{eased:?}");
        // A roast that is not on the plan keeps its correction.
        let mut trim = Trim::new(TrimTuning::default());
        trim.applied(5.0);
        let d = after_with(
            &mut trim,
            60,
            &TrimInput {
                since_correction: Some(200.0),
                ..input(-1.0, 0.0)
            },
        );
        assert_eq!(d, TrimDecision::Hold(TrimHold::InBand));
        assert_eq!(trim.offset(), 5.0);
    }

    /// The recipe of a real 200 g roast: seconds after CHARGE, burner, air.
    const RECIPE: [(f64, f64, f64); 14] = [
        (0.0, 29.0, 20.0),
        (50.0, 85.0, 20.0),
        (150.0, 90.0, 20.0),
        (260.0, 85.0, 20.0),
        (290.0, 85.0, 30.0),
        (320.0, 80.0, 30.0),
        (410.0, 75.0, 30.0),
        (445.0, 70.0, 30.0),
        (458.0, 70.0, 35.0),
        (485.0, 60.0, 35.0),
        (508.0, 50.0, 35.0),
        (538.0, 45.0, 35.0),
        (551.0, 40.0, 35.0),
        (566.0, 40.0, 40.0),
    ];
    const ROAST_S: usize = 630;

    struct Run {
        /// Bean temperature each second.
        bt: Vec<f64>,
        /// Burner changes the trim made.
        corrections: Vec<(usize, f64)>,
    }

    /// Roasts in the simulator along [`RECIPE`], the burner `handicap`
    /// points off (a machine that runs weaker or stronger than on the day
    /// the plan was made). With `plan`, the trim keeps to that curve.
    fn roast_with(tuning: TrimTuning, handicap: f64, plan: Option<&[f64]>) -> Run {
        let mut m = SimulatedMachine::new(1.0);
        m.set(Actuator::Air, 20.0);
        m.set(Actuator::Burner, 90.0);
        while m.probe() < 192.0 {
            m.advance(1.0);
        }
        m.set(Actuator::Burner, 38.0);
        m.advance(600.0);
        m.on_event(EventKind::Charge);

        let mut trim = Trim::new(tuning);
        let mut bt = Vec::new();
        let mut corrections = Vec::new();
        let mut burner = 0.0;
        let mut changed_at = 0_usize;
        let mut corrected_at: Option<usize> = None;
        for t in 0..ROAST_S {
            let now = t as f64;
            for &(at, value, air) in &RECIPE {
                if (at - now).abs() < 0.5 {
                    // The recipe's value, the trim's corrections carried along.
                    let next = (value + trim.carried()).clamp(0.0, 100.0);
                    if (next - burner).abs() > 1e-9 {
                        burner = next;
                        changed_at = t;
                    }
                    m.set(Actuator::Air, air);
                }
            }
            if let Some(plan) = plan
                && t >= 31
            {
                // The probe as the daemon reads it: a little noise, and now
                // and then a stray reading (as at 3:18 in BRYB-003).
                let read = |i: usize| bt[i] + noise(i);
                let ror = (read(t - 1) - read(t - 31)) * 2.0;
                let plan_ror = (plan[t - 1] - plan[t - 31]) * 2.0;
                let decision = trim.decide(&TrimInput {
                    since_charge: now,
                    bt: read(t - 1),
                    ror,
                    plan_bt: plan[t - 1],
                    plan_ror,
                    burner,
                    since_burner_change: (t - changed_at) as f64,
                    since_correction: corrected_at.map(|at| (t - at) as f64),
                    fc_in_s: None,
                });
                match decision {
                    TrimDecision::Step {
                        burner: to, delta, ..
                    } => {
                        burner = to;
                        changed_at = t;
                        corrected_at = Some(t);
                        trim.applied(delta);
                        corrections.push((t, delta));
                    }
                    TrimDecision::Ease { delta } => trim.applied(delta),
                    TrimDecision::Hold(_) => {}
                }
            }
            m.set(Actuator::Burner, (burner + handicap).clamp(0.0, 100.0));
            m.advance(1.0);
            bt.push(m.probe());
        }
        Run { bt, corrections }
    }

    /// Deterministic probe noise: ±0.3 °C, and 0.9 °C low every 37 s.
    fn noise(i: usize) -> f64 {
        if i % 37 == 0 {
            return -0.9;
        }
        let x = (i as f64 * 12.9898).sin() * 43_758.545_3;
        (x - x.floor() - 0.5) * 0.6
    }

    fn roast(handicap: f64, plan: Option<&[f64]>) -> Run {
        roast_with(TrimTuning::default(), handicap, plan)
    }

    fn worst(run: &Run, plan: &[f64], from: usize) -> f64 {
        run.bt
            .iter()
            .zip(plan)
            .skip(from)
            .map(|(a, b)| (a - b).abs())
            .fold(0.0, f64::max)
    }

    #[test]
    fn in_the_simulator_it_stays_quiet_on_plan_and_brings_a_weak_roast_back() {
        let plan = roast(0.0, None).bt;

        // A roast that runs as planned needs no correction at all.
        let same = roast(0.0, Some(&plan));
        assert!(same.corrections.is_empty(), "{:?}", same.corrections);

        // A machine five burner points weaker falls behind without the trim …
        let weak = roast(-5.0, None);
        let behind = plan[ROAST_S - 1] - weak.bt[ROAST_S - 1];
        assert!(behind > 5.0, "only {behind} °C behind");

        // … and with it ends within the band, by a handful of calm steps.
        let trimmed = roast(-5.0, Some(&plan));
        let end = (plan[ROAST_S - 1] - trimmed.bt[ROAST_S - 1]).abs();
        assert!(end < 3.0, "ends {end} °C off; {:?}", trimmed.corrections);
        assert!(
            worst(&trimmed, &plan, 300) < worst(&weak, &plan, 300),
            "no closer than without"
        );
        assert!(
            (1..=3).contains(&trimmed.corrections.len()),
            "{:?}",
            trimmed.corrections
        );
        for pair in trimmed.corrections.windows(2) {
            assert!(pair[1].0 - pair[0].0 >= 90, "too fast: {pair:?}");
        }

        // A stronger machine is held back the same way.
        let strong = roast(5.0, Some(&plan));
        let end = (plan[ROAST_S - 1] - strong.bt[ROAST_S - 1]).abs();
        assert!(end < 3.0, "ends {end} °C off; {:?}", strong.corrections);
        assert!(strong.corrections.iter().all(|c| c.1 < 0.0));

        // Far off, it helps as far as it may go from the recipe, and no further.
        let far = roast(-12.0, Some(&plan));
        let moved: f64 = far.corrections.iter().map(|c| c.1).sum();
        assert!((moved - 10.0).abs() < 1e-9, "{:?}", far.corrections);
    }
}
