//! Rate of rise (RoR): how fast the bean temperature changes, in °C/min.
//!
//! Algorithm (our own, see `docs/architecture.md`):
//!
//! 1. **Spike rejection.** Each reading is compared with the median of the
//!    last [`RorConfig::median_len`] readings (including itself). A reading is
//!    dropped when it deviates from that median by more than the bean
//!    temperature could plausibly change over the time those readings span
//!    (at [`RorConfig::max_abs_c_per_min`]), plus a small noise allowance.
//!    Every reading still enters the median buffer, so a genuine step (such
//!    as CHARGE) is followed after a few samples.
//! 2. **Slope.** A least-squares line is fitted to the accepted readings of
//!    the last [`RorConfig::window_s`] seconds; its slope × 60 is the RoR. The
//!    fit uses the real sample times, so irregular intervals and gaps are
//!    handled. At least three readings are needed.
//! 3. **Plausibility.** A result whose magnitude exceeds
//!    [`RorConfig::max_abs_c_per_min`] is reported as `None` (for example
//!    around CHARGE, where the probe temperature falls steeply).
//!
//! The filter is causal, so [`RorTracker`] (streaming, one sample at a time)
//! and [`ror_series`] (offline, a whole curve) give identical results.

use std::collections::VecDeque;

use serde::{Deserialize, Serialize};

use crate::Sample;
use crate::num::{ls_slope, median};

/// Readings may deviate from the median by this much on top of the
/// rate-based bound, to tolerate sensor noise at short sampling intervals.
const SPIKE_NOISE_ALLOWANCE_C: f64 = 1.0;

/// After this many rejected readings in a row the RoR is reported as `None`
/// until readings are accepted again.
const MAX_REJECTED_RUN: usize = 3;

/// Parameters of the rate-of-rise computation. Configurable per machine
/// profile; missing fields take the defaults.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RorConfig {
    /// Length of the least-squares window, seconds. Default 20 s, the span
    /// Artisan users know (Cubean+ vendor settings: `DeltaSpan=20`), so curves
    /// compare with roasts from Artisan; at 1-1.5 s sampling that is 13-20
    /// readings, enough to average out thermocouple noise while showing a
    /// burner change within 20 s.
    pub window_s: f64,
    /// Number of recent readings whose median is the reference for spike
    /// rejection. Default 5: tolerates up to two consecutive bad readings
    /// while adding no lag to the fit.
    pub median_len: usize,
    /// Plausibility bound, °C/min. Default 60: bean temperature never rises
    /// or falls faster than 1 °C/s during a roast; faster changes are spikes
    /// or the CHARGE/DROP transients, where a RoR is meaningless.
    pub max_abs_c_per_min: f64,
}

impl Default for RorConfig {
    fn default() -> Self {
        RorConfig {
            window_s: 20.0,
            median_len: 5,
            max_abs_c_per_min: 60.0,
        }
    }
}

/// Streaming rate-of-rise computation: feed bean temperatures one at a time.
#[derive(Debug, Clone)]
pub struct RorTracker {
    cfg: RorConfig,
    /// The last `median_len` readings, accepted or not.
    recent: VecDeque<(f64, f64)>,
    /// Accepted readings within the fit window.
    window: VecDeque<(f64, f64)>,
    last_t: Option<f64>,
    last_accepted: Option<(f64, f64)>,
    /// Readings rejected in a row; a run of them means the curve itself
    /// moves faster than plausible (CHARGE), not a spike.
    rejected_run: usize,
    current: Option<f64>,
    scratch: Vec<f64>,
}

impl RorTracker {
    /// A tracker with the given configuration.
    #[must_use]
    pub fn new(cfg: RorConfig) -> Self {
        RorTracker {
            cfg,
            recent: VecDeque::new(),
            window: VecDeque::new(),
            last_t: None,
            last_accepted: None,
            rejected_run: 0,
            current: None,
            scratch: Vec::new(),
        }
    }

    /// The configuration in use.
    #[must_use]
    pub fn config(&self) -> &RorConfig {
        &self.cfg
    }

