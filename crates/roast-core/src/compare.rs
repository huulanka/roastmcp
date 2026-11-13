//! The figures of a finished roast, the comparison of two roasts and the
//! consistency of a group of roasts (the same lot, the same profile).
//!
//! Everything here reads saved roasts only; nothing is live.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{EventKind, Roast, RorConfig, phases, ror_series};

/// What a comparison lines two roasts up on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Alignment {
    /// Both clocks start at CHARGE.
    #[default]
    Charge,
    /// Both clocks are zero at the start of first crack, so the development
    /// of two roasts lies on top of each other.
    FirstCrack,
}

/// The figures of a roast that comparisons and statistics use. Times are
/// seconds since CHARGE.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RoastFigures {
    pub id: Uuid,
    /// Bean temperature at CHARGE, °C.
    pub charge_bt_c: Option<f64>,
    pub turning_point_s: Option<f64>,
    pub turning_point_bt_c: Option<f64>,
    pub dry_end_s: Option<f64>,
    pub dry_end_bt_c: Option<f64>,
    pub first_crack_s: Option<f64>,
    pub first_crack_bt_c: Option<f64>,
    /// CHARGE to DROP.
    pub total_s: Option<f64>,
    pub drop_bt_c: Option<f64>,
    /// First crack to DROP.
    pub development_s: Option<f64>,
    /// Development time in percent of the roast time.
    pub dtr_pct: Option<f64>,
    pub weight_loss_pct: Option<f64>,
    /// Roast colour of the whole beans, on the roast's own scale.
    pub colour_whole: Option<f64>,
    /// Roast colour of the ground coffee.
    pub colour_ground: Option<f64>,
}

/// The figures of `roast`. A roast without CHARGE has only its weights and
/// colour; one without DROP has no total, drop or development figures.
#[must_use]
pub fn figures(roast: &Roast) -> RoastFigures {
    let charge = roast.charge_t();
    let since = |k: EventKind| {
        let e = roast.event(k)?;
        Some((e.t - charge?, e.bt))
    };
    let time = |k| since(k).map(|(t, _)| t).filter(|t| *t >= 0.0);
    let bt = |k| since(k).and_then(|(_, bt)| bt);
    let dropped = roast.event(EventKind::Drop).is_some();
    let ph = if dropped {
        roast.samples.last().and_then(|last| phases(roast, last.t))
    } else {
        None
    };
    RoastFigures {
        id: roast.id,
        charge_bt_c: roast.event(EventKind::Charge).and_then(|e| e.bt),
        turning_point_s: time(EventKind::TurningPoint),
        turning_point_bt_c: bt(EventKind::TurningPoint),
        dry_end_s: time(EventKind::DryEnd),
        dry_end_bt_c: bt(EventKind::DryEnd),
        first_crack_s: time(EventKind::FirstCrackStart),
        first_crack_bt_c: bt(EventKind::FirstCrackStart),
        total_s: ph.map(|p| p.total_s),
        drop_bt_c: bt(EventKind::Drop),
        development_s: ph.and_then(|p| p.development_s),
        dtr_pct: ph.and_then(|p| p.dtr_pct),
        weight_loss_pct: roast.weight_loss_pct(),
        colour_whole: roast.roast_degree.as_ref().and_then(|d| d.whole_bean),
        colour_ground: roast.roast_degree.as_ref().and_then(|d| d.ground),
    }
}

/// `a − b` for every figure both roasts have.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct FigureDeltas {
    pub charge_bt_c: Option<f64>,
    pub turning_point_s: Option<f64>,
    pub turning_point_bt_c: Option<f64>,
    pub dry_end_s: Option<f64>,
    pub first_crack_s: Option<f64>,
    pub first_crack_bt_c: Option<f64>,
    pub total_s: Option<f64>,
    pub drop_bt_c: Option<f64>,
    pub development_s: Option<f64>,
    pub dtr_pct: Option<f64>,
    pub weight_loss_pct: Option<f64>,
    pub colour_whole: Option<f64>,
    pub colour_ground: Option<f64>,
}

fn sub(a: Option<f64>, b: Option<f64>) -> Option<f64> {
    Some(a? - b?)
}

/// One step of two curves side by side.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct CurveRow {
    /// Seconds on the shared clock: since CHARGE, or relative to the start of
    /// first crack (negative before it).
    pub t: f64,
    pub a_bt: Option<f64>,
    pub b_bt: Option<f64>,
    /// `a_bt − b_bt`.
    pub delta_bt: Option<f64>,
    /// Rate of rise, °C/min.
    pub a_ror: Option<f64>,
    pub b_ror: Option<f64>,
}

