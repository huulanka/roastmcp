//! Where first crack is expected: the bean temperature it usually comes at
//! and the window in which a heard crack is believed (ADR 0021).

use serde::Serialize;

/// First crack is only believed between these bean temperatures, °C. The
/// roaster's 25 real roasts on the Cubean+ cracked at 175–203 °C.
pub const FIRST_CRACK_WINDOW_C: (f64, f64) = (170.0, 205.0);
/// The typical first-crack temperature when there are no roasts to learn from, °C.
pub const FIRST_CRACK_TYPICAL_C: f64 = 192.0;
/// Half width of a lot's window from one first crack a person marked, °C …
pub const LOT_MARGIN_ONE_C: f64 = 10.0;
/// … and from two or more (around their median), °C.
pub const LOT_MARGIN_C: f64 = 8.0;

/// What the expectation is based on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CrackBasis {
    /// Earlier roasts of this green coffee lot.
    Lot,
    /// Earlier roasts on this machine, any coffee.
    Roasts,
    /// No roasts with a marked first crack: a typical value.
    Typical,
}

/// Where first crack is expected in this roast.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct CrackExpectation {
    /// The bean temperature first crack usually starts at, °C.
    pub bt: f64,
    /// A heard first crack below this is not believed, °C.
    pub low: f64,
    /// … nor above this, °C.
    pub high: f64,
    /// How many roasts `bt` comes from.
    pub roasts: usize,
    pub basis: CrackBasis,
    /// Highest minus lowest of those first cracks, °C (0 with one).
    #[serde(default)]
    pub spread_c: f64,
}

/// One earlier roast's first crack: its bean temperature, and whether a
/// person (or an AI) marked it rather than only the microphone.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CrackMark {
    pub bt: f64,
    pub by_person: bool,
}

impl CrackExpectation {
    /// The expectation from the first-crack temperatures of earlier roasts
    /// of the same lot and of all earlier roasts (implausible values are
    /// left out), all taken as marked by a person.
    #[must_use]
    pub fn from_roasts(lot: &[f64], all: &[f64]) -> Self {
        let marks = |v: &[f64]| -> Vec<CrackMark> {
            v.iter()
                .map(|&bt| CrackMark {
                    bt,
                    by_person: true,
                })
                .collect()
        };
        Self::from_marks(&marks(lot), &marks(all))
    }

    /// The expectation from earlier first cracks: the **median** of the
    /// lot's, else of all roasts', else a typical value. First cracks a
    /// person marked count; those only the microphone heard count only when
    /// there are no others (a heard mark may be off, and one early mark
    /// moved the mean of a small lot by degrees). The window narrows around
    /// a lot's median: ± 10 °C from one person's mark, ± 8 °C from two or
    /// more, never outside 170–205 °C.
    #[must_use]
    pub fn from_marks(lot: &[CrackMark], all: &[CrackMark]) -> Self {
        let (lo, hi) = FIRST_CRACK_WINDOW_C;
        let pick = |v: &[CrackMark]| -> (Vec<f64>, bool) {
            let plausible: Vec<CrackMark> = v
                .iter()
                .copied()
                .filter(|m| m.bt.is_finite() && (lo..=hi).contains(&m.bt))
                .collect();
            let by_person: Vec<f64> = plausible
                .iter()
                .filter(|m| m.by_person)
                .map(|m| m.bt)
                .collect();
            if by_person.is_empty() {
                (plausible.iter().map(|m| m.bt).collect(), false)
            } else {
                (by_person, true)
            }
        };
        let (lot, lot_by_person) = pick(lot);
        let (all, _) = pick(all);
        let (values, basis) = if !lot.is_empty() {
            (lot, CrackBasis::Lot)
        } else if !all.is_empty() {
            (all, CrackBasis::Roasts)
        } else {
            return CrackExpectation {
                bt: FIRST_CRACK_TYPICAL_C,
                low: lo,
                high: hi,
                roasts: 0,
                basis: CrackBasis::Typical,
                spread_c: 0.0,
            };
        };
        let bt = median(&values);
        let spread_c = values.iter().copied().fold(f64::MIN, f64::max)
            - values.iter().copied().fold(f64::MAX, f64::min);
        let margin = match values.len() {
            _ if basis != CrackBasis::Lot || !lot_by_person => None,
            1 => Some(LOT_MARGIN_ONE_C),
            _ => Some(LOT_MARGIN_C),
        };
        let (low, high) = margin.map_or((lo, hi), |m| {
            ((bt - m).clamp(lo, hi), (bt + m).clamp(lo, hi))
        });
        CrackExpectation {
            bt,
            low,
            high,
            roasts: values.len(),
            basis,
            spread_c,
        }
    }

