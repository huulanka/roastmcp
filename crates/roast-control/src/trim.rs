//! The autopilot's fine control between the recipe's steps (ADR 0020).
//!
//! The recipe says when the burner goes where; the trim keeps the bean
//! temperature on the plan's curve around it. It works the way a careful
//! roaster does (step, wait, look): it looks where the bean temperature
//! will be in a minute, and only when that is outside a dead band around
//! the plan does it move the burner by a few points; then it waits until
//! the sluggish machine has answered before it looks again. It never moves
//! far from the recipe.
//!
//! It only decides; the daemon sends the step through the safety layer.

use serde::Serialize;

/// How the trim behaves.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TrimTuning {
    /// Dead band around the plan, °C: a predicted deviation inside it is left
    /// alone.
    pub band_c: f64,
    /// How far ahead the deviation from the plan is predicted from the
    /// difference in the rate of rise, s (about the time a burner change
    /// needs to show).
    pub horizon_s: f64,
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
            horizon_s: 90.0,
            catch_up_s: 120.0,
            wait_s: 90.0,
            settle_s: 20.0,
            max_step: 5.0,
            max_offset: 10.0,
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
}

/// The trim of one roast.
#[derive(Debug, Clone, PartialEq)]
pub struct Trim {
    tuning: TrimTuning,
    /// All corrections so far, burner points away from the recipe.
    offset: f64,
}

impl Trim {
    /// A fresh trim.
    #[must_use]
    pub fn new(tuning: TrimTuning) -> Self {
        Trim {
            tuning,
            offset: 0.0,
        }
    }

    /// A trim that takes up a roast it already corrected by `offset` points:
    /// those corrections still count toward how far it may go.
    #[must_use]
    pub fn resumed(tuning: TrimTuning, offset: f64) -> Self {
        Trim { tuning, offset }
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

    /// Where the bean temperature heads relative to the plan, °C: how far
    /// off it is now, plus what the difference in the rate of rise adds over
    /// the horizon.
    #[must_use]
    pub fn predicted_error(&self, input: &TrimInput) -> f64 {
        (input.bt - input.plan_bt) + (input.ror - input.plan_ror) * self.tuning.horizon_s / 60.0
    }

    /// Decides for this reading. A [`TrimDecision::Step`] counts only once
    /// [`Trim::applied`] reports what the machine took.
    #[must_use]
    pub fn decide(&self, input: &TrimInput) -> TrimDecision {
        let t = &self.tuning;
        if input.since_charge < t.start_s {
            return TrimDecision::Hold(TrimHold::Early);
        }
        if input.since_burner_change < t.settle_s
            || input.since_correction.is_some_and(|s| s < t.wait_s)
        {
            return TrimDecision::Hold(TrimHold::Settling);
        }
        let error = self.predicted_error(input);
        if error.abs() < t.band_c {
            return TrimDecision::Hold(TrimHold::InBand);
        }
        // Make the deviation up over the catch-up time: that needs this much
        // more (or less) rate of rise, and so many burner points.
        let ror_change = -error / (t.catch_up_s / 60.0);
        let wanted = (ror_change / t.ror_per_pct).clamp(-t.max_step, t.max_step);
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

    /// The machine took a correction of `delta` points.
    pub fn applied(&mut self, delta: f64) {
        self.offset += delta;
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
        }
    }

    #[test]
    fn it_leaves_the_burner_alone_inside_the_band_and_while_waiting() {
        let trim = Trim::new(TrimTuning::default());
        // 2 °C above the plan and rising like it: inside the band.
        assert_eq!(
            trim.decide(&input(2.0, 0.0)),
            TrimDecision::Hold(TrimHold::InBand)
        );
        // 4 °C below now, but catching up by itself: left alone.
        assert_eq!(
            trim.decide(&input(-4.0, 1.0)),
            TrimDecision::Hold(TrimHold::InBand)
        );
        // On the plan now, but rising too slowly: that heads out of the band.
        assert!(matches!(
            trim.decide(&input(0.0, -2.5)),
            TrimDecision::Step { .. }
        ));
        let off = input(-4.0, -1.0);
        assert!(matches!(trim.decide(&off), TrimDecision::Step { .. }));
        assert_eq!(
            trim.decide(&TrimInput {
                since_charge: 100.0,
                ..off
            }),
            TrimDecision::Hold(TrimHold::Early)
        );
        // Right after a recipe step, and until its own last correction shows.
        assert_eq!(
            trim.decide(&TrimInput {
                since_burner_change: 10.0,
                ..off
            }),
            TrimDecision::Hold(TrimHold::Settling)
        );
        assert_eq!(
            trim.decide(&TrimInput {
                since_correction: Some(60.0),
                ..off
            }),
            TrimDecision::Hold(TrimHold::Settling)
        );
        assert!(matches!(
            trim.decide(&TrimInput {
                since_correction: Some(95.0),
                ..off
            }),
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
        } = trim.decide(&input(-4.0, -2.0))
        else {
            panic!("no step");
        };
        assert_eq!((burner, delta), (85.0, 5.0));
        assert!((predicted_error_c + 7.0).abs() < 1e-9);
        trim.applied(5.0);
        trim.applied(5.0);
        // Ten points from the recipe: no further.
        assert_eq!(
            trim.decide(&input(-4.0, -2.0)),
            TrimDecision::Hold(TrimHold::AtLimit)
        );
        // Switched off and on again in the same roast: still no further.
        let again = Trim::resumed(TrimTuning::default(), trim.offset());
        assert_eq!(
            again.decide(&input(-4.0, -2.0)),
            TrimDecision::Hold(TrimHold::AtLimit)
        );
        // The other way is still open.
        let TrimDecision::Step { delta, .. } = trim.decide(&input(4.0, 0.0)) else {
            panic!("no step");
        };
        assert_eq!(delta, -5.0);
        // Never below the lowest burner.
        let fresh = Trim::new(TrimTuning::default());
        let TrimDecision::Step { delta, .. } = fresh.decide(&TrimInput {
            burner: 12.0,
            ..input(4.0, 0.0)
        }) else {
            panic!("no step");
        };
        assert_eq!(delta, -2.0);
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
                    let next = (value + trim.offset()).clamp(0.0, 100.0);
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
                let ror = (bt[t - 1] - bt[t - 31]) * 2.0;
                let plan_ror = (plan[t - 1] - plan[t - 31]) * 2.0;
                let decision = trim.decide(&TrimInput {
                    since_charge: now,
                    bt: bt[t - 1],
                    ror,
                    plan_bt: plan[t - 1],
                    plan_ror,
                    burner,
                    since_burner_change: (t - changed_at) as f64,
                    since_correction: corrected_at.map(|at| (t - at) as f64),
                });
                if let TrimDecision::Step {
                    burner: to, delta, ..
                } = decision
                {
                    burner = to;
                    changed_at = t;
                    corrected_at = Some(t);
                    trim.applied(delta);
                    corrections.push((t, delta));
                }
            }
            m.set(Actuator::Burner, (burner + handicap).clamp(0.0, 100.0));
            m.advance(1.0);
            bt.push(m.probe());
        }
        Run { bt, corrections }
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