    /// Adds a reading at recording time `t` (seconds) and returns the rate of
    /// rise at `t` in °C/min, or `None` when it cannot be computed or is
    /// implausible. Readings must arrive in increasing time order; a reading
    /// that is not later than the previous one, or not finite, is ignored and
    /// yields `None`.
    pub fn push(&mut self, t: f64, bt: f64) -> Option<f64> {
        if !t.is_finite() || !bt.is_finite() || self.last_t.is_some_and(|last| t <= last) {
            return None;
        }
        self.last_t = Some(t);

        self.recent.push_back((t, bt));
        while self.recent.len() > self.cfg.median_len.max(1) {
            self.recent.pop_front();
        }
        if self.is_plausible(t, bt) {
            self.window.push_back((t, bt));
            self.last_accepted = Some((t, bt));
            self.rejected_run = 0;
        } else {
            self.rejected_run += 1;
        }
        let from = t - self.cfg.window_s;
        while self.window.front().is_some_and(|&(t0, _)| t0 < from) {
            self.window.pop_front();
        }

        if self.rejected_run >= MAX_REJECTED_RUN {
            self.current = None;
            return None;
        }
        let max = self.cfg.max_abs_c_per_min;
        self.current = ls_slope(self.window.iter().copied(), 3)
            .map(|slope| slope * 60.0)
            .filter(|ror| ror.abs() <= max);
        self.current
    }

    /// The value returned by the latest [`push`](Self::push).
    #[must_use]
    pub fn current(&self) -> Option<f64> {
        self.current
    }

    /// The latest reading that passed spike rejection, as `(t, bt)`.
    #[must_use]
    pub fn last_accepted(&self) -> Option<(f64, f64)> {
        self.last_accepted
    }

    /// Forgets all readings, for example at the start of a new recording.
    pub fn reset(&mut self) {
        *self = RorTracker::new(self.cfg);
    }

    fn is_plausible(&mut self, t: f64, bt: f64) -> bool {
        self.scratch.clear();
        self.scratch.extend(self.recent.iter().map(|&(_, v)| v));
        let Some(med) = median(&mut self.scratch) else {
            return true;
        };
        let span = self.recent.front().map_or(0.0, |&(t0, _)| t - t0);
        let bound = self.cfg.max_abs_c_per_min.max(0.0) / 60.0 * span + SPIKE_NOISE_ALLOWANCE_C;
        (bt - med).abs() <= bound
    }
}

impl Default for RorTracker {
    fn default() -> Self {
        RorTracker::new(RorConfig::default())
    }
}