/// Two roasts side by side.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Comparison {
    /// What the curves are lined up on. [`Alignment::Charge`] when first
    /// crack was asked for but one of the roasts has none.
    pub align: Alignment,
    pub a: RoastFigures,
    pub b: RoastFigures,
    /// `a − b`.
    pub delta: FigureDeltas,
    /// Both curves from CHARGE to DROP on the shared clock, every `step_s`.
    pub curve: Vec<CurveRow>,
    /// The largest `|a_bt − b_bt|` after both turning points, °C.
    pub max_delta_bt: Option<f64>,
}

/// How far past its first and last reading a curve still counts, seconds
/// (about two sampling steps).
const EDGE_S: f64 = 3.0;

/// A roast's curve from CHARGE to DROP on a clock with its zero at `zero`
/// (a recording time), with its rate of rise.
struct Track {
    /// `(t, bt, ror)`, sorted by `t`.
    points: Vec<(f64, f64, Option<f64>)>,
    /// From here on the curve only rises: the turning point, on the track's
    /// clock.
    settled: f64,
}

impl Track {
    fn new(roast: &Roast, zero: f64) -> Option<Self> {
        let charge = roast.charge_t()?;
        let end = roast.event(EventKind::Drop).map_or(f64::INFINITY, |e| e.t);
        let turned = roast.event(EventKind::TurningPoint).map_or(charge, |e| e.t);
        let ror = ror_series(&roast.samples, &RorConfig::default());
        let points: Vec<_> = roast
            .samples
            .iter()
            .zip(ror)
            .filter(|(s, _)| s.t >= charge && s.t <= end)
            // Until the probe has turned, its rate of rise says nothing
            // about the beans.
            .filter_map(|(s, r)| Some((s.t - zero, s.bt?, r.filter(|_| s.t >= turned))))
            .collect();
        if points.len() < 2 {
            return None;
        }
        Some(Track {
            points,
            settled: turned - zero,
        })
    }

    fn span(&self) -> (f64, f64) {
        (
            self.points.first().map_or(0.0, |p| p.0),
            self.points.last().map_or(0.0, |p| p.0),
        )
    }

