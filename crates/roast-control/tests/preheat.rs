//! The preheat controller against two machines: a model fitted to the first
//! real Cubean+ preheat (2026-11-09, empty drum, air 30 %) whose losses grow
//! with the temperature so that it holds 200 °C at about 39 % like the
//! roaster's Artisan preheats (ADR 0018), and the built-in
//! simulator's empty drum, which heats about three times as fast and has a
//! slow probe. Readings every 1.5 s, the daemon's rate of rise, and the safety
//! layer's pace for automatic raises (10 points per 10 s).

#![allow(clippy::unwrap_used, clippy::expect_used)]

use roast_control::{PlantEstimate, PreheatController, PreheatInput, PreheatTuning};
use roast_core::RorTracker;

trait Plant {
    fn step(&mut self, burner: f64, dt: f64) -> f64;
}

/// Two first-order paths from the burner to the probe plus a short delay:
/// fitted to the real run with 0.8 °C RMS error. The fit is linear, so it
/// would hold 200 °C at 24 %; the real machine needs about 39 % (and 21 %
/// for 150 °C). The losses therefore scale with the temperature so that the
/// steady state follows `39 % × (ΔT / 178)^1.8`, while the burner acts as
/// fitted.
struct Cubean {
    y_fast: f64,
    y_slow: f64,
    gain: f64,
    delay_s: f64,
    history: Vec<(f64, f64)>,
    t: f64,
}

impl Cubean {
    const AMBIENT: f64 = 25.0;

    /// Burner that holds `above` °C above ambient at gain 1, %.
    fn hold(above: f64) -> f64 {
        39.0 * (above / 178.0).powf(1.8)
    }

    fn new(start_c: f64, gain: f64, extra_delay_s: f64) -> Self {
        let above = start_c - Self::AMBIENT;
        Cubean {
            y_fast: above * 0.67 / 7.42,
            y_slow: above * 6.75 / 7.42,
            gain,
            delay_s: 1.6 + extra_delay_s,
            history: Vec::new(),
            t: 0.0,
        }
    }
}

impl Plant for Cubean {
    fn step(&mut self, burner: f64, dt: f64) -> f64 {
        self.history.push((self.t, burner));
        self.t += dt;
        let u = self
            .history
            .iter()
            .rev()
            .find(|(t, _)| *t <= self.t - self.delay_s)
            .map_or(0.0, |(_, u)| *u);
        let n = 10;
        for _ in 0..n {
            let h = dt / f64::from(n);
            let above = (self.y_fast + self.y_slow).max(1.0);
            let loss = 7.42 * Self::hold(above) / above;
            self.y_fast += h * (0.67 * self.gain * u - loss * self.y_fast) / 252.0;
            self.y_slow += h * (6.75 * self.gain * u - loss * self.y_slow) / 2013.0;
        }
        Self::AMBIENT + self.y_fast + self.y_slow
    }
}

/// The simulator's empty drum (`roast-driver`), probe lag 25 s.
struct Simulator {
    drum: f64,
    probe: f64,
    air: f64,
}

impl Plant for Simulator {
    fn step(&mut self, burner: f64, dt: f64) -> f64 {
        let n = (dt / 0.1).round() as usize;
        for _ in 0..n {
            let loss = (4.0 + 6.0 * self.air / 100.0) * (self.drum - 22.0);
            self.drum += (1700.0 * burner / 100.0 - loss) * 0.1 / 1500.0;
            self.probe += (self.drum - self.probe) * 0.1 / 25.0;
        }
        self.probe
    }
}

struct Run {
    /// (t, bt, burner) every reading.
    trace: Vec<(f64, f64, f64)>,
    /// First time within 2 °C of the target.
    reached: Option<f64>,
    plant: PlantEstimate,
}

impl Run {
    /// Burner changes after `from` seconds.
    fn changes_after(&self, from: f64) -> usize {
        self.changes_between(from, f64::MAX)
    }

    /// Burner changes between `from` and `to` seconds.
    fn changes_between(&self, from: f64, to: f64) -> usize {
        self.trace
            .windows(2)
            .filter(|w| w[1].0 >= from && w[1].0 <= to && w[1].2 != w[0].2)
            .count()
    }

    /// The largest single burner change after `from` seconds, points.
    fn largest_change_after(&self, from: f64) -> f64 {
        self.trace
            .windows(2)
            .filter(|w| w[1].0 >= from)
            .map(|w| (w[1].2 - w[0].2).abs())
            .fold(0.0, f64::max)
    }

