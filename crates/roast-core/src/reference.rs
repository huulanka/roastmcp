//! Reference curves and short-term projection.
//!
//! A reference is an earlier roast shown behind the live curve. Both are
//! aligned on CHARGE, so "the same moment" means the same time since CHARGE.
//! The projection extrapolates the bean temperature from the current rate of
//! rise and its trend; it is a guide for the next minute or two, not a model
//! of the machine (that is the digital twin's job).

use serde::Serialize;
use uuid::Uuid;

use crate::event::{EventKind, EventOrigin, RoastEvent};
use crate::profile::{DropRule, EventTarget, PlannedAction, Profile, ProfileVersion};
use crate::roast::Roast;
use crate::ror::{RorConfig, ror_series};

/// One point of a reference curve, `t` in seconds since its CHARGE.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct RefPoint {
    pub t: f64,
    pub bt: Option<f64>,
    pub ror: Option<f64>,
}

/// What a reference curve comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReferenceKind {
    /// An earlier roast.
    Roast,
    /// A profile version's target curve (the plan).
    Profile,
}

/// An earlier roast or a profile's plan prepared as a reference: times
/// relative to its CHARGE.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ReferenceCurve {
    pub id: Uuid,
    pub title: String,
    pub kind: ReferenceKind,
    pub points: Vec<RefPoint>,
    /// Events with `t` relative to CHARGE.
    pub events: Vec<RoastEvent>,
    /// A profile's recipe: burner and air steps (empty for a roast).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub actions: Vec<PlannedAction>,
}

impl ReferenceCurve {
    /// Prepares a roast as a reference. Without a CHARGE event the recording
    /// start counts as CHARGE.
    #[must_use]
    pub fn from_roast(roast: &Roast, ror: &RorConfig) -> Self {
        let charge = roast.charge_t().unwrap_or(0.0);
        let rors = ror_series(&roast.samples, ror);
        // From the last sample before CHARGE, so the curve covers t = 0.
        let first = roast
            .samples
            .partition_point(|s| s.t < charge)
            .saturating_sub(1);
        let points = roast
            .samples
            .iter()
            .zip(rors)
            .skip(first)
            .map(|(s, r)| RefPoint {
                t: s.t - charge,
                bt: s.bt,
                ror: r,
            })
            .collect();
        let events = roast
            .events
            .iter()
            .map(|e| RoastEvent {
                t: e.t - charge,
                ..e.clone()
            })
            .collect();
        ReferenceCurve {
            id: roast.id,
            title: roast.title.clone(),
            kind: ReferenceKind::Roast,
            points,
            events,
            actions: Vec::new(),
        }
    }

    /// A profile version as the plan line: its target curve, and its dry end,
    /// first crack and (timed) drop as events. The id is the version's.
    #[must_use]
    pub fn from_profile(profile: &Profile, version: &ProfileVersion) -> Self {
        let target = &version.target;
        let points = target
            .points
            .iter()
            .map(|p| RefPoint {
                t: p.roast_time_s,
                bt: Some(p.bt_c),
                ror: p.ror_c_per_min,
            })
            .collect();
        let event = |kind, e: Option<EventTarget>| {
            let e = e?;
            Some(RoastEvent {
                t: e.roast_time_s?,
                kind,
                origin: EventOrigin::Human,
                bt: e.bt_c,
            })
        };
        let drop = match target.events.drop {
            Some(DropRule::AtTime { roast_time_s }) => Some(EventTarget {
                roast_time_s: Some(roast_time_s),
                bt_c: None,
            }),
            _ => None,
        };
        let mut events: Vec<RoastEvent> = [
            event(
                EventKind::Charge,
                Some(EventTarget {
                    roast_time_s: Some(0.0),
                    bt_c: target.points.first().map(|p| p.bt_c),
                }),
            ),
            event(EventKind::DryEnd, target.events.dry_end),
            event(EventKind::FirstCrackStart, target.events.first_crack),
            event(EventKind::Drop, drop),
        ]
        .into_iter()
        .flatten()
        .collect();
        events.sort_by(|a, b| a.t.total_cmp(&b.t));
        ReferenceCurve {
            id: version.id,
            title: format!("{} · v{}", profile.name, version.version),
            kind: ReferenceKind::Profile,
            points,
            events,
            actions: target.actions.clone(),
        }
    }