    /// Bean temperature and rate of rise at `t`, interpolated; `None`
    /// outside the curve. Within [`EDGE_S`] of its ends the curve counts as
    /// reaching `t`: the first reading comes a moment after CHARGE.
    fn at(&self, t: f64) -> Option<(f64, Option<f64>)> {
        let (from, to) = self.span();
        if t < from - EDGE_S || t > to + EDGE_S {
            return None;
        }
        let t = t.clamp(from, to);
        let i = self.points.partition_point(|p| p.0 < t);
        let hi = self.points.get(i).or(self.points.last())?;
        let lo = if i == 0 { hi } else { &self.points[i - 1] };
        let w = if hi.0 > lo.0 {
            ((t - lo.0) / (hi.0 - lo.0)).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let bt = lo.1 + (hi.1 - lo.1) * w;
        let ror = match (lo.2, hi.2) {
            (Some(a), Some(b)) => Some(a + (b - a) * w),
            (a, b) => a.or(b),
        };
        Some((bt, ror))
    }
}

/// Compares roast `a` with roast `b`: their figures, the differences
/// `a − b` and both curves every `step_s` seconds (at least 1 s) on a clock
/// lined up on `align`.
#[must_use]
pub fn compare(a: &Roast, b: &Roast, align: Alignment, step_s: f64) -> Comparison {
    let (fa, fb) = (figures(a), figures(b));
    let fc = |r: &Roast| r.event(EventKind::FirstCrackStart).map(|e| e.t);
    let (align, zero_a, zero_b) = match (align, fc(a), fc(b)) {
        (Alignment::FirstCrack, Some(za), Some(zb)) => (Alignment::FirstCrack, Some(za), Some(zb)),
        _ => (Alignment::Charge, a.charge_t(), b.charge_t()),
    };
    let ta = zero_a.and_then(|z| Track::new(a, z));
    let tb = zero_b.and_then(|z| Track::new(b, z));
    let step = step_s.max(1.0);
    let mut curve = Vec::new();
    let mut max_delta_bt: Option<f64> = None;
    let spans: Vec<(f64, f64)> = [&ta, &tb].into_iter().flatten().map(Track::span).collect();
    if let (Some(from), Some(to)) = (
        spans.iter().map(|s| s.0).min_by(f64::total_cmp),
        spans.iter().map(|s| s.1).max_by(f64::total_cmp),
    ) {
        let settled = [&ta, &tb]
            .into_iter()
            .flatten()
            .map(|t| t.settled)
            .fold(f64::NEG_INFINITY, f64::max);
        // Rows on whole steps of the shared clock, so zero is a row.
        let mut k = (from / step).ceil();
        while k * step <= to + 1e-9 {
            let t = k * step;
            let pa = ta.as_ref().and_then(|x| x.at(t));
            let pb = tb.as_ref().and_then(|x| x.at(t));
            let delta_bt = sub(pa.map(|p| p.0), pb.map(|p| p.0));
            if let Some(d) = delta_bt
                && t >= settled
            {
                max_delta_bt = Some(max_delta_bt.map_or(d.abs(), |m| m.max(d.abs())));
            }
            curve.push(CurveRow {
                t,
                a_bt: pa.map(|p| p.0),
                b_bt: pb.map(|p| p.0),
                delta_bt,
                a_ror: pa.and_then(|p| p.1),
                b_ror: pb.and_then(|p| p.1),
            });
            k += 1.0;
        }
    }
    let delta = FigureDeltas {
        charge_bt_c: sub(fa.charge_bt_c, fb.charge_bt_c),
        turning_point_s: sub(fa.turning_point_s, fb.turning_point_s),
        turning_point_bt_c: sub(fa.turning_point_bt_c, fb.turning_point_bt_c),
        dry_end_s: sub(fa.dry_end_s, fb.dry_end_s),
        first_crack_s: sub(fa.first_crack_s, fb.first_crack_s),
        first_crack_bt_c: sub(fa.first_crack_bt_c, fb.first_crack_bt_c),
        total_s: sub(fa.total_s, fb.total_s),
        drop_bt_c: sub(fa.drop_bt_c, fb.drop_bt_c),
        development_s: sub(fa.development_s, fb.development_s),
        dtr_pct: sub(fa.dtr_pct, fb.dtr_pct),
        weight_loss_pct: sub(fa.weight_loss_pct, fb.weight_loss_pct),
        colour_whole: sub(fa.colour_whole, fb.colour_whole),
        colour_ground: sub(fa.colour_ground, fb.colour_ground),
    };
    Comparison {
        align,
        a: fa,
        b: fb,
        delta,
        curve,
        max_delta_bt,
    }
}

/// How a figure spreads over a group of roasts.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Spread {
    /// How many roasts have the figure.
    pub n: usize,
    pub mean: f64,
    /// Sample standard deviation; 0 for a single roast.
    pub sd: f64,
    pub min: f64,
    pub max: f64,
}

/// The spread of `values`, or `None` when there are none.
#[must_use]
pub fn spread(values: impl IntoIterator<Item = f64>) -> Option<Spread> {
    let v: Vec<f64> = values.into_iter().filter(|x| x.is_finite()).collect();
    let n = v.len();
    if n == 0 {
        return None;
    }
    let mean = v.iter().sum::<f64>() / n as f64;
    let sd = if n > 1 {
        (v.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / (n - 1) as f64).sqrt()
    } else {
        0.0
    };
    Some(Spread {
        n,
        mean,
        sd,
        min: v.iter().copied().fold(f64::INFINITY, f64::min),
        max: v.iter().copied().fold(f64::NEG_INFINITY, f64::max),
    })
}

/// How alike a group of roasts is: the spread of each figure, and of the
/// curves themselves.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Consistency {
    /// How many roasts were looked at.
    pub roasts: usize,
    pub charge_bt_c: Option<Spread>,
    pub dry_end_s: Option<Spread>,
    pub first_crack_s: Option<Spread>,
    pub first_crack_bt_c: Option<Spread>,
    pub total_s: Option<Spread>,
    pub drop_bt_c: Option<Spread>,
    pub development_s: Option<Spread>,
    pub dtr_pct: Option<Spread>,
    pub weight_loss_pct: Option<Spread>,
    /// How far the curves lie apart, °C: the standard deviation of the bean
    /// temperature across the roasts, averaged (root mean square) over the
    /// time all of them were between turning point and DROP. `None` with
    /// fewer than two such curves.
    pub curve_sd_c: Option<f64>,
}

