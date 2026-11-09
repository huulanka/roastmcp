//! Preheat: bring the empty drum to a target temperature and hold it, the way
//! a roaster does it by hand — full heat, one deliberate step down before the
//! target, then rare, meaningful corrections, each followed by a wait for the
//! sluggish machine to answer. ADR 0015 explains the design and how it
//! compares with Artisan's software PID.
//!
//! The controller only decides; the daemon sends its decisions through the
//! safety layer, which still bounds every raise.

use serde::{Deserialize, Serialize};

/// Fixed settings of the preheat controller.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PreheatTuning {
    /// Highest burner value it asks for, %.
    pub max_burner: f64,
    /// No correction while the predicted temperature is within this of the
    /// target, °C (the dead band).
    pub band_c: f64,
    /// How far ahead the temperature is predicted from the rate of rise, s:
    /// the lag of the 20 s rate of rise plus the machine's own delay.
    pub lead_s: f64,
    /// Wait after a change before judging its effect and deciding again, s.
    pub settle_s: f64,
    /// A predicted miss larger than this may be corrected before the settle
    /// time is over (but not before `urgent_after_s`), °C.
    pub urgent_c: f64,
    /// Earliest re-decision after a change, s.
    pub urgent_after_s: f64,
    /// A correction aims to close the error within this time, s.
    pub horizon_s: f64,
    /// Smallest change worth making, burner %.
    pub min_step: f64,
    /// Largest single correction while holding, burner %.
    pub max_step: f64,
    /// Above target + this, the burner goes off at once, °C.
    pub cut_above_c: f64,
}

impl Default for PreheatTuning {
    /// Values for the Skywalker Cubean+ (empty drum), checked against a model
    /// fitted to its first real preheat and against the simulator.
    fn default() -> Self {
        PreheatTuning {
            max_burner: 90.0,
            band_c: 2.0,
            lead_s: 30.0,
            settle_s: 45.0,
            urgent_c: 8.0,
            urgent_after_s: 20.0,
            horizon_s: 120.0,
            min_step: 3.0,
            max_step: 20.0,
            cut_above_c: 10.0,
        }
    }
}

/// What the controller knows about the machine, refined by every preheat and
/// kept between runs.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PlantEstimate {
    /// How much the rate of rise changes per burner percent, °C/min per %.
    pub ror_per_pct: f64,
    /// Burner needed to hold a temperature, % per °C above ambient.
    pub hold_pct_per_c: f64,
}

impl Default for PlantEstimate {
    /// The Cubean+ with an empty drum at air 30 %: 0.22 °C/min per % from the
    /// fitted model, about 21 % to hold 150 °C.
    fn default() -> Self {
        PlantEstimate {
            ror_per_pct: 0.22,
            hold_pct_per_c: 0.165,
        }
    }
}

/// A reading as the controller sees it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PreheatInput {
    /// Seconds, any monotonic clock.
    pub t: f64,
    /// Bean probe temperature, °C.
    pub bt: f64,
    /// Rate of rise, °C/min, if known.
    pub ror: Option<f64>,
    /// The burner as it is now, %.
    pub burner: f64,
}

/// Where the preheat is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PreheatPhase {
    /// Full heat towards the target.
    Heating,
    /// At the target, correcting rarely.
    Holding,
}

/// Why the burner should change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreheatReason {
    /// Full heat while far from the target.
    Heat,
    /// The one step down to the holding power before the target.
    StepToHold,
    /// A correction while holding.
    Correct,
    /// Far above the target: heat off.
    Cut,
}

/// A decision: set the burner to this value.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PreheatDecision {
    /// New burner value, %.
    pub burner: f64,
    /// Why.
    pub reason: PreheatReason,
}

#[derive(Debug, Clone, Copy)]
struct Change {
    t: f64,
    burner_before: f64,
    ror_before: Option<f64>,
    learned: bool,
}

/// The preheat controller. Feed it every reading with [`PreheatController::step`].
#[derive(Debug, Clone)]
pub struct PreheatController {
    target_c: f64,
    ambient_c: f64,
    tuning: PreheatTuning,
    plant: PlantEstimate,
    phase: PreheatPhase,
    last_change: Option<Change>,
}

impl PreheatController {
    /// A controller for `target_c`, starting from what is known about the machine.
    #[must_use]
    pub fn new(target_c: f64, ambient_c: f64, tuning: PreheatTuning, plant: PlantEstimate) -> Self {
        PreheatController {
            target_c,
            ambient_c,
            tuning,
            plant,
            phase: PreheatPhase::Heating,
            last_change: None,
        }
    }

    /// The target, °C.
    #[must_use]
    pub fn target_c(&self) -> f64 {
        self.target_c
    }

