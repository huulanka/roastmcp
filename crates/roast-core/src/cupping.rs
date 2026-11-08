//! Cupping (tasting) records and rest time.

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Attribute scores of a cupping, each `0.0..=10.0` and each optional — score
/// only what you paid attention to.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct CuppingScores {
    /// Fragrance (dry) and aroma (wet).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fragrance_aroma: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub flavor: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub aftertaste: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub acidity: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sweetness: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub balance: Option<f64>,
    /// The overall impression as an attribute (not the total score).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub overall: Option<f64>,
}

impl CuppingScores {
    /// Lowest allowed attribute score.
    pub const MIN: f64 = 0.0;
    /// Highest allowed attribute score.
    pub const MAX: f64 = 10.0;

    /// The attribute scores with their stable names, in display order.
    #[must_use]
    pub fn named(&self) -> [(&'static str, Option<f64>); 8] {
        [
            ("fragrance_aroma", self.fragrance_aroma),
            ("flavor", self.flavor),
            ("aftertaste", self.aftertaste),
            ("acidity", self.acidity),
            ("body", self.body),
            ("sweetness", self.sweetness),
            ("balance", self.balance),
            ("overall", self.overall),
        ]
    }

    /// Mean of the given attribute scores, if any is given.
    #[must_use]
    pub fn mean(&self) -> Option<f64> {
        let given: Vec<f64> = self.named().iter().filter_map(|(_, s)| *s).collect();
        if given.is_empty() {
            None
        } else {
            Some(given.iter().sum::<f64>() / given.len() as f64)
        }
    }
}

/// Why a cupping is not valid.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum CuppingError {
    /// An attribute score outside `0.0..=10.0` (or not a number).
    #[error("score `{field}` is {value}, expected 0 to 10")]
    ScoreOutOfRange { field: &'static str, value: f64 },
    /// The overall score is negative or not a number.
    #[error("overall score is {0}, expected a non-negative number")]
    InvalidOverallScore(f64),
}

/// One tasting of one roast.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Cupping {
    pub id: Uuid,
    pub roast_id: Uuid,
    pub cupped_at: DateTime<Utc>,
    /// Days of rest since the roast. When `None`, it is derived from the roast
    /// date (see [`rest_days_between`]); the store fills it in on reading.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rest_days: Option<u32>,
    #[serde(default)]
    pub scores: CuppingScores,
    /// The total on whatever scale the taster uses (for example SCA 0–100).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub overall_score: Option<f64>,
    /// Flavour descriptors, for example "jasmine", "bergamot", "black tea".
    #[serde(default)]
    pub descriptors: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    /// How it was brewed, for example "cupping bowl", "V60 15 g / 250 g".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub brew_method: Option<String>,
}

impl Cupping {
    /// An empty cupping of a roast with a fresh id.
    #[must_use]
    pub fn new(roast_id: Uuid, cupped_at: DateTime<Utc>) -> Self {
        Cupping {
            id: Uuid::new_v4(),
            roast_id,
            cupped_at,
            rest_days: None,
            scores: CuppingScores::default(),
            overall_score: None,
            descriptors: Vec::new(),
            notes: None,
            brew_method: None,
        }
    }

    /// Rest days as given, or derived from the roast date.
    #[must_use]
    pub fn rest_days_or_derived(&self, roasted_at: DateTime<Utc>) -> u32 {
        self.rest_days
            .unwrap_or_else(|| rest_days_between(roasted_at, self.cupped_at))
    }

    /// Checks score ranges.
    ///
    /// # Errors
    /// The first score that is out of range.
    pub fn validate(&self) -> Result<(), CuppingError> {
        for (field, score) in self.scores.named() {
            match score {
                Some(value) if !(CuppingScores::MIN..=CuppingScores::MAX).contains(&value) => {
                    return Err(CuppingError::ScoreOutOfRange { field, value });
                }
                _ => {}
            }
        }
        match self.overall_score {
            Some(total) if !(total.is_finite() && total >= 0.0) => {
                Err(CuppingError::InvalidOverallScore(total))
            }
            _ => Ok(()),
        }
    }
}

/// Whole days elapsed between a roast and a later moment (24-hour periods,
/// rounded down; never negative).
#[must_use]
pub fn rest_days_between(roasted_at: DateTime<Utc>, at: DateTime<Utc>) -> u32 {
    let days = (at - roasted_at).num_days().max(0);
    u32::try_from(days).unwrap_or(u32::MAX)
}

