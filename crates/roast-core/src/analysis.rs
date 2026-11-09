//! Live event detection (CHARGE, turning point, DRY END, DROP) and roast
//! phases.
//!
//! The detector is causal: it sees one reading at a time, as the daemon does
//! while recording, and never moves an event that was already emitted. Events
//! a person marked by hand always win: the detector only emits a kind that the
//! roast does not contain yet.

use std::collections::VecDeque;

use serde::{Deserialize, Serialize};

use crate::{EventKind, EventOrigin, Roast, RoastEvent};

/// Thresholds of the event detector. Configurable per machine profile;
/// missing fields take the defaults.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DetectorConfig {
    /// CHARGE is only detected when the drum was preheated to at least this
    /// bean temperature, °C.
    pub charge_min_bt_c: f64,
    /// CHARGE is detected when the bean temperature falls by this much, °C …
    pub charge_drop_c: f64,
    /// … within this many seconds.
    pub charge_window_s: f64,
    /// The turning point is confirmed once the temperature rose this much
    /// above the lowest reading after CHARGE, °C.
    pub tp_confirm_rise_c: f64,
    /// The turning point is only searched for this long after CHARGE, s.
    pub tp_max_after_charge_s: f64,
    /// Bean temperature that ends the drying phase, °C.
    pub dry_end_c: f64,
    /// DROP is detected when the bean temperature falls this far below its
    /// recent peak, °C. The level is the median of the last five readings,
    /// so up to two bad readings never end a roast …
    pub drop_fall_c: f64,
    /// … within this many seconds of that peak.
    pub drop_window_s: f64,
    /// DROP is only searched for after DRY END and this long after CHARGE, s.
    pub drop_min_after_charge_s: f64,
}

impl Default for DetectorConfig {
    fn default() -> Self {
        DetectorConfig {
            charge_min_bt_c: 100.0,
            charge_drop_c: 5.0,
            charge_window_s: 8.0,
            tp_confirm_rise_c: 2.0,
            tp_max_after_charge_s: 240.0,
            dry_end_c: 150.0,
            // In 23 real Cubean+ roasts the probe peaked at the drop and then
            // fell 1.5-2 °C/s once the beans had left: 5 °C below the peak
            // within about 10 s. With beans in the drum the bean temperature
            // never falls that fast (a hard heat cut gives about 1 °C in 10 s).
            drop_fall_c: 5.0,
            drop_window_s: 25.0,
            drop_min_after_charge_s: 180.0,
        }
    }
}

/// Detects CHARGE, the turning point, DRY END and DROP from a live curve.
#[derive(Debug, Clone)]
pub struct EventDetector {
    cfg: DetectorConfig,
    recent: VecDeque<(f64, f64)>,
    lowest_after_charge: Option<(f64, f64)>,
}

impl EventDetector {
    /// A detector with the given thresholds.
    #[must_use]
    pub fn new(cfg: DetectorConfig) -> Self {
        EventDetector {
            cfg,
            recent: VecDeque::new(),
            lowest_after_charge: None,
        }
    }

    /// The thresholds in use.
    #[must_use]
    pub fn config(&self) -> &DetectorConfig {
        &self.cfg
    }

    /// Feeds one bean-temperature reading at recording time `t` and returns
    /// the events it detects. `known` are the events the roast already has;
    /// a kind that is present there is never emitted again.
    pub fn push(&mut self, t: f64, bt: f64, known: &[RoastEvent]) -> Vec<RoastEvent> {
        let mut out = Vec::new();
        if !t.is_finite() || !bt.is_finite() {
            return out;
        }
        let has =
            |k: EventKind, out: &[RoastEvent]| known.iter().chain(out.iter()).any(|e| e.kind == k);
        let find = |k: EventKind, out: &[RoastEvent]| {
            known
                .iter()
                .chain(out.iter())
                .find(|e| e.kind == k)
                .map(|e| e.t)
        };
        if has(EventKind::Drop, &out) {
            return out;
        }

        self.recent.push_back((t, bt));
        let keep_from = t - self.cfg.charge_window_s.max(self.cfg.drop_window_s);
        while self.recent.front().is_some_and(|&(t0, _)| t0 < keep_from) {
            self.recent.pop_front();
        }

        if !has(EventKind::Charge, &out) {
            let peak = self.peak_since(t - self.cfg.charge_window_s);
            if let Some((pt, pbt)) = peak
                && pbt >= self.cfg.charge_min_bt_c
                && pbt - bt >= self.cfg.charge_drop_c
            {
                out.push(detected(pt, EventKind::Charge, pbt));
                self.lowest_after_charge = Some((t, bt));
            }
            return out;
        }

        let Some(charge_t) = find(EventKind::Charge, &out) else {
            return out;
        };
        if t <= charge_t {
            return out;
        }

        if !has(EventKind::TurningPoint, &out) && t - charge_t <= self.cfg.tp_max_after_charge_s {
            let low = self.lowest_after_charge.get_or_insert((t, bt));
            if bt < low.1 {
                *low = (t, bt);
            } else if bt - low.1 >= self.cfg.tp_confirm_rise_c {
                let (lt, lbt) = *low;
                out.push(detected(lt, EventKind::TurningPoint, lbt));
            }
        }

        if !has(EventKind::DryEnd, &out)
            && has(EventKind::TurningPoint, &out)
            && bt >= self.cfg.dry_end_c
        {
            out.push(detected(t, EventKind::DryEnd, bt));
        }

        if has(EventKind::DryEnd, &out) && t - charge_t >= self.cfg.drop_min_after_charge_s {
            let peak = self.peak_since(t - self.cfg.drop_window_s);
            if let (Some((pt, pbt)), Some(level)) = (peak, self.recent_median())
                && pbt - level >= self.cfg.drop_fall_c
            {
                out.push(detected(pt, EventKind::Drop, pbt));
            }
        }
        out
    }