    /// Highest and lowest bean temperature after `from` seconds.
    fn range_after(&self, from: f64) -> (f64, f64) {
        self.trace
            .iter()
            .filter(|p| p.0 >= from)
            .fold((f64::MIN, f64::MAX), |(hi, lo), p| {
                (hi.max(p.1), lo.min(p.1))
            })
    }

    fn final_bt(&self) -> f64 {
        self.trace.last().unwrap().1
    }
}

fn run(
    plant: &mut dyn Plant,
    target: f64,
    estimate: PlantEstimate,
    start_burner: f64,
    duration_s: f64,
) -> Run {
    let dt = 1.5;
    let mut ctl = PreheatController::new(target, 25.0, PreheatTuning::default(), estimate);
    let mut ror = RorTracker::default();
    let mut burner = start_burner;
    let mut last_raise = f64::MIN;
    let mut trace = Vec::new();
    let mut reached = None;
    let mut t = 0.0;
    while t < duration_s {
        t += dt;
        let bt = (plant.step(burner, dt) * 10.0).round() / 10.0;
        let r = ror.push(t, bt);
        trace.push((t, bt, burner));
        if reached.is_none() && bt >= target - 2.0 {
            reached = Some(t);
        }
        if let Some(d) = ctl.step(PreheatInput {
            t,
            bt,
            ror: r,
            burner,
        }) {
            if d.burner > burner {
                // The safety layer: automatic raises at most 10 points per 10 s.
                if t - last_raise >= 10.0 {
                    burner = d.burner.min(burner + 10.0);
                    last_raise = t;
                }
            } else {
                burner = d.burner;
            }
        }
    }
    Run {
        trace,
        reached,
        plant: ctl.plant(),
    }
}

#[test]
fn cubean_reaches_200_without_overshoot_and_then_rests() {
    let r = run(
        &mut Cubean::new(30.0, 1.0, 0.0),
        200.0,
        PlantEstimate::default(),
        0.0,
        1500.0,
    );
    let reached = r.reached.expect("reaches the target");
    assert!(reached < 700.0, "reached after {reached} s");
    let (hi, lo) = r.range_after(reached);
    assert!(hi <= 203.5, "peak {hi}");
    assert!(lo >= 197.5, "low {lo}");
    assert!((r.final_bt() - 200.0).abs() <= 2.0);
    // ADR 0018 on the machine: at most 3 changes while holding (a preheat
    // holds a few minutes before CHARGE). The old controller: 39 in 7 min.
    let changes = r.changes_between(reached, reached + 300.0);
    assert!(changes <= 3, "{changes} burner changes while holding");
    assert!(r.largest_change_after(reached) <= 10.0);
}

#[test]
fn cubean_stays_calm_when_the_machine_differs_from_the_estimate() {
    for (gain, delay) in [(0.7, 0.0), (1.4, 0.0), (1.0, 10.0)] {
        let r = run(
            &mut Cubean::new(30.0, gain, delay),
            200.0,
            PlantEstimate::default(),
            0.0,
            1800.0,
        );
        let reached = r.reached.expect("reaches the target");
        let (hi, lo) = r.range_after(reached);
        // Corrections of at most 10 points (ADR 0018) let a much stronger
        // machine overshoot by a few degrees; calm wins over a tight peak.
        assert!(hi <= 206.0, "gain {gain}, delay {delay}: peak {hi}");
        assert!(lo >= 197.0, "gain {gain}, delay {delay}: low {lo}");
        // Over the 20+ min after reaching the target: at most one change in
        // about three minutes, even with the estimate off by 40 %.
        let changes = r.changes_after(reached);
        assert!(
            changes <= 8,
            "gain {gain}, delay {delay}: {changes} changes"
        );
    }
}

#[test]
fn cubean_preheats_again_after_a_roast() {
    // Hot drum, burner still high: cut, then settle at the target.
    let r = run(
        &mut Cubean::new(215.0, 1.0, 0.0),
        200.0,
        PlantEstimate::default(),
        90.0,
        1200.0,
    );
    let (hi, lo) = r.range_after(400.0);
    assert!(hi <= 203.0 && lo >= 197.0, "{lo}..{hi}");
}