    /// Bean temperature and RoR at `t` seconds since CHARGE (linear
    /// interpolation; `None` outside the curve).
    #[must_use]
    pub fn at(&self, t: f64) -> (Option<f64>, Option<f64>) {
        let i = self.points.partition_point(|p| p.t < t);
        let (Some(b), Some(a)) = (self.points.get(i), i.checked_sub(1).map(|j| self.points[j]))
        else {
            // Exactly on the first point, or outside.
            return match self.points.get(i) {
                Some(p) if (p.t - t).abs() < 1e-9 => (p.bt, p.ror),
                _ => (None, None),
            };
        };
        let f = if b.t > a.t {
            (t - a.t) / (b.t - a.t)
        } else {
            0.0
        };
        let lerp = |x: Option<f64>, y: Option<f64>| match (x, y) {
            (Some(x), Some(y)) => Some(x + (y - x) * f),
            _ => None,
        };
        (lerp(a.bt, b.bt), lerp(a.ror, b.ror))
    }

    /// The reference's event of a kind.
    #[must_use]
    pub fn event(&self, kind: EventKind) -> Option<&RoastEvent> {
        self.events.iter().find(|e| e.kind == kind)
    }
}

/// The next milestone the roast is heading for.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Milestone {
    pub kind: EventKind,
    /// Bean temperature at which it is expected.
    pub bt: f64,
    /// Seconds until the projected curve reaches `bt`; `None` if it does not
    /// within the horizon.
    pub eta_s: Option<f64>,
}

/// Where the bean temperature is heading.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Projection {
    /// Projected bean temperature in 60 s.
    pub bt_60s: Option<f64>,
    pub next: Option<Milestone>,
}

/// How far ahead the projection looks for milestones, seconds.
const HORIZON_S: f64 = 900.0;
/// The window over which the RoR trend is estimated, seconds.
const TREND_WINDOW_S: f64 = 60.0;

/// Projects the bean temperature from the recent curve.
///
/// `recent` holds `(t, bt, ror)` of the live curve in time order (any time
/// base). The RoR is assumed to keep changing at its recent rate (never
/// rising: a projection that assumes a rising RoR overshoots), and the bean
/// temperature follows it; the RoR does not go below zero. Returns `None`
/// before the RoR is known or while it is not positive (before the turning
/// point).
#[must_use]
pub fn project(
    recent: &[(f64, f64, f64)],
    events: &[RoastEvent],
    reference: Option<&ReferenceCurve>,
) -> Option<Projection> {
    let &(t_now, bt, ror) = recent.last()?;
    if ror <= 0.0 {
        return None;
    }
    let trend = ror_trend(recent, t_now);
    let bt_at = |h: f64| -> f64 {
        // RoR (°C/min) falls linearly at `trend` (°C/min per s) until zero.
        let h_zero = if trend < 0.0 {
            -ror / trend
        } else {
            f64::INFINITY
        };
        let h = h.min(h_zero);
        bt + (ror * h + 0.5 * trend * h * h) / 60.0
    };
    // A milestone whose temperature is already passed is not shown.
    let next = reference
        .and_then(|r| next_milestone(events, r))
        .filter(|&(_, target)| bt < target)
        .map(|(kind, target)| Milestone {
            kind,
            bt: target,
            eta_s: (1..=HORIZON_S as u32)
                .map(f64::from)
                .find(|&h| bt_at(h) >= target),
        });
    Some(Projection {
        bt_60s: Some(bt_at(60.0)),
        next,
    })
}

