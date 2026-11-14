//! Playing recorded sound through the detector, alone or alongside the
//! roast it belongs to, and comparing what it heard with the marks.

use roast_core::{CrackExpectation, EventKind, Roast};
use serde::Serialize;

use crate::crack::{CrackLogic, HeardCrack, RoastContext};
use crate::listener::Listener;
use crate::pops::{Band, Pop, Sensitivity};

/// The kinds the detector hears.
pub const HEARD_KINDS: [EventKind; 3] = [
    EventKind::FirstCrackStart,
    EventKind::FirstCrackEnd,
    EventKind::SecondCrackStart,
];

/// What the detector made of a recording.
#[derive(Debug, Clone, Serialize)]
pub struct Analysis {
    pub rate: u32,
    pub seconds: f64,
    /// Pops, `t` on the roast's recording clock (or the sound's without a roast).
    pub pops: Vec<Pop>,
    pub heard: Vec<HeardCrack>,
    /// For each heard kind the roast has a mark of: heard minus marked, s.
    pub deviations: Vec<Deviation>,
    /// Pops per 10 s: `(start, crack band, high band)`.
    pub per_10s: Vec<(f64, usize, usize)>,
}

/// How far a heard crack lies from the person's mark.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct Deviation {
    pub kind: EventKind,
    /// The mark, s since CHARGE.
    pub marked_s: f64,
    /// The heard one, s since CHARGE; `None`: not heard.
    pub heard_s: Option<f64>,
    /// `heard_s − marked_s`.
    pub delta_s: Option<f64>,
}