    /// Heating or holding.
    #[must_use]
    pub fn phase(&self) -> PreheatPhase {
        self.phase
    }

    /// What the controller has learned about the machine so far.
    #[must_use]
    pub fn plant(&self) -> PlantEstimate {
        self.plant
    }

    /// The burner that should hold the target, %.
    fn hold_burner(&self) -> f64 {
        self.plant.hold_pct_per_c * (self.target_c - self.ambient_c).max(0.0)
    }

    /// Burner change for a wanted change of the rate of rise (°C/s).
    fn burner_for(&self, d_ror_per_s: f64) -> f64 {
        d_ror_per_s * 60.0 / self.plant.ror_per_pct.max(0.01)
    }

    /// Takes a reading and returns a new burner value, or `None` to leave it.
    pub fn step(&mut self, r: PreheatInput) -> Option<PreheatDecision> {
        let tu = self.tuning;
        let ror_per_s = r.ror.unwrap_or(0.0) / 60.0;
        let predicted = r.bt + ror_per_s * tu.lead_s;
        let error = r.bt - self.target_c;

        // Far too hot: no waiting.
        if error > tu.cut_above_c {
            self.phase = PreheatPhase::Holding;
            return (r.burner > 0.0).then(|| self.change(r, 0.0, PreheatReason::Cut));
        }

        self.learn_response(r);

        if let Some(c) = self.last_change {
            let since = r.t - c.t;
            let gross_miss = (predicted - self.target_c).abs() > tu.urgent_c;
            if since < tu.settle_s && !(gross_miss && since >= tu.urgent_after_s) {
                return None;
            }
        }

        match self.phase {
            PreheatPhase::Heating => {
                if predicted >= self.target_c - tu.band_c {
                    // One deliberate step to the holding power, plus what it
                    // takes to close the remaining error within the horizon.
                    self.phase = PreheatPhase::Holding;
                    let wanted = self.hold_burner() - self.burner_for(error / tu.horizon_s);
                    let burner = wanted.clamp(0.0, tu.max_burner).round();
                    return Some(self.change(r, burner, PreheatReason::StepToHold));
                }
                // Full heat; the safety layer paces the raise. Not a step to
                // wait for.
                (r.burner < tu.max_burner).then_some(PreheatDecision {
                    burner: tu.max_burner,
                    reason: PreheatReason::Heat,
                })
            }
            PreheatPhase::Holding => {
                if (predicted - self.target_c).abs() <= tu.band_c {
                    self.learn_hold(r, error);
                    return None;
                }
                // Aim the rate of rise at closing the error within the horizon.
                let wanted_ror = -error / tu.horizon_s;
                let mut delta = self
                    .burner_for(wanted_ror - ror_per_s)
                    .clamp(-tu.max_step, tu.max_step);
                if delta.abs() < tu.min_step {
                    delta = tu.min_step.copysign(delta);
                }
                let burner = (r.burner + delta).clamp(0.0, tu.max_burner).round();
                ((burner - r.burner).abs() >= 1.0)
                    .then(|| self.change(r, burner, PreheatReason::Correct))
            }
        }
    }

    fn change(&mut self, r: PreheatInput, burner: f64, reason: PreheatReason) -> PreheatDecision {
        self.last_change = Some(Change {
            t: r.t,
            burner_before: r.burner,
            ror_before: r.ror,
            learned: false,
        });
        PreheatDecision { burner, reason }
    }

    /// After a settled change of at least 5 %, compare the rate of rise
    /// before and after: that is how strongly the burner acts.
    fn learn_response(&mut self, r: PreheatInput) {
        let Some(c) = self.last_change.as_mut() else {
            return;
        };
        if c.learned || r.t - c.t < self.tuning.settle_s {
            return;
        }
        c.learned = true;
        let du = r.burner - c.burner_before;
        let (Some(before), Some(after)) = (c.ror_before, r.ror) else {
            return;
        };
        if du.abs() < 5.0 {
            return;
        }
        let seen = (after - before) / du;
        if seen > 0.0 {
            let old = self.plant.ror_per_pct;
            self.plant.ror_per_pct = (0.6 * old + 0.4 * seen).clamp(old / 2.0, old * 2.0);
        }
    }

    /// Steady at the target: the burner now is the holding power.
    fn learn_hold(&mut self, r: PreheatInput, error: f64) {
        let above = self.target_c - self.ambient_c;
        if above < 20.0 || error.abs() > self.tuning.band_c {
            return;
        }
        if r.ror.is_some_and(|ror| ror.abs() < 1.0) {
            let seen = r.burner / above;
            self.plant.hold_pct_per_c = 0.95 * self.plant.hold_pct_per_c + 0.05 * seen;
        }
    }
}
