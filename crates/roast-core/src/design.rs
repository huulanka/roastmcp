//! Designing a profile's plan: a target curve shaped by milestones, and a
//! recipe of burner and air steps (ADR 0017). A curve keeps the natural
//! shape of the roast it came from; moving a milestone stretches the curve
//! between its neighbours in time and shifts it in temperature.
//!
//! The desktop app's profile designer (`app/src/lib/profile.ts`) does the
//! same in the browser; this is the version for the daemon and the MCP
//! server, so a plan an AI writes looks like one made in the app.

use serde::{Deserialize, Serialize};

use crate::Actuator;
use crate::event::EventKind;
use crate::profile::{
    ActionTrigger, DropRule, EventTarget, EventTargets, PlannedAction, ProfileTarget, RecipeAnchor,
    TargetPoint, ToleranceBand,
};
use crate::roast::Roast;

/// Spacing of a stored target curve, seconds.
const STEP_S: f64 = 2.0;

/// A point of a curve: seconds since CHARGE and bean temperature.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct CurvePoint {
    pub t: f64,
    pub bt: f64,
}

/// Where the roast should be when.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Milestones {
    /// Bean temperature at CHARGE, °C.
    pub charge: f64,
    pub dry_end: CurvePoint,
    pub first_crack: CurvePoint,
    pub drop: CurvePoint,
}

/// Milestones of a typical espresso roast of 200 g on the Cubean+ (the
/// roaster's own roasts).
pub const TEMPLATE_MILESTONES: Milestones = Milestones {
    charge: 200.0,
    dry_end: CurvePoint {
        t: 280.0,
        bt: 155.0,
    },
    first_crack: CurvePoint {
        t: 525.0,
        bt: 193.0,
    },
    drop: CurvePoint {
        t: 640.0,
        bt: 204.0,
    },
};

fn lerp(a: f64, b: f64, f: f64) -> f64 {
    a + (b - a) * f
}

/// Bean temperature of a curve at `t` (linear between points, held at the ends).
#[must_use]
pub fn bt_at(curve: &[CurvePoint], t: f64) -> f64 {
    let (Some(first), Some(last)) = (curve.first(), curve.last()) else {
        return f64::NAN;
    };
    if t <= first.t {
        return first.bt;
    }
    if t >= last.t {
        return last.bt;
    }
    let i = curve.partition_point(|p| p.t < t).max(1);
    let (a, b) = (curve[i - 1], curve[i]);
    lerp(a.bt, b.bt, (t - a.t) / (b.t - a.t))
}

/// The turning point: the lowest temperature in the first three minutes.
#[must_use]
pub fn turning_point(curve: &[CurvePoint]) -> CurvePoint {
    let mut best = curve
        .first()
        .copied()
        .unwrap_or(CurvePoint { t: 0.0, bt: 0.0 });
    for p in curve.iter().take_while(|p| p.t <= 180.0) {
        if p.bt < best.bt {
            best = *p;
        }
    }
    best
}

/// Resamples to every `STEP_S` seconds from 0 to `end`.
fn resample(curve: &[CurvePoint], end: f64) -> Vec<CurvePoint> {
    let mut out = Vec::new();
    let mut t = 0.0;
    while t < end {
        out.push(CurvePoint {
            t,
            bt: bt_at(curve, t),
        });
        t += STEP_S;
    }
    out.push(CurvePoint {
        t: end,
        bt: bt_at(curve, end),
    });
    out
}

/// A centred moving average over about `window_s` seconds, keeping the ends.
fn smooth(curve: &[CurvePoint], window_s: f64) -> Vec<CurvePoint> {
    // A handful of points; the cast cannot truncate meaningfully.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let half = ((window_s / STEP_S / 2.0).round() as usize).max(1);
    curve
        .iter()
        .enumerate()
        .map(|(i, p)| {
            if i < half || i + half >= curve.len() {
                return *p;
            }
            let window = &curve[i - half..=i + half];
            #[allow(clippy::cast_precision_loss)]
            let n = window.len() as f64;
            CurvePoint {
                t: p.t,
                bt: window.iter().map(|q| q.bt).sum::<f64>() / n,
            }
        })
        .collect()
}