/// Rate of rise for every sample of a curve, in °C/min (one entry per
/// sample; `None` where the sample has no bean temperature or the RoR is not
/// computable). Samples must be sorted by time. Identical to feeding the
/// samples to a [`RorTracker`].
#[must_use]
pub fn ror_series(samples: &[Sample], cfg: &RorConfig) -> Vec<Option<f64>> {
    let mut tracker = RorTracker::new(*cfg);
    samples
        .iter()
        .map(|s| s.bt.and_then(|bt| tracker.push(s.t, bt)))
        .collect()
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;
    use crate::testutil;

    fn linear(slope_c_per_min: f64, start: f64, times: &[f64]) -> Vec<Sample> {
        times
            .iter()
            .map(|&t| Sample::bt(t, start + slope_c_per_min / 60.0 * t))
            .collect()
    }

    #[test]
    fn follows_the_synthetic_curve() {
        let cfg = RorConfig::default();
        let samples = testutil::samples(1.0, testutil::DROP_T, 0.0, 1);
        let ror = ror_series(&samples, &cfg);
        for (s, r) in samples.iter().zip(&ror) {
            let tau = s.t - testutil::CHARGE_T;
            if !(150.0..=600.0).contains(&tau) {
                continue;
            }
            // A least-squares slope over a window estimates the derivative at
            // the window's centre.
            let expected = testutil::roast_ror(tau - cfg.window_s / 2.0);
            let got = r.expect("RoR after the turning point");
            assert!(
                (got - expected).abs() < 0.3,
                "t {} got {got} want {expected}",
                s.t
            );
        }
    }

    #[test]
    fn needs_three_points() {
        let mut tr = RorTracker::default();
        assert_eq!(tr.push(0.0, 100.0), None);
        assert_eq!(tr.push(1.0, 100.2), None);
        let r = tr.push(2.0, 100.4).unwrap();
        assert!((r - 12.0).abs() < 1e-9);
    }

    #[test]
    fn implausible_rates_are_none() {
        // Around CHARGE the probe falls by several °C per second.
        let ror = ror_series(&testutil::curve(), &RorConfig::default());
        let at = |t: f64| ror[t as usize];
        // The first readings after the drop are rejected as spikes, so the RoR
        // briefly stays near the preheat value; it never shows the plunge.
        assert!(at(testutil::CHARGE_T + 3.0).is_none_or(|r| r.abs() < 1.0));
        assert_eq!(at(testutil::CHARGE_T + 10.0), None);
    }

    #[test]
    fn rejects_spikes_and_dropouts() {
        let times: Vec<f64> = (0..120).map(f64::from).collect();
        let mut samples = linear(10.0, 150.0, &times);
        samples[50].bt = Some(0.0); // thermocouple dropout
        samples[80].bt = Some(400.0); // spike
        samples[81].bt = Some(390.0); // two in a row
        let ror = ror_series(&samples, &RorConfig::default());
        for r in &ror[2..] {
            assert!((r.unwrap() - 10.0).abs() < 1e-9);
        }
    }

    #[test]
    fn follows_a_genuine_step() {
        // A real level change is accepted once the median has caught up.
        let mut tr = RorTracker::default();
        for k in 0..40 {
            tr.push(f64::from(k), 180.0);
        }
        for k in 40..120 {
            tr.push(f64::from(k), 120.0);
        }
        assert_eq!(tr.current(), Some(0.0));
        assert_eq!(tr.last_accepted(), Some((119.0, 120.0)));
    }

    #[test]
    fn handles_gaps_and_missing_readings() {
        let times: Vec<f64> = (0..40)
            .map(f64::from)
            .chain((100..140).map(f64::from))
            .collect();
        let mut samples = linear(-5.0, 200.0, &times);
        samples[10].bt = None;
        let ror = ror_series(&samples, &RorConfig::default());
        assert_eq!(ror[10], None);
        // Right after the gap only one or two points are in the window.
        assert_eq!(ror[40], None);
        assert_eq!(ror[41], None);
        assert!((ror[42].unwrap() + 5.0).abs() < 1e-9);
        assert!((ror[79].unwrap() + 5.0).abs() < 1e-9);
    }

    #[test]
    fn ignores_time_going_backwards() {
        let mut tr = RorTracker::default();
        tr.push(0.0, 100.0);
        tr.push(1.0, 101.0);
        assert_eq!(tr.push(1.0, 500.0), None);
        assert_eq!(tr.push(0.5, 500.0), None);
        assert_eq!(tr.push(f64::NAN, 100.0), None);
        let r = tr.push(2.0, 102.0).unwrap();
        assert!((r - 60.0).abs() < 1e-9);
    }

    #[test]
    fn reset_forgets_everything() {
        let mut tr = RorTracker::default();
        for k in 0..10 {
            tr.push(f64::from(k), 100.0 + f64::from(k));
        }
        tr.reset();
        assert_eq!(tr.current(), None);
        assert_eq!(tr.last_accepted(), None);
        assert_eq!(tr.push(0.0, 50.0), None);
    }

    #[test]
    fn config_from_partial_json() {
        let cfg: RorConfig = serde_json::from_str(r#"{"window_s": 20}"#).unwrap();
        assert_eq!(cfg.window_s, 20.0);
        assert_eq!(cfg.median_len, 5);
        assert_eq!(cfg.max_abs_c_per_min, 60.0);
    }

    /// Strictly increasing sample times with irregular steps.
    fn irregular_times(steps: Vec<f64>) -> Vec<f64> {
        let mut t = 0.0;
        steps
            .into_iter()
            .map(|dt| {
                t += dt;
                t
            })
            .collect()
    }

    proptest! {
        #[test]
        fn streaming_and_offline_agree(
            bts in prop::collection::vec(prop::option::of(-50.0f64..400.0), 0..200),
            steps in prop::collection::vec(0.1f64..5.0, 200),
        ) {
            let times = irregular_times(steps);
            let samples: Vec<Sample> = bts
                .iter()
                .zip(&times)
                .map(|(&bt, &t)| Sample { bt, ..Sample::bt(t, 0.0) })
                .collect();
            let cfg = RorConfig::default();
            let offline = ror_series(&samples, &cfg);
            let mut tracker = RorTracker::new(cfg);
            let streamed: Vec<Option<f64>> = samples
                .iter()
                .map(|s| s.bt.and_then(|bt| tracker.push(s.t, bt)))
                .collect();
            prop_assert_eq!(offline, streamed);
        }

        #[test]
        fn constant_slope_yields_that_slope(
            slope in -50.0f64..50.0,
            start in 20.0f64..250.0,
            steps in prop::collection::vec(0.2f64..4.0, 10..150),
            gap_at in 0usize..150,
            gap in 0.0f64..60.0,
        ) {
            let mut steps = steps;
            if let Some(s) = steps.get_mut(gap_at) {
                *s += gap;
            }
            let samples = linear(slope, start, &irregular_times(steps));
            let cfg = RorConfig::default();
            let ror = ror_series(&samples, &cfg);
            for r in ror.iter().flatten() {
                prop_assert!((r - slope).abs() < 1e-6, "got {} want {}", r, slope);
            }
            // A RoR exists whenever the window holds three readings.
            prop_assert!(ror.last().copied().flatten().is_some() || samples.len() < 3
                || samples[samples.len() - 3].t < samples[samples.len() - 1].t - cfg.window_s);
        }

        #[test]
        fn large_spikes_do_not_move_it(
            slope in -50.0f64..50.0,
            start in 20.0f64..250.0,
            spikes in prop::collection::vec((5usize..200, 8.0f64..200.0, any::<bool>()), 0..12),
        ) {
            let times: Vec<f64> = (0..200).map(f64::from).collect();
            let mut samples = linear(slope, start, &times);
            // Isolated spikes: at least `median_len` samples apart.
            let mut used: Vec<usize> = Vec::new();
            for (at, size, up) in spikes {
                if used.iter().any(|&u| u.abs_diff(at) < 5) {
                    continue;
                }
                used.push(at);
                let s = &mut samples[at];
                s.bt = s.bt.map(|bt| if up { bt + size } else { bt - size });
            }
            let ror = ror_series(&samples, &RorConfig::default());
            for (k, r) in ror.iter().enumerate().skip(2) {
                let r = r.expect("RoR on a plausible ramp");
                prop_assert!((r - slope).abs() < 1e-6, "sample {}: got {} want {}", k, r, slope);
            }
        }

        #[test]
        fn small_spikes_move_it_only_a_little(
            slope in -40.0f64..40.0,
            at in 31usize..200,
            size in -8.0f64..8.0,
        ) {
            // One spike within the plausibility bound slips through the
            // median check; the least-squares fit bounds its influence. At
            // 1 Hz the window holds n readings; a reading d off the line at
            // the window's edge moves the slope by 60·d·((n-1)/2)/Sxx °C/min,
            // Sxx = n(n²-1)/12 (about 6.2 °C/min for 8 °C over 20 s).
            let cfg = RorConfig::default();
            let n = cfg.window_s + 1.0;
            let sxx = n * (n * n - 1.0) / 12.0;
            let bound = 60.0 * 8.0 * ((n - 1.0) / 2.0) / sxx + 0.5;
            let times: Vec<f64> = (0..240).map(f64::from).collect();
            let mut samples = linear(slope, 150.0, &times);
            samples[at].bt = samples[at].bt.map(|bt| bt + size);
            let ror = ror_series(&samples, &cfg);
            for r in ror.iter().skip(31) {
                let r = r.expect("RoR on a plausible ramp");
                prop_assert!((r - slope).abs() < bound, "got {} want {} (bound {})", r, slope, bound);
            }
        }
    }
}
