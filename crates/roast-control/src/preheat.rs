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
    /// While holding, no correction unless both the measured and the
    /// predicted temperature are this far from the target on the same side,
    /// °C (the dead band, ADR 0018). Before the target, the step to the
    /// holding power comes when the prediction enters it.
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
    /// Largest boost above the holding power at the step to hold, burner %.
    pub max_boost: f64,
    /// Smallest change worth making, burner %.
    pub min_step: f64,
    /// Largest single correction while holding, burner %.
    pub max_step: f64,
    /// While holding, a second correction in the same direction waits this
    /// long after the last change, s: the time a burner change takes to show
    /// its full effect on the rate of rise (about 90 s on the Cubean+,
    /// measured on 2026-11-09).
    pub repeat_after_s: f64,
    /// While holding, no correction while the rate of rise already carries
    /// the temperature back towards the target at least this fast, °C/min.
    pub returning_ror: f64,
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
            max_boost: 15.0,
            min_step: 3.0,
            max_step: 10.0,
            repeat_after_s: 90.0,
            returning_ror: 1.0,
            cut_above_c: 10.0,
        }
    }
}

/// Temperature above ambient the holding power is given for: 200 °C at
/// 22 °C ambient, °C.
pub const HOLD_REF_ABOVE_C: f64 = 178.0;

/// How the holding power grows with the temperature above ambient: with its
/// power `HOLD_EXPONENT`, because the losses grow faster than linearly.
/// Fitted to the Cubean+: about 21 % held 150 °C (128 °C above ambient) and
/// about 39 % held 200 °C (ADR 0018, `docs/research/preheat-control.md`).
pub const HOLD_EXPONENT: f64 = 1.8;

/// What the controller knows about the machine, refined by every preheat and
/// kept between runs.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(from = "StoredEstimate")]
pub struct PlantEstimate {
    /// How much the rate of rise changes per burner percent, °C/min per %.
    pub ror_per_pct: f64,
    /// Burner needed to hold 200 °C ([`HOLD_REF_ABOVE_C`] above ambient), %.
    /// Other temperatures scale with [`HOLD_EXPONENT`].
    pub hold_pct_200: f64,
}

/// `control.json` before ADR 0018 kept the holding power as a linear factor
/// (`hold_pct_per_c`, % per °C above ambient); it is read as the same power
/// at 200 °C.
#[derive(Deserialize)]
struct StoredEstimate {
    ror_per_pct: f64,
    #[serde(default)]
    hold_pct_200: Option<f64>,
    #[serde(default)]
    hold_pct_per_c: Option<f64>,
}

impl From<StoredEstimate> for PlantEstimate {
    fn from(s: StoredEstimate) -> Self {
        PlantEstimate {
            ror_per_pct: s.ror_per_pct,
            hold_pct_200: s
                .hold_pct_200
                .or(s.hold_pct_per_c.map(|k| k * HOLD_REF_ABOVE_C))
                .unwrap_or(PlantEstimate::default().hold_pct_200),
        }
    }
}

impl PlantEstimate {
    /// The burner that holds `above_c` °C above ambient, %.
    #[must_use]
    pub fn hold_burner(&self, above_c: f64) -> f64 {
        self.hold_pct_200 * (above_c.max(0.0) / HOLD_REF_ABOVE_C).powf(HOLD_EXPONENT)
    }
}

impl Default for PlantEstimate {
    /// The Cubean+ with an empty drum: 0.22 °C/min per % from the model
    /// fitted to its first preheat (air 30 %), and about 39 % to hold 200 °C
    /// at air 20 % from the roaster's Artisan preheats (ADR 0018).
    fn default() -> Self {
        PlantEstimate {
            ror_per_pct: 0.22,
            hold_pct_200: 39.0,
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
    /// +1 for a raise, -1 for a cut.
    direction: f64,
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
        self.plant.hold_burner(self.target_c - self.ambient_c)
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

        let gross_miss = (predicted - self.target_c).abs() > tu.urgent_c;
        if let Some(c) = self.last_change {
            let since = r.t - c.t;
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
                    // The boost is capped: at a high rate of rise the
                    // measured error is large when the prediction arrives,
                    // and the momentum closes most of it on its own.
                    let boost = (-self.burner_for(error / tu.horizon_s)).min(tu.max_boost);
                    let wanted = self.hold_burner() + boost;
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
                // Correct only when the measured and the predicted temperature
                // are both outside the band, on the same side: a swing of the
                // rate of rise alone moves the prediction by ±2 °C (ADR 0018).
                let predicted_error = predicted - self.target_c;
                let outside = error.abs() > tu.band_c
                    && predicted_error.abs() > tu.band_c
                    && error.signum() == predicted_error.signum();
                if !outside {
                    self.learn_hold(r, error);
                    return None;
                }
                // Already on the way back: the last change is working.
                if r.ror
                    .is_some_and(|ror| -ror * error.signum() >= tu.returning_ror)
                {
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
                // Another step the same way only once the last one has had
                // its full effect (ADR 0018 on the machine: four cuts in
                // 2.5 min chased a curve that was already turning).
                if let Some(c) = self.last_change {
                    if c.direction == delta.signum() && r.t - c.t < tu.repeat_after_s && !gross_miss
                    {
                        return None;
                    }
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
            direction: (burner - r.burner).signum(),
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

    /// Settled near the target: the burner that would keep the rate of rise
    /// at zero is the holding power. Learned from every such reading, since
    /// with the band rule the curve tends to turn only at the band's edges.
    fn learn_hold(&mut self, r: PreheatInput, error: f64) {
        let above = self.target_c - self.ambient_c;
        if above < 20.0 || error.abs() > self.tuning.band_c {
            return;
        }
        let Some(ror) = r.ror.filter(|ror| ror.abs() < 4.0) else {
            return;
        };
        let hold_now = r.burner - ror / self.plant.ror_per_pct.max(0.01);
        let seen = hold_now.max(0.0) / (above / HOLD_REF_ABOVE_C).powf(HOLD_EXPONENT);
        self.plant.hold_pct_200 = 0.98 * self.plant.hold_pct_200 + 0.02 * seen;
    }
}
