//! When to drop when the plan names both a drop temperature and a
//! development time ratio (ADR 0014), and the two do not agree.
//!
//! From first crack on, both say when the roast should end: the temperature
//! when the bean temperature will reach it, the ratio at
//! `first crack / (1 − DTR)`. On a roast with too much energy late (BRYB-003,
//! 2026-11-18) the temperature came at a DTR of 14 % while the plan asked for
//! 19 %. The person chooses which wins; by default the earlier one, because
//! it keeps the roast from going darker than planned.

use serde::{Deserialize, Serialize};

/// Which target ends the roast when the plan has both.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DropBy {
    /// Whichever comes first: never darker than planned.
    #[default]
    Earlier,
    /// Only when both are reached.
    Later,
    /// The drop temperature.
    Bt,
    /// The development time ratio.
    Dtr,
}

/// The two targets differ by more than this before it counts as a conflict, s.
pub const CONFLICT_S: f64 = 20.0;

/// Where the two targets lie in this roast. Times are seconds since CHARGE.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct DropOutlook {
    /// When the bean temperature is expected at the drop temperature.
    pub bt_at_s: Option<f64>,
    /// The DTR at that moment, %.
    pub dtr_at_bt_pct: Option<f64>,
    /// When the development time ratio reaches its target.
    pub dtr_at_s: Option<f64>,
    /// The bean temperature expected then, °C.
    pub bt_at_dtr_c: Option<f64>,
    /// When the roast should end, by the rule.
    pub drop_at_s: Option<f64>,
    /// The rule that decided.
    pub by: DropBy,
    /// The two lie more than [`CONFLICT_S`] apart.
    pub conflict: bool,
}

/// What the outlook looks at.
#[derive(Debug, Clone, Copy)]
pub struct DropInput {
    /// Seconds since CHARGE now.
    pub since: f64,
    /// First crack, seconds since CHARGE.
    pub first_crack_s: f64,
    /// The plan's drop temperature, °C.
    pub drop_bt_c: Option<f64>,
    /// The plan's development time ratio, %.
    pub dtr_pct: Option<f64>,
    pub by: DropBy,
}

/// The outlook from first crack on. `bt_ahead(h)` is the bean temperature
/// expected `h` s from now and `reaches(target)` the seconds until it gets
/// there, if it does (see [`crate::reference::Projector`]).
#[must_use]
pub fn drop_outlook(
    input: &DropInput,
    bt_ahead: impl Fn(f64) -> f64,
    reaches: impl Fn(f64) -> Option<f64>,
) -> DropOutlook {
    let fc = input.first_crack_s;
    let bt_at_s = input.drop_bt_c.and_then(&reaches).map(|h| input.since + h);
    let dtr_at_bt_pct = bt_at_s.filter(|&t| t > fc).map(|t| (t - fc) / t * 100.0);
    let dtr_at_s = input
        .dtr_pct
        .filter(|d| (0.0..100.0).contains(d))
        .map(|d| fc / (1.0 - d / 100.0));
    let bt_at_dtr_c = dtr_at_s.map(|t| bt_ahead((t - input.since).max(0.0)));
    let drop_at_s = match (bt_at_s, dtr_at_s, input.by) {
        (Some(b), Some(d), DropBy::Earlier) => Some(b.min(d)),
        (Some(b), Some(d), DropBy::Later) => Some(b.max(d)),
        (Some(b), _, DropBy::Bt) | (Some(b), None, _) => Some(b),
        (_, Some(d), DropBy::Dtr) | (None, Some(d), _) => Some(d),
        (None, None, _) => None,
    };
    let conflict = bt_at_s
        .zip(dtr_at_s)
        .is_some_and(|(b, d)| (b - d).abs() > CONFLICT_S);
    DropOutlook {
        bt_at_s,
        dtr_at_bt_pct,
        dtr_at_s,
        bt_at_dtr_c,
        drop_at_s,
        by: input.by,
        conflict,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// BRYB-003 at 9:00: first crack at 8:33, 197.9 °C rising 6.8 °C/min.
    fn bryb003(by: DropBy) -> DropOutlook {
        let input = DropInput {
            since: 540.0,
            first_crack_s: 513.0,
            drop_bt_c: Some(203.0),
            dtr_pct: Some(19.0),
            by,
        };
        let ahead = |h: f64| 197.9 + 6.8 * h / 60.0;
        let reaches = |target: f64| Some((target - 197.9) / 6.8 * 60.0);
        drop_outlook(&input, ahead, reaches)
    }

    #[test]
    fn the_temperature_comes_long_before_the_ratio_and_that_is_a_conflict() {
        let o = bryb003(DropBy::Earlier);
        let bt = o.bt_at_s.unwrap();
        assert!((bt - 585.0).abs() < 1.0, "{bt}");
        assert!((o.dtr_at_bt_pct.unwrap() - 12.3).abs() < 0.2);
        let dtr = o.dtr_at_s.unwrap();
        assert!((dtr - 633.3).abs() < 0.1, "{dtr}");
        assert!((o.bt_at_dtr_c.unwrap() - 208.5).abs() < 0.2);
        assert!(o.conflict);
        assert_eq!(o.drop_at_s, Some(bt));
    }

    #[test]
    fn the_rule_picks_the_target() {
        let (bt, dtr) = (585.0, 633.3);
        let at = |by| bryb003(by).drop_at_s.unwrap();
        assert!((at(DropBy::Later) - dtr).abs() < 0.1);
        assert!((at(DropBy::Dtr) - dtr).abs() < 0.1);
        assert!((at(DropBy::Bt) - bt).abs() < 1.0);
    }

    #[test]
    fn with_one_target_it_is_that_one_and_no_conflict() {
        let input = DropInput {
            since: 540.0,
            first_crack_s: 513.0,
            drop_bt_c: None,
            dtr_pct: Some(19.0),
            by: DropBy::Bt,
        };
        let o = drop_outlook(&input, |_| 200.0, |_| None);
        assert!((o.drop_at_s.unwrap() - 633.3).abs() < 0.1);
        assert!(!o.conflict);
    }
}