    /// Whether a crack heard at this bean temperature is believed.
    #[must_use]
    pub fn believable(&self, bt: f64) -> bool {
        (self.low..=self.high).contains(&bt)
    }
}

/// The median of `v` (not empty).
fn median(v: &[f64]) -> f64 {
    let mut v = v.to_vec();
    v.sort_by(f64::total_cmp);
    let n = v.len();
    if n % 2 == 1 {
        v[n / 2]
    } else {
        f64::midpoint(v[n / 2 - 1], v[n / 2])
    }
}

#[cfg(test)]
#[allow(clippy::float_cmp)]
mod tests {
    use super::*;

    #[test]
    fn without_roasts_the_typical_value_and_the_wide_window() {
        let e = CrackExpectation::from_roasts(&[], &[]);
        assert_eq!(e.basis, CrackBasis::Typical);
        assert_eq!(e.bt, FIRST_CRACK_TYPICAL_C);
        assert_eq!((e.low, e.high), FIRST_CRACK_WINDOW_C);
        assert!(e.believable(171.0));
        assert!(!e.believable(150.0));
        assert!(!e.believable(210.0));
    }

    #[test]
    fn all_roasts_give_the_median_but_keep_the_wide_window() {
        let e = CrackExpectation::from_roasts(&[], &[190.0, 194.0, 120.0, 201.0]);
        assert_eq!(e.basis, CrackBasis::Roasts);
        assert_eq!(e.bt, 194.0);
        assert_eq!(e.roasts, 3, "the implausible 120 °C is left out");
        assert_eq!(e.spread_c, 11.0);
        assert_eq!((e.low, e.high), FIRST_CRACK_WINDOW_C);
    }

    #[test]
    fn a_lots_marks_narrow_the_window_around_their_median() {
        // BR012 and BRYB-003: 190.9 and 194.9 °C.
        let two = CrackExpectation::from_roasts(&[190.9, 194.9], &[185.0]);
        assert_eq!(two.basis, CrackBasis::Lot);
        assert!((two.bt - 192.9).abs() < 1e-9);
        assert!((two.low - 184.9).abs() < 1e-9 && (two.high - 200.9).abs() < 1e-9);
        assert!(
            !two.believable(179.8),
            "BRYB-003's false heard crack at 6:48"
        );

        let one = CrackExpectation::from_roasts(&[190.9], &[]);
        assert!((one.low - 180.9).abs() < 1e-9 && (one.high - 200.9).abs() < 1e-9);

        let high = CrackExpectation::from_roasts(&[198.0, 200.0, 201.0], &[]);
        assert_eq!(high.bt, 200.0);
        assert_eq!(high.high, 205.0, "never outside the wide window");
    }

    #[test]
    fn a_crack_only_the_microphone_heard_counts_only_without_others() {
        let person = |bt| CrackMark {
            bt,
            by_person: true,
        };
        let heard = |bt| CrackMark {
            bt,
            by_person: false,
        };
        let e = CrackExpectation::from_marks(&[person(194.0), heard(180.0)], &[]);
        assert_eq!((e.bt, e.roasts), (194.0, 1));
        // Only heard ones: used, but they do not narrow the window.
        let e = CrackExpectation::from_marks(&[heard(193.0)], &[]);
        assert_eq!((e.bt, e.basis), (193.0, CrackBasis::Lot));
        assert_eq!((e.low, e.high), FIRST_CRACK_WINDOW_C);
    }
}