/// A roast's curve from CHARGE to DROP and its milestones; `None` without a
/// CHARGE or with too few readings.
#[must_use]
pub fn from_roast(roast: &Roast) -> Option<(Vec<CurvePoint>, Milestones)> {
    let charge = roast.charge_t()?;
    let raw: Vec<CurvePoint> = roast
        .samples
        .iter()
        .filter(|s| s.t >= charge)
        .filter_map(|s| {
            Some(CurvePoint {
                t: s.t - charge,
                bt: s.bt?,
            })
        })
        .collect();
    if raw.len() < 10 {
        return None;
    }
    let at = |kind: EventKind| {
        roast
            .events
            .iter()
            .find(|e| e.kind == kind)
            .map(|e| e.t - charge)
    };
    let end = at(EventKind::Drop).unwrap_or_else(|| raw[raw.len() - 1].t);
    let curve = smooth(&resample(&raw, end), 6.0);
    let point = |t: Option<f64>, fallback_bt: f64| match t {
        Some(t) => CurvePoint {
            t,
            bt: bt_at(&curve, t),
        },
        // Without the event: where the curve first reaches a usual temperature.
        None => curve
            .iter()
            .find(|p| p.t > 90.0 && p.bt >= fallback_bt)
            .copied()
            .unwrap_or(CurvePoint {
                t: end * if fallback_bt < 180.0 { 0.45 } else { 0.8 },
                bt: fallback_bt,
            }),
    };
    let milestones = Milestones {
        charge: curve[0].bt,
        dry_end: point(at(EventKind::DryEnd), 150.0),
        first_crack: point(at(EventKind::FirstCrackStart), 193.0),
        drop: CurvePoint {
            t: end,
            bt: bt_at(&curve, end),
        },
    };
    Some((curve, milestones))
}

fn anchors(m: &Milestones, tp: CurvePoint) -> [CurvePoint; 5] {
    [
        CurvePoint {
            t: 0.0,
            bt: m.charge,
        },
        tp,
        m.dry_end,
        m.first_crack,
        m.drop,
    ]
}

/// Moves a curve's milestones from `from` to `to`: time is stretched
/// linearly between neighbouring milestones, temperature shifted by an
/// offset that changes linearly between them. The turning point stays.
#[must_use]
pub fn warp(curve: &[CurvePoint], from: &Milestones, to: &Milestones) -> Vec<CurvePoint> {
    let tp = turning_point(curve);
    let a = anchors(from, tp);
    let b = anchors(to, tp);
    let mut out = Vec::new();
    for p in curve {
        if p.t > from.drop.t {
            break;
        }
        let mut i = 0;
        while i < a.len() - 2 && p.t > a[i + 1].t {
            i += 1;
        }
        let span = a[i + 1].t - a[i].t;
        let f = if span > 0.0 {
            (p.t - a[i].t) / span
        } else {
            0.0
        };
        out.push(CurvePoint {
            t: lerp(b[i].t, b[i + 1].t, f),
            bt: p.bt + lerp(b[i].bt - a[i].bt, b[i + 1].bt - a[i + 1].bt, f),
        });
    }
    resample(&out, to.drop.t)
}

/// What is wrong with milestones, if anything: roast order, time between
/// them, rising temperatures.
#[must_use]
pub fn milestones_problem(m: &Milestones) -> Option<&'static str> {
    let finite = [
        m.charge,
        m.dry_end.t,
        m.dry_end.bt,
        m.first_crack.t,
        m.first_crack.bt,
        m.drop.t,
        m.drop.bt,
    ]
    .iter()
    .all(|v| v.is_finite());
    if !finite {
        return Some("milestones need numbers");
    }
    if m.dry_end.t <= 120.0 {
        return Some("dry end must come after 2:00");
    }
    if m.first_crack.t <= m.dry_end.t + 30.0 || m.drop.t <= m.first_crack.t + 30.0 {
        return Some("dry end, first crack and drop need at least 30 s between them");
    }
    if m.drop.t > 1800.0 {
        return Some("the drop must come within 30 min");
    }
    if !(m.dry_end.bt < m.first_crack.bt && m.first_crack.bt < m.drop.bt) {
        return Some("the temperatures must rise from dry end to first crack to drop");
    }
    if !(50.0..=300.0).contains(&m.charge) || m.drop.bt > 300.0 {
        return Some("temperatures must be between 50 and 300 °C");
    }
    None
}

/// Rate of rise, °C/min: least-squares slope over the trailing 20 s, like
/// the daemon.
fn ror_of(curve: &[CurvePoint]) -> Vec<Option<f64>> {
    (0..curve.len())
        .map(|i| {
            let p = curve[i];
            let mut j = i;
            while j > 0 && p.t - curve[j - 1].t <= 20.0 {
                j -= 1;
            }
            let w = &curve[j..=i];
            if w.len() < 3 {
                return None;
            }
            #[allow(clippy::cast_precision_loss)]
            let n = w.len() as f64;
            let mt = w.iter().map(|q| q.t).sum::<f64>() / n;
            let mb = w.iter().map(|q| q.bt).sum::<f64>() / n;
            let (num, den) = w.iter().fold((0.0, 0.0), |(num, den), q| {
                (
                    num + (q.t - mt) * (q.bt - mb),
                    den + (q.t - mt) * (q.t - mt),
                )
            });
            (den > 0.0).then(|| num / den * 60.0)
        })
        .collect()
}