/// Least-squares slope of the RoR over the last minute, clamped to ≤ 0.
fn ror_trend(recent: &[(f64, f64, f64)], t_now: f64) -> f64 {
    let window: Vec<(f64, f64)> = recent
        .iter()
        .filter(|(t, _, _)| *t >= t_now - TREND_WINDOW_S)
        .map(|&(t, _, r)| (t, r))
        .collect();
    if window.len() < 3 {
        return 0.0;
    }
    let n = window.len() as f64;
    let mt = window.iter().map(|w| w.0).sum::<f64>() / n;
    let mr = window.iter().map(|w| w.1).sum::<f64>() / n;
    let cov: f64 = window.iter().map(|w| (w.0 - mt) * (w.1 - mr)).sum();
    let var: f64 = window.iter().map(|w| (w.0 - mt).powi(2)).sum();
    if var <= f64::EPSILON {
        return 0.0;
    }
    (cov / var).min(0.0)
}

/// First crack (until marked), then DROP, at the reference's temperatures.
fn next_milestone(events: &[RoastEvent], reference: &ReferenceCurve) -> Option<(EventKind, f64)> {
    let has = |k: EventKind| events.iter().any(|e| e.kind == k);
    if has(EventKind::Drop) || !has(EventKind::Charge) {
        return None;
    }
    let kind = if has(EventKind::FirstCrackStart) {
        EventKind::Drop
    } else {
        EventKind::FirstCrackStart
    };
    let r = reference.event(kind)?;
    let bt = r.bt.or_else(|| reference.at(r.t).0)?;
    Some((kind, bt))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::float_cmp)]
mod tests {
    use super::*;
    use crate::event::EventOrigin;
    use crate::testutil;

    fn ev(kind: EventKind, t: f64, bt: f64) -> RoastEvent {
        RoastEvent {
            t,
            kind,
            origin: EventOrigin::Human,
            bt: Some(bt),
        }
    }

    #[test]
    fn reference_is_aligned_on_charge_and_interpolates() {
        let roast = testutil::roast();
        let charge = roast.charge_t().unwrap();
        let r = ReferenceCurve::from_roast(&roast, &RorConfig::default());
        assert!(r.points.first().unwrap().t <= 0.0);
        assert!(r.at(0.0).0.is_some());
        assert_eq!(r.event(EventKind::Charge).unwrap().t, 0.0);
        // Halfway between two samples lies halfway between their values.
        let (a, b) = (r.points[10], r.points[11]);
        let (bt, _) = r.at((a.t + b.t) / 2.0);
        assert!((bt.unwrap() - (a.bt.unwrap() + b.bt.unwrap()) / 2.0).abs() < 1e-9);
        assert_eq!(r.at(-5.0), (None, None));
        assert_eq!(r.at(1e9), (None, None));
        let sample_at_charge = roast.samples.iter().find(|s| s.t >= charge).unwrap();
        assert_eq!(r.at(sample_at_charge.t - charge).0, sample_at_charge.bt);
    }

    #[test]
    fn steady_ror_projects_a_straight_line() {
        // 10 °C/min, constant: +10 °C in 60 s.
        let recent: Vec<_> = (0..60)
            .map(|i| (f64::from(i), 150.0 + f64::from(i) / 6.0, 10.0))
            .collect();
        let p = project(&recent, &[], None).unwrap();
        let last = recent.last().unwrap().1;
        assert!((p.bt_60s.unwrap() - (last + 10.0)).abs() < 1e-6);
        assert!(p.next.is_none());
    }

    #[test]
    fn falling_ror_bends_the_projection_and_never_rises() {
        // RoR falls 1 °C/min every 10 s: it reaches zero in 60 s from 6.
        let recent: Vec<_> = (0..=60)
            .map(|i| {
                let t = f64::from(i);
                (t, 190.0, 12.0 - t / 10.0)
            })
            .collect();
        let p = project(&recent, &[], None).unwrap();
        // Area under a RoR falling from 6 to 0 over 60 s: 6/2 °C.
        assert!((p.bt_60s.unwrap() - 193.0).abs() < 1e-6);

        let rising: Vec<_> = (0..=60)
            .map(|i| (f64::from(i), 150.0, 5.0 + f64::from(i) / 10.0))
            .collect();
        let p = project(&rising, &[], None).unwrap();
        // A rising RoR is held, not extrapolated: 11 °C/min for 60 s.
        assert!((p.bt_60s.unwrap() - 161.0).abs() < 1e-6);
    }

