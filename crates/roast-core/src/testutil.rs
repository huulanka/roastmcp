//! A synthetic but realistic roast curve for tests.
//!
//! Preheat at 200 °C until CHARGE at recording time 30 s. The probe then reads
//! a mix of the cold beans and its own lagging heat, giving a turning point of
//! about 92 °C roughly 63 s after CHARGE, followed by a rise with declining
//! rate of rise: DRY END (150 °C) at about 4:11, first crack (196 °C) at about
//! 8:28 and DROP at 10:00 roast time with 205 °C. After DROP the probe cools
//! quickly in the empty drum.

use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::{BatchInfo, EventKind, EventOrigin, Roast, RoastEvent, RoastOrigin, Sample};

/// Recording time of CHARGE.
pub(crate) const CHARGE_T: f64 = 30.0;
/// Roast time of DROP.
pub(crate) const DROP_ROAST_T: f64 = 600.0;
/// Recording time of DROP.
pub(crate) const DROP_T: f64 = CHARGE_T + DROP_ROAST_T;
/// Preheat bean-probe temperature.
pub(crate) const PREHEAT_BT: f64 = 200.0;

const T_INF: f64 = 230.0;
const BEAN_SPAN: f64 = 185.0;
const BEAN_TAU: f64 = 299.8;
const PROBE_OFFSET: f64 = 155.0;
const PROBE_TAU: f64 = 25.0;

/// Bean temperature at roast time `tau` (seconds since CHARGE).
pub(crate) fn roast_bt(tau: f64) -> f64 {
    T_INF - BEAN_SPAN * (-tau / BEAN_TAU).exp() + PROBE_OFFSET * (-tau / PROBE_TAU).exp()
}

/// Exact rate of rise at roast time `tau`, °C/min.
pub(crate) fn roast_ror(tau: f64) -> f64 {
    60.0 * (BEAN_SPAN / BEAN_TAU * (-tau / BEAN_TAU).exp()
        - PROBE_OFFSET / PROBE_TAU * (-tau / PROBE_TAU).exp())
}

/// Bean temperature at recording time `t`.
pub(crate) fn bt_at(t: f64) -> f64 {
    if t <= CHARGE_T {
        PREHEAT_BT
    } else if t <= DROP_T {
        roast_bt(t - CHARGE_T)
    } else {
        roast_bt(DROP_ROAST_T) - 25.0 * (1.0 - (-(t - DROP_T) / 8.0).exp())
    }
}

/// Samples every `dt` seconds from 0 to `end_t` with uniform noise of
/// `±noise_c`, deterministic for a given `seed`.
pub(crate) fn samples(dt: f64, end_t: f64, noise_c: f64, seed: u64) -> Vec<Sample> {
    let mut rng = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
    let mut out = Vec::new();
    let mut k = 0u32;
    loop {
        let t = f64::from(k) * dt;
        if t > end_t {
            break;
        }
        rng ^= rng << 13;
        rng ^= rng >> 7;
        rng ^= rng << 17;
        let u = (rng >> 11) as f64 / (1u64 << 53) as f64; // 0..1
        out.push(Sample::bt(t, bt_at(t) + noise_c * (2.0 * u - 1.0)));
        k += 1;
    }
    out
}

/// The noise-free curve at 1 s, up to 30 s after DROP.
pub(crate) fn curve() -> Vec<Sample> {
    samples(1.0, DROP_T + 30.0, 0.0, 1)
}

/// Roast time at which the bean temperature first reaches `target` after the
/// turning point.
pub(crate) fn crossing(target: f64) -> f64 {
    let (mut lo, mut hi) = (tp_roast_t(), DROP_ROAST_T);
    for _ in 0..100 {
        let mid = (lo + hi) / 2.0;
        if roast_bt(mid) < target {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    (lo + hi) / 2.0
}

/// Roast time of the turning point (minimum of [`roast_bt`]).
pub(crate) fn tp_roast_t() -> f64 {
    let (mut lo, mut hi) = (1.0, 200.0);
    for _ in 0..100 {
        let mid = (lo + hi) / 2.0;
        if roast_ror(mid) < 0.0 {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    (lo + hi) / 2.0
}

/// The synthetic roast with human-marked events at their exact positions.
pub(crate) fn roast() -> Roast {
    let ev = |roast_t: f64, kind| RoastEvent {
        t: CHARGE_T + roast_t,
        kind,
        origin: EventOrigin::Human,
        bt: Some(if roast_t == 0.0 {
            PREHEAT_BT
        } else {
            roast_bt(roast_t)
        }),
    };
    Roast {
        id: Uuid::nil(),
        title: "synthetic".into(),
        roasted_at: DateTime::<Utc>::UNIX_EPOCH,
        machine: None,
        sampling_interval_s: 1.0,
        batch: BatchInfo {
            green_weight_g: Some(200.0),
            roasted_weight_g: Some(170.0),
            beans: None,
        },
        ambient: None,
        roast_degree: None,
        green_lot_id: None,
        profile_version_id: None,
        experiment_id: None,
        origin: RoastOrigin::Simulated,
        samples: curve(),
        events: vec![
            ev(0.0, EventKind::Charge),
            ev(tp_roast_t(), EventKind::TurningPoint),
            ev(crossing(150.0), EventKind::DryEnd),
            ev(crossing(196.0), EventKind::FirstCrackStart),
            ev(DROP_ROAST_T, EventKind::Drop),
        ],
        control_log: vec![],
        notes: vec![],
    }
}

#[test]
fn synthetic_curve_is_realistic() {
    let tp = tp_roast_t();
    assert!((55.0..=90.0).contains(&tp), "TP at {tp}");
    assert!(
        (85.0..=95.0).contains(&roast_bt(tp)),
        "TP BT {}",
        roast_bt(tp)
    );
    assert!((roast_bt(DROP_ROAST_T) - 205.0).abs() < 0.5);
    // RoR declines after its peak.
    assert!(roast_ror(180.0) > roast_ror(400.0));
    assert!(roast_ror(400.0) > roast_ror(590.0));
    assert!(roast_ror(590.0) > 3.0);
}