/// Plays `samples` through the detector. With a roast, `offset_s` is where
/// the sound starts on the roast's recording clock; the roast gives the bean
/// temperature, CHARGE and dry end, and its marks are what the heard cracks
/// are compared with. Without one the temperature is taken to be inside the
/// window throughout.
#[must_use]
pub fn analyze(
    samples: &[f32],
    rate: u32,
    sensitivity: Sensitivity,
    roast: Option<&Roast>,
    offset_s: f64,
    expectation: &CrackExpectation,
) -> Analysis {
    let mut listener = Listener::new(rate, sensitivity);
    let mut logic = CrackLogic::new(sensitivity);
    let mut pops = Vec::new();
    let mut heard: Vec<HeardCrack> = Vec::new();
    let chunk = (rate / 10).max(1) as usize;
    let mut done = 0usize;
    let mut next_tick = 0.0;
    for part in samples.chunks(chunk) {
        let out = listener.push(part);
        done += part.len();
        for p in out.pops {
            let p = Pop {
                t: p.t + offset_s,
                ..p
            };
            pops.push(p);
            let ctx = context(roast, p.t, &heard, expectation);
            if let Some(h) = logic.pop(p, &ctx) {
                heard.push(h);
            }
        }
        let now = done as f64 / f64::from(rate) + offset_s;
        if now >= next_tick {
            next_tick = now + 1.0;
            let ctx = context(roast, now, &heard, expectation);
            if let Some(h) = logic.tick(now, &ctx) {
                heard.push(h);
            }
        }
    }
    let charge = roast.and_then(Roast::charge_t).unwrap_or(0.0);
    let deviations = roast
        .map(|r| {
            HEARD_KINDS
                .iter()
                .filter_map(|&kind| {
                    let marked = r.events.iter().find(|e| {
                        e.kind == kind && e.origin != roast_core::EventOrigin::Detector
                    })?;
                    let h = heard.iter().find(|h| h.kind == kind).map(|h| h.t - charge);
                    let m = marked.t - charge;
                    Some(Deviation {
                        kind,
                        marked_s: m,
                        heard_s: h,
                        delta_s: h.map(|h| h - m),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    let seconds = samples.len() as f64 / f64::from(rate.max(1));
    let mut per_10s = Vec::new();
    let mut start = offset_s;
    while start < offset_s + seconds {
        let inside = |b: Band| {
            pops.iter()
                .filter(|p| p.band == b && p.t >= start && p.t < start + 10.0)
                .count()
        };
        per_10s.push((start, inside(Band::Crack), inside(Band::High)));
        start += 10.0;
    }
    Analysis {
        rate,
        seconds,
        pops,
        heard,
        deviations,
        per_10s,
    }
}

fn context(
    roast: Option<&Roast>,
    t: f64,
    heard: &[HeardCrack],
    expectation: &CrackExpectation,
) -> RoastContext {
    let own = |k: EventKind| heard.iter().find(|h| h.kind == k).map(|h| h.t);
    let Some(roast) = roast else {
        let fc = own(EventKind::FirstCrackStart);
        return RoastContext {
            bt: Some(if fc.is_some() {
                expectation.high + 20.0
            } else {
                expectation.bt
            }),
            charged: true,
            dry_end: None,
            first_crack: fc.map(|t| (t, Some(expectation.bt))),
            first_crack_end: own(EventKind::FirstCrackEnd).is_some(),
            second_crack: own(EventKind::SecondCrackStart).is_some(),
            window: (expectation.low, expectation.high),
        };
    };
    let bt_at = |t: f64| bt_at(roast, t);
    let before = |k: EventKind| roast.event(k).map(|e| e.t).filter(|&e| e <= t);
    // Only what the detector heard itself: the person's marks are what it
    // is measured against.
    let fc = own(EventKind::FirstCrackStart);
    RoastContext {
        bt: bt_at(t),
        charged: before(EventKind::Charge).is_some(),
        dry_end: before(EventKind::DryEnd),
        first_crack: fc.map(|f| (f, bt_at(f))),
        first_crack_end: own(EventKind::FirstCrackEnd).is_some(),
        second_crack: own(EventKind::SecondCrackStart).is_some(),
        window: (expectation.low, expectation.high),
    }
}

/// The roast's bean temperature at recording time `t`, interpolated.
#[must_use]
pub fn bt_at(roast: &Roast, t: f64) -> Option<f64> {
    let s = &roast.samples;
    let i = s.partition_point(|x| x.t < t);
    let b = s.get(i)?;
    let Some(a) = i.checked_sub(1).and_then(|j| s.get(j)) else {
        return b.bt;
    };
    match (a.bt, b.bt) {
        (Some(x), Some(y)) if b.t > a.t => Some(x + (y - x) * (t - a.t) / (b.t - a.t)),
        (x, y) => y.or(x),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::synth::Synth;

    /// A roast whose bean temperature climbs 1 °C every 4 s from 150 °C at
    /// t = 300, with CHARGE at 0 and first crack marked at 480 (195 °C).
    fn roast() -> Roast {
        let samples: Vec<serde_json::Value> = (0..700)
            .map(|t| {
                let bt = 150.0 + (f64::from(t) - 300.0).max(0.0) / 4.0;
                serde_json::json!({"t": f64::from(t), "bt": bt, "et": 220.0})
            })
            .collect();
        serde_json::from_value(serde_json::json!({
            "id": "6f1c55a2-6a35-4a43-a1a4-1c2f3e1d0a01",
            "title": "test",
            "roasted_at": "2026-11-14T19:00:00Z",
            "sampling_interval_s": 1.0,
            "batch": {},
            "origin": {"kind": "recorded"},
            "samples": samples,
            "events": [
                {"t": 0.0, "kind": "charge", "origin": "human", "bt": 150.0},
                {"t": 280.0, "kind": "dry_end", "origin": "human", "bt": 150.0},
                {"t": 480.0, "kind": "first_crack_start", "origin": "human", "bt": 195.0}
            ],
            "control_log": [],
            "notes": []
        }))
        .unwrap_or_else(|e| panic!("{e}"))
    }

    #[test]
    fn a_synthetic_first_crack_is_heard_near_the_mark() {
        let r = roast();
        // Sound starts at t = 420; pops from 478 on, every ~2.5 s.
        let mut s = Synth::new(48_000, 5);
        let mut sound = s.take(58.0);
        for _ in 0..16 {
            s.pop(false);
            sound.extend(s.take(2.5));
        }
        sound.extend(s.take(40.0));
        let e = CrackExpectation::from_roasts(&[], &[]);
        let a = analyze(&sound, 48_000, Sensitivity::Normal, Some(&r), 420.0, &e);
        let fc = a
            .deviations
            .iter()
            .find(|d| d.kind == EventKind::FirstCrackStart)
            .unwrap_or_else(|| panic!("{a:?}"));
        let delta = fc
            .delta_s
            .unwrap_or_else(|| panic!("not heard: {:?}", a.heard));
        assert!(delta.abs() < 3.0, "{delta}");
        assert!(
            a.heard.iter().any(|h| h.kind == EventKind::FirstCrackEnd),
            "{:?}",
            a.heard
        );
    }

    #[test]
    fn temperatures_come_from_the_samples() {
        let r = roast();
        assert_eq!(bt_at(&r, 300.0), Some(150.0));
        assert_eq!(bt_at(&r, 302.0), Some(150.5));
        assert_eq!(bt_at(&r, 9999.0), None);
    }
}