    #[test]
    fn milestones_come_from_the_reference() {
        let mut roast = testutil::roast();
        roast.events = vec![
            ev(EventKind::Charge, 0.0, 180.0),
            ev(EventKind::FirstCrackStart, 450.0, 196.0),
            ev(EventKind::Drop, 540.0, 205.0),
        ];
        let reference = ReferenceCurve::from_roast(&roast, &RorConfig::default());
        let recent: Vec<_> = (0..=30)
            .map(|i| (400.0 + f64::from(i), 190.0, 6.0))
            .collect();
        let live = [ev(EventKind::Charge, 0.0, 180.0)];
        let p = project(&recent, &live, Some(&reference)).unwrap();
        let next = p.next.unwrap();
        assert_eq!(next.kind, EventKind::FirstCrackStart);
        assert_eq!(next.bt, 196.0);
        // 6 °C at 6 °C/min: one minute.
        assert_eq!(next.eta_s, Some(60.0));

        let live = [
            ev(EventKind::Charge, 0.0, 180.0),
            ev(EventKind::FirstCrackStart, 420.0, 195.0),
        ];
        let next = project(&recent, &live, Some(&reference))
            .unwrap()
            .next
            .unwrap();
        assert_eq!(next.kind, EventKind::Drop);
        assert_eq!(next.bt, 205.0);

        let live = [ev(EventKind::Drop, 500.0, 204.0)];
        assert!(
            project(&recent, &live, Some(&reference))
                .unwrap()
                .next
                .is_none()
        );
    }

    #[test]
    fn no_projection_before_the_turning_point() {
        assert!(project(&[(10.0, 120.0, -15.0)], &[], None).is_none());
        assert!(project(&[], &[], None).is_none());
    }

    #[test]
    fn a_profile_version_becomes_the_plan_line() {
        use crate::profile::{EventTargets, ProfileTarget, TargetPoint};
        let profile = Profile::new("Brasilien Espresso", chrono::Utc::now());
        let version = ProfileVersion {
            id: Uuid::new_v4(),
            profile_id: profile.id,
            version: 2,
            parent_version: Some(1),
            created_at: chrono::Utc::now(),
            change_note: None,
            target: ProfileTarget {
                points: vec![
                    TargetPoint {
                        roast_time_s: 0.0,
                        bt_c: 200.0,
                        ror_c_per_min: None,
                    },
                    TargetPoint {
                        roast_time_s: 541.0,
                        bt_c: 194.0,
                        ror_c_per_min: Some(7.0),
                    },
                    TargetPoint {
                        roast_time_s: 631.0,
                        bt_c: 205.0,
                        ror_c_per_min: Some(6.0),
                    },
                ],
                actions: Vec::new(),
                events: EventTargets {
                    dry_end: None,
                    first_crack: Some(EventTarget {
                        bt_c: Some(194.0),
                        roast_time_s: Some(541.0),
                    }),
                    drop: Some(DropRule::AtTime {
                        roast_time_s: 631.0,
                    }),
                },
                tolerance: Default::default(),
            },
        };
        let r = ReferenceCurve::from_profile(&profile, &version);
        assert_eq!(r.id, version.id);
        assert_eq!(r.title, "Brasilien Espresso · v2");
        assert_eq!(r.points.len(), 3);
        let kinds: Vec<EventKind> = r.events.iter().map(|e| e.kind).collect();
        assert_eq!(
            kinds,
            [
                EventKind::Charge,
                EventKind::FirstCrackStart,
                EventKind::Drop
            ]
        );
        assert_eq!(r.at(541.0).0, Some(194.0));
    }
}