    /// The highest reading at or after `from`.
    fn peak_since(&self, from: f64) -> Option<(f64, f64)> {
        self.recent
            .iter()
            .copied()
            .filter(|&(t0, _)| t0 >= from)
            .max_by(|a, b| a.1.total_cmp(&b.1))
    }

    /// The median of the last five readings (needs five): two bad readings
    /// cannot move it, as in the rate-of-rise spike filter.
    fn recent_median(&self) -> Option<f64> {
        let n = self.recent.len();
        if n < 5 {
            return None;
        }
        let mut last: Vec<f64> = self.recent.iter().skip(n - 5).map(|r| r.1).collect();
        last.sort_by(f64::total_cmp);
        Some(last[2])
    }
}

impl Default for EventDetector {
    fn default() -> Self {
        EventDetector::new(DetectorConfig::default())
    }
}

fn detected(t: f64, kind: EventKind, bt: f64) -> RoastEvent {
    RoastEvent {
        t,
        kind,
        origin: EventOrigin::Auto,
        bt: Some(bt),
    }
}

/// Durations of the three roast phases, seconds, and the development time
/// ratio. A phase that has not started is `None`; a running phase counts up
/// to `now`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Phases {
    /// CHARGE to DRY END (or to now).
    pub drying_s: f64,
    /// DRY END to first crack (or to now).
    pub maillard_s: Option<f64>,
    /// First crack to DROP (or to now).
    pub development_s: Option<f64>,
    /// CHARGE to DROP (or to now).
    pub total_s: f64,
    /// Development time as percent of the total roast time.
    pub dtr_pct: Option<f64>,
}