/// The consistency of `roasts`. Roasts without CHARGE add nothing but their
/// weights.
#[must_use]
pub fn consistency(roasts: &[&Roast]) -> Consistency {
    let figs: Vec<RoastFigures> = roasts.iter().map(|r| figures(r)).collect();
    let of = |pick: fn(&RoastFigures) -> Option<f64>| spread(figs.iter().filter_map(pick));
    Consistency {
        roasts: roasts.len(),
        charge_bt_c: of(|f| f.charge_bt_c),
        dry_end_s: of(|f| f.dry_end_s),
        first_crack_s: of(|f| f.first_crack_s),
        first_crack_bt_c: of(|f| f.first_crack_bt_c),
        total_s: of(|f| f.total_s),
        drop_bt_c: of(|f| f.drop_bt_c),
        development_s: of(|f| f.development_s),
        dtr_pct: of(|f| f.dtr_pct),
        weight_loss_pct: of(|f| f.weight_loss_pct),
        curve_sd_c: curve_sd(roasts),
    }
}

/// Seconds between the points at which the curves of a group are compared.
const CURVE_STEP_S: f64 = 15.0;

fn curve_sd(roasts: &[&Roast]) -> Option<f64> {
    let tracks: Vec<Track> = roasts
        .iter()
        .filter(|r| r.event(EventKind::Drop).is_some())
        .filter_map(|r| Track::new(r, r.charge_t()?))
        .collect();
    if tracks.len() < 2 {
        return None;
    }
    let from = tracks
        .iter()
        .map(|t| t.settled.max(t.span().0))
        .fold(f64::NEG_INFINITY, f64::max);
    let to = tracks
        .iter()
        .map(|t| t.span().1)
        .fold(f64::INFINITY, f64::min);
    let mut squares = Vec::new();
    let mut k = (from / CURVE_STEP_S).ceil();
    while k * CURVE_STEP_S <= to + 1e-9 {
        let t = k * CURVE_STEP_S;
        if let Some(s) = spread(tracks.iter().filter_map(|x| x.at(t).map(|p| p.0)))
            && s.n == tracks.len()
        {
            squares.push(s.sd * s.sd);
        }
        k += 1.0;
    }
    if squares.is_empty() {
        return None;
    }
    Some((squares.iter().sum::<f64>() / squares.len() as f64).sqrt())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use crate::testutil::{self, CHARGE_T, DROP_ROAST_T};
    use crate::{ColorScale, RoastDegree};

    /// The synthetic roast, `bt_off` °C hotter and its events `shift_s` later
    /// on the clock (the curve itself moves with them).
    fn variant(bt_off: f64, shift_s: f64) -> Roast {
        let mut r = testutil::roast();
        r.id = Uuid::new_v4();
        for s in &mut r.samples {
            s.t += shift_s;
            s.bt = s.bt.map(|b| b + bt_off);
        }
        for e in &mut r.events {
            e.t += shift_s;
            e.bt = e.bt.map(|b| b + bt_off);
        }
        r
    }

    #[test]
    fn figures_of_the_synthetic_roast() {
        let mut r = testutil::roast();
        r.roast_degree = Some(RoastDegree {
            scale: ColorScale::Agtron,
            whole_bean: Some(62.0),
            ground: Some(71.0),
        });
        let f = figures(&r);
        assert_eq!(f.charge_bt_c, Some(200.0));
        assert!((f.total_s.unwrap() - DROP_ROAST_T).abs() < 1e-9);
        assert!((f.drop_bt_c.unwrap() - 205.0).abs() < 0.5);
        let fc = f.first_crack_s.unwrap();
        assert!((fc - testutil::crossing(196.0)).abs() < 1e-6);
        assert!((f.development_s.unwrap() - (DROP_ROAST_T - fc)).abs() < 1e-6);
        assert!((f.dtr_pct.unwrap() - (DROP_ROAST_T - fc) / DROP_ROAST_T * 100.0).abs() < 1e-6);
        assert!((f.weight_loss_pct.unwrap() - 15.0).abs() < 1e-9);
        assert_eq!((f.colour_whole, f.colour_ground), (Some(62.0), Some(71.0)));

        // Without DROP there is no total and no development; without CHARGE
        // no times at all.
        r.events.retain(|e| e.kind != EventKind::Drop);
        let f = figures(&r);
        assert_eq!((f.total_s, f.development_s, f.dtr_pct), (None, None, None));
        assert!(f.first_crack_s.is_some());
        r.events.retain(|e| e.kind != EventKind::Charge);
        let f = figures(&r);
        assert_eq!((f.first_crack_s, f.charge_bt_c), (None, None));
        assert!(f.weight_loss_pct.is_some());
    }

    #[test]
    fn a_roast_compared_with_a_hotter_one() {
        let a = variant(3.0, 0.0);
        let b = variant(0.0, 12.0); // the same roast, recorded 12 s later
        let c = compare(&a, &b, Alignment::Charge, 30.0);
        assert_eq!(c.align, Alignment::Charge);
        // Both start at CHARGE, whatever the recording clock said.
        assert_eq!(c.curve.first().unwrap().t, 0.0);
        assert!((c.curve.last().unwrap().t - DROP_ROAST_T).abs() < 1e-9);
        assert_eq!(c.curve.len(), 21);
        for row in &c.curve {
            assert!((row.delta_bt.unwrap() - 3.0).abs() < 1e-6, "{row:?}");
        }
        assert!((c.max_delta_bt.unwrap() - 3.0).abs() < 1e-6);
        assert!((c.delta.drop_bt_c.unwrap() - 3.0).abs() < 1e-6);
        assert!(c.delta.total_s.unwrap().abs() < 1e-9);
        // No rate of rise before the turning point.
        assert_eq!((c.curve[0].a_ror, c.curve[1].a_ror), (None, None));
        // The rate of rise is known once the window has filled.
        let mid = &c.curve[10];
        assert!((mid.a_ror.unwrap() - testutil::roast_ror(300.0)).abs() < 0.5);
        assert!((mid.a_ror.unwrap() - mid.b_ror.unwrap()).abs() < 0.2);
    }

    #[test]
    fn lined_up_on_first_crack_the_development_lies_on_top() {
        let a = testutil::roast();
        // The same roast with first crack marked 40 s earlier.
        let mut b = testutil::roast();
        b.id = Uuid::new_v4();
        for e in &mut b.events {
            if e.kind == EventKind::FirstCrackStart {
                e.t -= 40.0;
            }
        }
        let c = compare(&a, &b, Alignment::FirstCrack, 10.0);
        assert_eq!(c.align, Alignment::FirstCrack);
        assert!((c.delta.first_crack_s.unwrap() - 40.0).abs() < 1e-9);
        // Zero is a row, and there a is where b was 40 s earlier.
        let zero = c.curve.iter().find(|r| r.t == 0.0).unwrap();
        let fc = a.event(EventKind::FirstCrackStart).unwrap().t - CHARGE_T;
        let want = testutil::roast_bt(fc) - testutil::roast_bt(fc - 40.0);
        assert!((zero.delta_bt.unwrap() - want).abs() < 0.05, "{zero:?}");
        // Before first crack the clock is negative; a's curve reaches further back.
        let first = c.curve.first().unwrap();
        assert!(first.t < -400.0 && first.a_bt.is_some() && first.b_bt.is_none());

        // A roast without first crack falls back to CHARGE.
        b.events.retain(|e| e.kind != EventKind::FirstCrackStart);
        assert_eq!(
            compare(&a, &b, Alignment::FirstCrack, 10.0).align,
            Alignment::Charge
        );
    }

    #[test]
    fn spread_of_a_few_values() {
        assert_eq!(spread([]), None);
        let one = spread([5.0]).unwrap();
        assert_eq!((one.n, one.mean, one.sd), (1, 5.0, 0.0));
        let s = spread([2.0, 4.0, 4.0, 4.0, 5.0, 5.0, 7.0, 9.0]).unwrap();
        assert_eq!((s.n, s.mean, s.min, s.max), (8, 5.0, 2.0, 9.0));
        assert!((s.sd - (32.0_f64 / 7.0).sqrt()).abs() < 1e-12);
        // A figure that is not a number is left out.
        assert_eq!(spread([1.0, f64::NAN]).unwrap().n, 1);
    }

    #[test]
    fn consistency_of_a_group() {
        let (a, b, c) = (variant(-2.0, 0.0), variant(0.0, 5.0), variant(2.0, 0.0));
        let k = consistency(&[&a, &b, &c]);
        assert_eq!(k.roasts, 3);
        let drop = k.drop_bt_c.unwrap();
        assert_eq!(drop.n, 3);
        assert!((drop.sd - 2.0).abs() < 1e-6 && (drop.max - drop.min - 4.0).abs() < 1e-6);
        // The same curve at three heights: the same times, 2 °C apart.
        assert!(k.total_s.unwrap().sd.abs() < 1e-9);
        assert!((k.curve_sd_c.unwrap() - 2.0).abs() < 1e-6);
        assert!(k.weight_loss_pct.unwrap().sd.abs() < 1e-9);

        // One roast alone has no curve spread, and nothing has no figures.
        assert_eq!(consistency(&[&a]).curve_sd_c, None);
        let none = consistency(&[]);
        assert_eq!((none.roasts, none.drop_bt_c), (0, None));
    }
}