#[test]
fn simulator_holds_and_learns_its_much_stronger_burner() {
    let first = run(
        &mut Simulator {
            drum: 30.0,
            probe: 30.0,
            air: 10.0,
        },
        200.0,
        PlantEstimate::default(),
        0.0,
        1500.0,
    );
    let reached = first.reached.expect("reaches the target");
    let (hi, _) = first.range_after(reached);
    assert!(hi <= 205.5, "peak {hi}");
    assert!((first.final_bt() - 200.0).abs() <= 3.0);
    assert!(
        first.plant.ror_per_pct > 0.33,
        "learned a stronger burner: {:?}",
        first.plant
    );
    assert!(
        first.plant.hold_pct_200 > 42.0,
        "learned a higher holding power: {:?}",
        first.plant
    );

    // Starting hot with what it learned: far closer than without.
    let hot = |estimate| {
        let r = run(
            &mut Simulator {
                drum: 207.0,
                probe: 207.0,
                air: 10.0,
            },
            200.0,
            estimate,
            90.0,
            1200.0,
        );
        r.range_after(120.0).1
    };
    let naive = hot(PlantEstimate::default());
    let learned = hot(first.plant);
    assert!(
        learned > naive,
        "learned low {learned} vs naive low {naive}"
    );
    assert!(learned >= 196.0, "learned low {learned}");
}

#[test]
fn far_above_the_target_the_burner_goes_off_at_once() {
    let mut ctl = PreheatController::new(
        200.0,
        25.0,
        PreheatTuning::default(),
        PlantEstimate::default(),
    );
    let d = ctl
        .step(PreheatInput {
            t: 0.0,
            bt: 211.0,
            ror: Some(5.0),
            burner: 60.0,
        })
        .expect("a decision");
    assert_eq!(d.burner, 0.0);
}

#[test]
fn inside_the_dead_band_nothing_changes() {
    let mut ctl = PreheatController::new(
        200.0,
        25.0,
        PreheatTuning::default(),
        PlantEstimate::default(),
    );
    // Reach the target: one step to the holding power.
    let first = ctl.step(PreheatInput {
        t: 0.0,
        bt: 199.0,
        ror: Some(0.5),
        burner: 90.0,
    });
    assert!(first.is_some());
    for i in 1..200 {
        let t = 60.0 + f64::from(i) * 1.5;
        let d = ctl.step(PreheatInput {
            t,
            bt: 199.5 + (f64::from(i) * 0.3).sin() * 1.2,
            ror: Some(0.4),
            burner: first.unwrap().burner,
        });
        assert!(d.is_none(), "changed at {t}: {d:?}");
    }
}

#[test]
fn holding_corrects_only_when_measured_and_predicted_agree() {
    let mut ctl = PreheatController::new(
        200.0,
        22.0,
        PreheatTuning::default(),
        PlantEstimate::default(),
    );
    let step = ctl
        .step(PreheatInput {
            t: 0.0,
            bt: 199.0,
            ror: Some(0.5),
            burner: 90.0,
        })
        .expect("the step to the holding power");
    let burner = step.burner;
    // In the band, but the rate of rise points 3 °C out: no correction.
    assert!(
        ctl.step(PreheatInput {
            t: 60.0,
            bt: 201.0,
            ror: Some(6.0),
            burner,
        })
        .is_none()
    );
    // Below the band, but rising back into it: no correction either.
    assert!(
        ctl.step(PreheatInput {
            t: 61.5,
            bt: 197.0,
            ror: Some(8.0),
            burner,
        })
        .is_none()
    );
    // Both out on the same side: one correction of at most 10 points.
    let d = ctl
        .step(PreheatInput {
            t: 63.0,
            bt: 203.0,
            ror: Some(8.0),
            burner,
        })
        .expect("a correction");
    assert!(d.burner < burner && burner - d.burner <= 10.0, "{d:?}");
}

#[test]
fn the_holding_power_grows_faster_than_the_temperature() {
    let e = PlantEstimate::default();
    // Anchored on the roaster's Artisan preheats at air 20 %: about 39 %.
    assert!((e.hold_burner(178.0) - 39.0).abs() < 0.01);
    // 150 °C took about 21 % on the machine.
    let at_150 = e.hold_burner(128.0);
    assert!((20.0..23.0).contains(&at_150), "{at_150}");
}

#[test]
fn a_linear_estimate_from_before_adr_0018_is_read_as_the_power_at_200() {
    let old: PlantEstimate =
        serde_json::from_str(r#"{"ror_per_pct": 0.393, "hold_pct_per_c": 0.231}"#).unwrap();
    assert!((old.hold_pct_200 - 41.1).abs() < 0.1, "{old:?}");
    let new: PlantEstimate = serde_json::from_str(&serde_json::to_string(&old).unwrap()).unwrap();
    assert_eq!(new, old);
}
