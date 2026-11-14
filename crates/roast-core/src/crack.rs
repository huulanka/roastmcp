//! Where first crack is expected: the bean temperature it usually comes at
//! and the window in which a heard crack is believed (ADR 0021).

use serde::Serialize;

/// First crack is only believed between these bean temperatures, °C. The
/// roaster's 25 real roasts on the Cubean+ cracked at 175–203 °C.
pub const FIRST_CRACK_WINDOW_C: (f64, f64) = (170.0, 205.0);
/// The typical first-crack temperature when there are no roasts to learn from, °C.
pub const FIRST_CRACK_TYPICAL_C: f64 = 192.0;
/// From this many roasts of a lot on, the window narrows to the lot's mean
/// ± [`LOT_MARGIN_C`].
pub const LOT_ROASTS_FOR_WINDOW: usize = 3;
/// Half width of a lot's window, °C.
pub const LOT_MARGIN_C: f64 = 12.0;

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
}

impl CrackExpectation {
    /// The expectation from the first-crack temperatures of earlier roasts
    /// of the same lot and of all earlier roasts (implausible values are
    /// left out).
    #[must_use]
    pub fn from_roasts(lot: &[f64], all: &[f64]) -> Self {
        let (lo, hi) = FIRST_CRACK_WINDOW_C;
        let plausible = |v: &[f64]| -> Vec<f64> {
            v.iter()
                .copied()
                .filter(|b| b.is_finite() && (lo..=hi).contains(b))
                .collect()
        };
        let mean = |v: &[f64]| v.iter().sum::<f64>() / v.len() as f64;
        let lot = plausible(lot);
        let all = plausible(all);
        let (bt, roasts, basis) = if !lot.is_empty() {
            (mean(&lot), lot.len(), CrackBasis::Lot)
        } else if !all.is_empty() {
            (mean(&all), all.len(), CrackBasis::Roasts)
        } else {
            (FIRST_CRACK_TYPICAL_C, 0, CrackBasis::Typical)
        };
        let (low, high) = if basis == CrackBasis::Lot && roasts >= LOT_ROASTS_FOR_WINDOW {
            (
                (bt - LOT_MARGIN_C).clamp(lo, hi),
                (bt + LOT_MARGIN_C).clamp(lo, hi),
            )
        } else {
            (lo, hi)
        };
        CrackExpectation {
            bt,
            low,
            high,
            roasts,
            basis,
        }
    }

    /// Whether a crack heard at this bean temperature is believed.
    #[must_use]
    pub fn believable(&self, bt: f64) -> bool {
        (self.low..=self.high).contains(&bt)
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
    fn all_roasts_give_the_mean_but_keep_the_wide_window() {
        let e = CrackExpectation::from_roasts(&[], &[190.0, 194.0, 120.0]);
        assert_eq!(e.basis, CrackBasis::Roasts);
        assert_eq!(e.bt, 192.0);
        assert_eq!(e.roasts, 2, "the implausible 120 °C is left out");
        assert_eq!((e.low, e.high), FIRST_CRACK_WINDOW_C);
    }

    #[test]
    fn a_lot_with_three_roasts_narrows_the_window() {
        let two = CrackExpectation::from_roasts(&[196.0, 198.0], &[185.0]);
        assert_eq!(two.basis, CrackBasis::Lot);
        assert_eq!(two.bt, 197.0);
        assert_eq!((two.low, two.high), FIRST_CRACK_WINDOW_C);

        let three = CrackExpectation::from_roasts(&[196.0, 198.0, 200.0], &[185.0]);
        assert_eq!(three.bt, 198.0);
        assert_eq!(three.low, 186.0);
        assert_eq!(three.high, 205.0, "never outside the wide window");
        assert!(!three.believable(180.0));
    }
}