fn round1(v: f64) -> f64 {
    (v * 10.0).round() / 10.0
}

/// Time steps first in time order, then temperature steps by temperature,
/// then steps after dry end and after first crack by their offset.
#[must_use]
pub fn sort_actions(actions: &[PlannedAction]) -> Vec<PlannedAction> {
    let key = |a: &PlannedAction| match a.trigger {
        ActionTrigger::Time { roast_time_s } => roast_time_s,
        ActionTrigger::Bt { bt_c } => 10_000.0 + bt_c,
        ActionTrigger::Event {
            event: RecipeAnchor::DryEnd,
            offset_s,
        } => 20_000.0 + offset_s,
        ActionTrigger::Event {
            event: RecipeAnchor::FirstCrackStart,
            offset_s,
        } => 30_000.0 + offset_s,
    };
    let mut out = actions.to_vec();
    out.sort_by(|p, q| key(p).total_cmp(&key(q)));
    out
}

/// The plan to save: the curve with its rate of rise, the milestones and the
/// recipe.
#[must_use]
pub fn to_target(curve: &[CurvePoint], m: &Milestones, actions: &[PlannedAction]) -> ProfileTarget {
    let ror = ror_of(curve);
    let mut points: Vec<TargetPoint> = Vec::with_capacity(curve.len());
    for (p, r) in curve.iter().zip(ror) {
        let t = round1(p.t);
        // Rounding must not make two times equal.
        if points.last().is_some_and(|l| l.roast_time_s >= t) {
            continue;
        }
        points.push(TargetPoint {
            roast_time_s: t,
            bt_c: round1(p.bt),
            ror_c_per_min: r.map(round1),
        });
    }
    let event = |p: CurvePoint| EventTarget {
        roast_time_s: Some(round1(p.t)),
        bt_c: Some(round1(p.bt)),
    };
    ProfileTarget {
        points,
        actions: sort_actions(actions),
        events: EventTargets {
            dry_end: Some(event(m.dry_end)),
            first_crack: Some(event(m.first_crack)),
            drop: Some(DropRule::AtTime {
                roast_time_s: round1(m.drop.t),
            }),
        },
        tolerance: ToleranceBand::default(),
    }
}

/// A stored plan back to its curve and milestones.
#[must_use]
pub fn from_target(target: &ProfileTarget) -> (Vec<CurvePoint>, Milestones) {
    let curve: Vec<CurvePoint> = target
        .points
        .iter()
        .map(|p| CurvePoint {
            t: p.roast_time_s,
            bt: p.bt_c,
        })
        .collect();
    let end = match target.events.drop {
        Some(DropRule::AtTime { roast_time_s }) => roast_time_s,
        _ => curve.last().map_or(0.0, |p| p.t),
    };
    let ev = |e: Option<EventTarget>, fallback_bt: f64| {
        let t = e.and_then(|e| e.roast_time_s);
        match t {
            Some(t) => CurvePoint {
                t,
                bt: e.and_then(|e| e.bt_c).unwrap_or_else(|| bt_at(&curve, t)),
            },
            None => curve
                .iter()
                .find(|p| p.t > 90.0 && p.bt >= fallback_bt)
                .copied()
                .unwrap_or(CurvePoint {
                    t: end * if fallback_bt < 180.0 { 0.45 } else { 0.8 },
                    bt: fallback_bt,
                }),
        }
    };
    let milestones = Milestones {
        charge: curve.first().map_or(200.0, |p| p.bt),
        dry_end: ev(target.events.dry_end, 150.0),
        first_crack: ev(target.events.first_crack, 193.0),
        drop: CurvePoint {
            t: end,
            bt: bt_at(&curve, end),
        },
    };
    (curve, milestones)
}

/// The recipe a roast followed: a step whenever burner or air move to a new
/// value. Changes less than `merge_s` apart count as one move (Artisan logs
/// a slider move as a short ramp), to its last value, in whole percent.
#[must_use]
pub fn recipe_from_roast(roast: &Roast, merge_s: f64) -> Vec<PlannedAction> {
    let Some(charge) = roast.charge_t() else {
        return Vec::new();
    };
    let drop = roast
        .events
        .iter()
        .find(|e| e.kind == EventKind::Drop)
        .map_or(f64::INFINITY, |e| e.t);
    let mut out = Vec::new();
    for actuator in [Actuator::Burner, Actuator::Air] {
        let mut changes: Vec<(f64, f64)> = Vec::new();
        let mut last: Option<f64> = None;
        for s in &roast.samples {
            if s.t < charge - 2.0 || s.t > drop {
                continue;
            }
            let Some(&v) = s.actuators.get(&actuator) else {
                continue;
            };
            if last.is_none_or(|l| (v - l).abs() >= 0.5) {
                changes.push(((s.t - charge).max(0.0), v));
            }
            last = Some(v);
        }
        let mut committed: Option<f64> = None;
        let mut i = 0;
        while i < changes.len() {
            let mut j = i;
            while j + 1 < changes.len() && changes[j + 1].0 - changes[j].0 < merge_s {
                j += 1;
            }
            let v = changes[j].1.round();
            if committed != Some(v) {
                out.push(PlannedAction {
                    trigger: ActionTrigger::Time {
                        roast_time_s: changes[i].0.round(),
                    },
                    actuator,
                    value: v,
                });
                committed = Some(v);
            }
            i = j + 1;
        }
    }
    sort_actions(&out)
}