/// A rough roast level, used to pick a rest window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RoastLevel {
    Light,
    Medium,
    Dark,
}

/// When a roast is ready to drink and until when it is at its best, in days
/// after roasting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct RestWindow {
    /// Ready to drink from this many days after roasting.
    pub min_days: u32,
    /// Past its best after this many days.
    pub max_days: u32,
}

impl RestWindow {
    /// The moment the roast is ready to drink.
    #[must_use]
    pub fn ready_from(&self, roasted_at: DateTime<Utc>) -> DateTime<Utc> {
        roasted_at + Duration::days(i64::from(self.min_days))
    }

    /// The moment the roast is past its best.
    #[must_use]
    pub fn best_until(&self, roasted_at: DateTime<Utc>) -> DateTime<Utc> {
        roasted_at + Duration::days(i64::from(self.max_days))
    }
}

/// Rest windows per roast level. These are conventions, not rules: rest depends
/// on beans, roast, brew method and taste, so every value is meant to be
/// configurable (stored in the settings).
///
/// The defaults are typical recommendations for filter coffee:
///
/// | level  | from | until |
/// |--------|------|-------|
/// | light  | 7 d  | 21 d  |
/// | medium | 4 d  | 14 d  |
/// | dark   | 2 d  | 10 d  |
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct RestWindows {
    pub light: RestWindow,
    pub medium: RestWindow,
    pub dark: RestWindow,
}

impl Default for RestWindows {
    fn default() -> Self {
        RestWindows {
            light: RestWindow {
                min_days: 7,
                max_days: 21,
            },
            medium: RestWindow {
                min_days: 4,
                max_days: 14,
            },
            dark: RestWindow {
                min_days: 2,
                max_days: 10,
            },
        }
    }
}

impl RestWindows {
    /// The window for a roast level.
    #[must_use]
    pub fn window(&self, level: RoastLevel) -> RestWindow {
        match level {
            RoastLevel::Light => self.light,
            RoastLevel::Medium => self.medium,
            RoastLevel::Dark => self.dark,
        }
    }
}

/// The default rest window `(min_days, max_days)` for a roast level, from
/// [`RestWindows::default`]. Conventions for filter coffee, not rules — use a
/// configured [`RestWindows`] where the person has set their own.
#[must_use]
pub fn rest_window(roast_degree_hint: RoastLevel) -> (u32, u32) {
    let w = RestWindows::default().window(roast_degree_hint);
    (w.min_days, w.max_days)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_rest_windows() {
        assert_eq!(rest_window(RoastLevel::Light), (7, 21));
        assert_eq!(rest_window(RoastLevel::Medium), (4, 14));
        assert_eq!(rest_window(RoastLevel::Dark), (2, 10));
    }

    #[test]
    fn rest_days_are_whole_elapsed_days() {
        let roasted = DateTime::<Utc>::UNIX_EPOCH;
        assert_eq!(rest_days_between(roasted, roasted + Duration::hours(47)), 1);
        assert_eq!(rest_days_between(roasted, roasted + Duration::hours(48)), 2);
        assert_eq!(rest_days_between(roasted, roasted - Duration::hours(5)), 0);
        let c = Cupping::new(Uuid::nil(), roasted + Duration::days(9));
        assert_eq!(c.rest_days_or_derived(roasted), 9);
    }

    #[test]
    fn validate_rejects_out_of_range_scores() {
        let mut c = Cupping::new(Uuid::nil(), DateTime::UNIX_EPOCH);
        c.scores.acidity = Some(8.5);
        assert!(c.validate().is_ok());
        c.scores.body = Some(11.0);
        assert_eq!(
            c.validate(),
            Err(CuppingError::ScoreOutOfRange {
                field: "body",
                value: 11.0
            })
        );
        c.scores.body = Some(f64::NAN);
        assert!(c.validate().is_err());
    }

    #[test]
    fn mean_of_given_scores() {
        let scores = CuppingScores {
            flavor: Some(8.0),
            acidity: Some(7.0),
            ..CuppingScores::default()
        };
        assert_eq!(scores.mean(), Some(7.5));
        assert_eq!(CuppingScores::default().mean(), None);
    }

    #[test]
    fn ready_dates() {
        let roasted = DateTime::<Utc>::UNIX_EPOCH;
        let w = RestWindows::default().window(RoastLevel::Medium);
        assert_eq!(w.ready_from(roasted), roasted + Duration::days(4));
        assert_eq!(w.best_until(roasted), roasted + Duration::days(14));
    }
}