/// Phases of a roast at recording time `now` (use the last sample's time for
/// a live roast; it is ignored once the roast has a DROP). `None` before
/// CHARGE.
#[must_use]
pub fn phases(roast: &Roast, now: f64) -> Option<Phases> {
    let at = |k: EventKind| roast.event(k).map(|e| e.t);
    let charge = at(EventKind::Charge)?;
    let end = at(EventKind::Drop).unwrap_or(now).max(charge);
    let dry = at(EventKind::DryEnd).filter(|&d| d >= charge && d <= end);
    let fc = at(EventKind::FirstCrackStart).filter(|&f| f >= charge && f <= end);

    let drying_s = dry.or(fc).unwrap_or(end) - charge;
    let maillard_s = dry.map(|d| fc.unwrap_or(end) - d);
    let development_s = fc.map(|f| end - f);
    let total_s = end - charge;
    let dtr_pct = development_s
        .filter(|_| total_s > 0.0)
        .map(|d| d / total_s * 100.0);
    Some(Phases {
        drying_s,
        maillard_s,
        development_s,
        total_s,
        dtr_pct,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil;

    fn detect_all(samples: &[crate::Sample]) -> Vec<RoastEvent> {
        let mut d = EventDetector::default();
        let mut events = Vec::new();
        for s in samples {
            if let Some(bt) = s.bt {
                let new = d.push(s.t, bt, &events);
                events.extend(new);
            }
        }
        events
    }

    #[test]
    fn detects_charge_tp_and_dry_end_on_a_realistic_curve() {
        let events = detect_all(&testutil::curve());
        let t = |k| events.iter().find(|e| e.kind == k).map(|e| e.t);
        let charge = t(EventKind::Charge).unwrap();
        assert!(
            (charge - testutil::CHARGE_T).abs() <= 1.0,
            "charge at {charge}"
        );
        let tp = t(EventKind::TurningPoint).unwrap() - charge;
        assert!((50.0..80.0).contains(&tp), "tp {tp}");
        let dry = t(EventKind::DryEnd).unwrap() - charge;
        assert!((240.0..262.0).contains(&dry), "dry end {dry}");
        // DROP is dated at the peak, i.e. where the beans left the drum.
        let drop = t(EventKind::Drop).unwrap();
        assert!((drop - testutil::DROP_T).abs() <= 1.0, "drop at {drop}");
        assert_eq!(events.len(), 4);
        assert!(events.iter().all(|e| e.origin == EventOrigin::Auto));
    }

    #[test]
    fn manual_events_win() {
        let manual = RoastEvent {
            t: 5.0,
            kind: EventKind::Charge,
            origin: EventOrigin::Human,
            bt: None,
        };
        let mut d = EventDetector::default();
        let mut events = vec![manual];
        for s in testutil::curve() {
            let new = d.push(s.t, s.bt.unwrap_or(0.0), &events);
            events.extend(new);
        }
        let charges: Vec<_> = events
            .iter()
            .filter(|e| e.kind == EventKind::Charge)
            .collect();
        assert_eq!(charges.len(), 1);
        assert_eq!(charges[0].origin, EventOrigin::Human);
    }

    #[test]
    fn drop_is_found_within_seconds_and_dated_at_the_peak() {
        // The probe as measured on the Cubean+: the peak lingers a few
        // seconds, then falls about 2 °C/s once the beans are out.
        let mut samples = Vec::new();
        let mut events = vec![
            RoastEvent {
                t: 30.0,
                kind: EventKind::Charge,
                origin: EventOrigin::Human,
                bt: None,
            },
            RoastEvent {
                t: 280.0,
                kind: EventKind::DryEnd,
                origin: EventOrigin::Auto,
                bt: None,
            },
        ];
        for i in 0..=1200 {
            let t = 30.0 + f64::from(i) * 0.5;
            let drop_t = 620.0;
            let bt = if t <= drop_t {
                195.0 + (t - 560.0).max(0.0) * 0.15
            } else if t <= drop_t + 5.0 {
                204.0
            } else {
                204.0 - 2.0 * (t - drop_t - 5.0)
            };
            samples.push((t, bt));
        }
        let mut d = EventDetector::default();
        let mut found_at = None;
        for &(t, bt) in &samples {
            let new = d.push(t, bt, &events);
            if new.iter().any(|e| e.kind == EventKind::Drop) && found_at.is_none() {
                found_at = Some(t);
            }
            events.extend(new);
        }
        let drop = events.iter().find(|e| e.kind == EventKind::Drop).unwrap();
        assert_eq!(drop.origin, EventOrigin::Auto);
        assert!((619.0..=626.0).contains(&drop.t), "dated at {}", drop.t);
        let found_at = found_at.unwrap();
        assert!(found_at <= 620.0 + 10.0, "found only at {found_at}");
    }

    #[test]
    fn one_bad_reading_never_drops() {
        let mut events = vec![
            RoastEvent {
                t: 0.0,
                kind: EventKind::Charge,
                origin: EventOrigin::Human,
                bt: None,
            },
            RoastEvent {
                t: 250.0,
                kind: EventKind::DryEnd,
                origin: EventOrigin::Auto,
                bt: None,
            },
        ];
        let mut d = EventDetector::default();
        for i in 0..400 {
            let t = 300.0 + f64::from(i);
            // A steady rise with one reading 20 °C low and, later, two
            // readings 8 °C low among five.
            let mut bt = 170.0 + f64::from(i) * 0.1;
            if i == 100 {
                bt -= 20.0;
            }
            if i == 200 || i == 202 {
                bt -= 8.0;
            }
            let new = d.push(t, bt, &events);
            events.extend(new);
        }
        assert!(
            events.iter().all(|e| e.kind != EventKind::Drop),
            "{events:?}"
        );
    }

    #[test]
    fn no_drop_before_dry_end_or_early_after_charge() {
        // CHARGE-like fall early in the roast: not a DROP.
        let mut events = vec![RoastEvent {
            t: 0.0,
            kind: EventKind::Charge,
            origin: EventOrigin::Human,
            bt: None,
        }];
        let mut d = EventDetector::default();
        for i in 0..120 {
            let t = f64::from(i);
            let bt = 180.0 - f64::from(i) * 1.5;
            let new = d.push(t, bt.max(90.0), &events);
            events.extend(new);
        }
        assert!(events.iter().all(|e| e.kind != EventKind::Drop));
    }

    #[test]
    fn no_charge_without_preheat() {
        let samples: Vec<_> = (0..120)
            .map(|i| crate::Sample::bt(f64::from(i), 60.0 - f64::from(i) * 0.5))
            .collect();
        assert!(detect_all(&samples).is_empty());
    }

    #[test]
    fn phases_and_dtr() {
        let mut roast = testutil::roast();
        roast.events.clear();
        for (t, kind) in [
            (30.0, EventKind::Charge),
            (270.0, EventKind::DryEnd),
            (510.0, EventKind::FirstCrackStart),
            (630.0, EventKind::Drop),
        ] {
            roast.events.push(RoastEvent {
                t,
                kind,
                origin: EventOrigin::Human,
                bt: None,
            });
        }
        let p = phases(&roast, 9999.0).unwrap();
        assert_eq!(p.drying_s, 240.0);
        assert_eq!(p.maillard_s, Some(240.0));
        assert_eq!(p.development_s, Some(120.0));
        assert_eq!(p.total_s, 600.0);
        assert!((p.dtr_pct.unwrap() - 20.0).abs() < 1e-9);
    }
}