/// A curve for an empty start: a typical 200 g roast on the Cubean+, shaped
/// by its milestones.
#[must_use]
pub fn template_curve(m: &Milestones) -> Vec<CurvePoint> {
    // Charge to turning point: the probe falls quickly to about 98 °C at 1:10.
    let tp = CurvePoint { t: 70.0, bt: 98.0 };
    let mut pts = Vec::new();
    let end = (-tp.t / 18.0).exp();
    let mut t = 0.0;
    while t < tp.t {
        let f = (-t / 18.0).exp();
        pts.push(CurvePoint {
            t,
            bt: tp.bt + (m.charge - tp.bt) * ((f - end) / (1.0 - end)),
        });
        t += STEP_S;
    }
    // Turning point to drop: a monotone cubic through the milestones, flat at
    // the turning point.
    let knots = [tp, m.dry_end, m.first_crack, m.drop];
    let slope = |a: CurvePoint, b: CurvePoint| (b.bt - a.bt) / (b.t - a.t);
    let d: Vec<f64> = (0..knots.len())
        .map(|i| {
            if i == 0 {
                return 0.0;
            }
            let prev = slope(knots[i - 1], knots[i]);
            if i == knots.len() - 1 {
                return prev;
            }
            let next = slope(knots[i], knots[i + 1]);
            if prev * next <= 0.0 {
                0.0
            } else {
                2.0 * prev * next / (prev + next)
            }
        })
        .collect();
    for i in 0..knots.len() - 1 {
        let (a, b) = (knots[i], knots[i + 1]);
        let h = b.t - a.t;
        let mut t = a.t;
        while t < b.t {
            let s = (t - a.t) / h;
            let h00 = 2.0 * s.powi(3) - 3.0 * s.powi(2) + 1.0;
            let h10 = s.powi(3) - 2.0 * s.powi(2) + s;
            let h01 = -2.0 * s.powi(3) + 3.0 * s.powi(2);
            let h11 = s.powi(3) - s.powi(2);
            pts.push(CurvePoint {
                t,
                bt: h00 * a.bt + h10 * h * d[i] + h01 * b.bt + h11 * h * d[i + 1],
            });
            t += STEP_S;
        }
    }
    pts.push(m.drop);
    pts
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn a_template_passes_its_milestones() {
        let m = TEMPLATE_MILESTONES;
        let c = template_curve(&m);
        for p in [m.dry_end, m.first_crack, m.drop] {
            assert!((bt_at(&c, p.t) - p.bt).abs() < 0.5, "{p:?}");
        }
        assert!((c[0].bt - m.charge).abs() < 0.5);
    }

    #[test]
    fn warping_moves_the_milestones_and_keeps_the_shape() {
        let m = TEMPLATE_MILESTONES;
        let c = template_curve(&m);
        let to = Milestones {
            first_crack: CurvePoint {
                t: 540.0,
                bt: 194.0,
            },
            drop: CurvePoint {
                t: 660.0,
                bt: 205.0,
            },
            ..m
        };
        let w = warp(&c, &m, &to);
        assert!((bt_at(&w, 540.0) - 194.0).abs() < 0.5);
        assert!((bt_at(&w, 660.0) - 205.0).abs() < 0.5);
        assert!((bt_at(&w, m.dry_end.t) - m.dry_end.bt).abs() < 0.5);
        assert_eq!(w.last().unwrap().t, 660.0);
        let target = to_target(&w, &to, &[]);
        let (back, mb) = from_target(&target);
        assert_eq!(mb.first_crack, to.first_crack);
        assert!((mb.drop.t - 660.0).abs() < 1e-9);
        assert!(back.len() > 300);
        assert!(
            target
                .points
                .iter()
                .skip(20)
                .all(|p| p.ror_c_per_min.is_some())
        );
    }

    #[test]
    fn milestones_out_of_order_are_named() {
        let mut m = TEMPLATE_MILESTONES;
        assert_eq!(milestones_problem(&m), None);
        m.drop.t = m.first_crack.t + 10.0;
        assert!(milestones_problem(&m).is_some());
    }
}
