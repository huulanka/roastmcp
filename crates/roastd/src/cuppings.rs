//! Cuppings (tastings of a roast) and rest time: when a roast is ready to
//! drink and until when it is at its best.
//!
//! Cuppings live in the database beside their roast. The rest windows per
//! roast level are a setting; the level of a roast comes from its measured
//! colour and is taken as medium without one.

use std::collections::HashMap;
use std::sync::Arc;

use chrono::{DateTime, Duration, Utc};
use roast_core::{Cupping, CuppingScores, RestWindow, RestWindows, Roast, RoastLevel};
use roast_store::Store;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::engine::Fail;
use crate::files;

/// The setting the rest windows are stored under.
const REST_WINDOWS_SETTING: &str = "rest_windows";

/// A roast stays in the rest overview this long after it is past its best.
const PAST_BEST_SHOWN_DAYS: i64 = 14;

/// The longest rest window that can be set, in days.
const MAX_REST_DAYS: u32 = 365;

/// The most flavour descriptors of one cupping.
const MAX_DESCRIPTORS: usize = 30;

/// Body of `POST /api/roasts/{id}/cuppings`.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct NewCupping {
    /// When it was tasted; now if left out.
    #[serde(default)]
    pub cupped_at: Option<DateTime<Utc>>,
    /// Days of rest, if not simply the days since the roast.
    #[serde(default)]
    pub rest_days: Option<u32>,
    #[serde(default)]
    pub scores: CuppingScores,
    #[serde(default)]
    pub overall_score: Option<f64>,
    #[serde(default)]
    pub descriptors: Vec<String>,
    #[serde(default)]
    pub notes: Option<String>,
    #[serde(default)]
    pub brew_method: Option<String>,
}

/// Body of `PATCH /api/cuppings/{id}`; what is left out stays.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct CuppingPatch {
    #[serde(default)]
    pub cupped_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub rest_days: Option<u32>,
    /// Replaces all attribute scores.
    #[serde(default)]
    pub scores: Option<CuppingScores>,
    #[serde(default)]
    pub overall_score: Option<f64>,
    /// Replaces all descriptors.
    #[serde(default)]
    pub descriptors: Option<Vec<String>>,
    /// Empty text removes the notes.
    #[serde(default)]
    pub notes: Option<String>,
    /// Empty text removes the brew method.
    #[serde(default)]
    pub brew_method: Option<String>,
}

/// A cupping as the API gives it: with its rest days filled in and the
/// roast it belongs to named.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CuppingView {
    #[serde(flatten)]
    pub cupping: Cupping,
    /// Mean of the attribute scores given.
    pub mean_score: Option<f64>,
    pub roast_title: String,
    pub batch_no: Option<String>,
    pub lot_id: Option<Uuid>,
}

/// Where a roast's level in the rest overview comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LevelFrom {
    /// Its measured colour.
    Colour,
    /// No colour was measured: taken as medium.
    Default,
}

/// Where a roast is in its rest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RestState {
    /// Not ready to drink yet.
    Resting,
    /// In its window.
    Ready,
    /// Past its best.
    PastBest,
}

/// One roast in the rest overview.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RestEntry {
    pub roast_id: Uuid,
    pub title: String,
    pub batch_no: Option<String>,
    pub beans: Option<String>,
    pub roasted_at: DateTime<Utc>,
    pub level: RoastLevel,
    pub level_from: LevelFrom,
    /// Whole days since the roast.
    pub rest_days: u32,
    pub ready_from: DateTime<Utc>,
    pub best_until: DateTime<Utc>,
    pub state: RestState,
    /// How often it was cupped.
    pub cuppings: usize,
}

fn db(e: roast_store::Error) -> Fail {
    Fail::internal(format!("the database failed: {e}"))
}

fn text(s: Option<String>) -> Option<String> {
    s.map(|t| t.trim().to_owned()).filter(|t| !t.is_empty())
}

fn descriptors(given: Vec<String>) -> Result<Vec<String>, Fail> {
    let mut out: Vec<String> = Vec::new();
    for d in given {
        let d = d.trim();
        if !d.is_empty() && !out.iter().any(|o| o.eq_ignore_ascii_case(d)) {
            out.push(d.to_owned());
        }
    }
    if out.len() > MAX_DESCRIPTORS {
        return Err(Fail::bad(format!(
            "at most {MAX_DESCRIPTORS} descriptors per cupping"
        )));
    }
    Ok(out)
}

/// What the listing needs to know of a cupping's roast.
#[derive(Debug, Clone)]
struct RoastLine {
    title: String,
    roasted_at: DateTime<Utc>,
    batch_no: Option<String>,
    lot_id: Option<Uuid>,
}

impl RoastLine {
    fn of(roast: &Roast) -> Self {
        RoastLine {
            title: roast.title.clone(),
            roasted_at: roast.roasted_at,
            batch_no: files::batch_no(roast),
            lot_id: roast.green_lot_id,
        }
    }

    /// From a line of the archive listing.
    fn of_summary(v: &serde_json::Value) -> Option<(Uuid, Self)> {
        let s = |k: &str| v.get(k).and_then(serde_json::Value::as_str);
        Some((
            s("id")?.parse().ok()?,
            RoastLine {
                title: s("title")?.to_owned(),
                roasted_at: s("roasted_at")?.parse().ok()?,
                batch_no: s("batch_no").map(str::to_owned),
                lot_id: s("lot_id").and_then(|l| l.parse().ok()),
            },
        ))
    }
}

fn view(mut cupping: Cupping, roast: &RoastLine) -> CuppingView {
    cupping.rest_days = Some(cupping.rest_days_or_derived(roast.roasted_at));
    CuppingView {
        mean_score: cupping.scores.mean(),
        roast_title: roast.title.clone(),
        batch_no: roast.batch_no.clone(),
        lot_id: roast.lot_id,
        cupping,
    }
}

fn check_windows(w: &RestWindows) -> Result<(), Fail> {
    for (name, RestWindow { min_days, max_days }) in
        [("light", w.light), ("medium", w.medium), ("dark", w.dark)]
    {
        if min_days > max_days {
            return Err(Fail::bad(format!(
                "{name}: ready from day {min_days} is after its best until day {max_days}"
            )));
        }
        if max_days > MAX_REST_DAYS {
            return Err(Fail::bad(format!(
                "{name}: a rest window ends after {MAX_REST_DAYS} days at the latest"
            )));
        }
    }
    Ok(())
}

/// The cuppings in the database and the rest of the roasts.
#[derive(Debug, Clone)]
pub struct Cuppings {
    store: Arc<Store>,
}

impl Cuppings {
    /// The cuppings in `store`.
    #[must_use]
    pub fn new(store: Arc<Store>) -> Self {
        Cuppings { store }
    }

    fn roast(&self, id: Uuid) -> Result<Roast, Fail> {
        self.store
            .roast(id)
            .map_err(db)?
            .map(|s| s.roast)
            .ok_or_else(|| Fail::not_found("no such roast"))
    }

    fn stored(&self, id: Uuid) -> Result<Cupping, Fail> {
        self.store
            .cupping(id)
            .map_err(db)?
            .ok_or_else(|| Fail::not_found("no such cupping"))
    }

    fn save(&self, cupping: Cupping, roast: &Roast) -> Result<CuppingView, Fail> {
        cupping.validate().map_err(|e| Fail::bad(e.to_string()))?;
        if cupping.cupped_at < roast.roasted_at {
            return Err(Fail::bad("a roast cannot be cupped before it was roasted"));
        }
        self.store.save_cupping(&cupping).map_err(db)?;
        Ok(view(cupping, &RoastLine::of(roast)))
    }

    /// The cuppings of one roast, of the roasts of one lot, or all of them;
    /// newest first.
    ///
    /// # Errors
    /// For an unknown roast or on a database error.
    pub fn list(&self, roast: Option<Uuid>, lot: Option<Uuid>) -> Result<Vec<CuppingView>, Fail> {
        if let Some(id) = roast {
            let line = RoastLine::of(&self.roast(id)?);
            let cuppings = self.store.cuppings(Some(id)).map_err(db)?;
            return Ok(cuppings.into_iter().map(|c| view(c, &line)).collect());
        }
        let roasts: HashMap<Uuid, RoastLine> = self
            .store
            .roast_summaries()
            .map_err(db)?
            .iter()
            .filter_map(RoastLine::of_summary)
            .collect();
        Ok(self
            .store
            .cuppings(None)
            .map_err(db)?
            .into_iter()
            .filter_map(|c| {
                let line = roasts.get(&c.roast_id)?;
                (lot.is_none() || line.lot_id == lot).then(|| view(c, line))
            })
            .collect())
    }

    /// Writes a tasting of a roast down.
    ///
    /// # Errors
    /// For an unknown roast, a score out of range, a tasting before the
    /// roast, or on a database error.
    pub fn create(
        &self,
        roast: Uuid,
        new: NewCupping,
        now: DateTime<Utc>,
    ) -> Result<CuppingView, Fail> {
        let roast = self.roast(roast)?;
        let mut c = Cupping::new(roast.id, new.cupped_at.unwrap_or(now));
        c.rest_days = new.rest_days;
        c.scores = new.scores;
        c.overall_score = new.overall_score;
        c.descriptors = descriptors(new.descriptors)?;
        c.notes = text(new.notes);
        c.brew_method = text(new.brew_method);
        self.save(c, &roast)
    }

    /// Changes a cupping.
    ///
    /// # Errors
    /// For an unknown cupping, a score out of range, a tasting before the
    /// roast, or on a database error.
    pub fn update(&self, id: Uuid, p: CuppingPatch) -> Result<CuppingView, Fail> {
        let mut c = self.stored(id)?;
        let roast = self.roast(c.roast_id)?;
        if let Some(at) = p.cupped_at {
            c.cupped_at = at;
        }
        if let Some(days) = p.rest_days {
            c.rest_days = Some(days);
        }
        if let Some(scores) = p.scores {
            c.scores = scores;
        }
        if let Some(total) = p.overall_score {
            c.overall_score = Some(total);
        }
        if let Some(d) = p.descriptors {
            c.descriptors = descriptors(d)?;
        }
        if let Some(n) = p.notes {
            c.notes = text(Some(n));
        }
        if let Some(b) = p.brew_method {
            c.brew_method = text(Some(b));
        }
        self.save(c, &roast)
    }

    /// Removes a cupping.
    ///
    /// # Errors
    /// For an unknown cupping or on a database error.
    pub fn delete(&self, id: Uuid) -> Result<(), Fail> {
        if self.store.delete_cupping(id).map_err(db)? {
            Ok(())
        } else {
            Err(Fail::not_found("no such cupping"))
        }
    }

    /// The rest windows per roast level: the person's, or the defaults.
    ///
    /// # Errors
    /// On a database error.
    pub fn rest_windows(&self) -> Result<RestWindows, Fail> {
        Ok(self
            .store
            .setting(REST_WINDOWS_SETTING)
            .map_err(db)?
            .unwrap_or_default())
    }

    /// Sets the rest windows.
    ///
    /// # Errors
    /// When a window ends before it starts or is longer than a year, or on
    /// a database error.
    pub fn set_rest_windows(&self, w: RestWindows) -> Result<RestWindows, Fail> {
        check_windows(&w)?;
        self.store
            .set_setting(REST_WINDOWS_SETTING, &w)
            .map_err(db)?;
        Ok(w)
    }

    /// Where the recent roasts are in their rest, newest first: every roast
    /// that is resting or ready, and those past their best for two more
    /// weeks. Simulator roasts only with `simulated`.
    ///
    /// # Errors
    /// On a database error.
    pub fn rest(&self, now: DateTime<Utc>, simulated: bool) -> Result<Vec<RestEntry>, Fail> {
        let windows = self.rest_windows()?;
        let longest = [windows.light, windows.medium, windows.dark]
            .iter()
            .map(|w| w.max_days)
            .max()
            .unwrap_or(0);
        let from = now - Duration::days(i64::from(longest) + PAST_BEST_SHOWN_DAYS);
        let mut cupped: HashMap<Uuid, usize> = HashMap::new();
        for c in self.store.cuppings(None).map_err(db)? {
            *cupped.entry(c.roast_id).or_default() += 1;
        }
        Ok(self
            .store
            .roasts_since(from)
            .map_err(db)?
            .into_iter()
            .filter(|r| simulated || !matches!(r.origin, roast_core::RoastOrigin::Simulated))
            .filter_map(|r| {
                let measured = r.roast_degree.as_ref().and_then(|d| d.level());
                let level = measured.unwrap_or(RoastLevel::Medium);
                let window = windows.window(level);
                let (ready_from, best_until) = (
                    window.ready_from(r.roasted_at),
                    window.best_until(r.roasted_at),
                );
                if now > best_until + Duration::days(PAST_BEST_SHOWN_DAYS) {
                    return None;
                }
                Some(RestEntry {
                    roast_id: r.id,
                    batch_no: files::batch_no(&r),
                    beans: r.batch.beans.clone(),
                    roasted_at: r.roasted_at,
                    level,
                    level_from: if measured.is_some() {
                        LevelFrom::Colour
                    } else {
                        LevelFrom::Default
                    },
                    rest_days: roast_core::rest_days_between(r.roasted_at, now),
                    ready_from,
                    best_until,
                    state: if now < ready_from {
                        RestState::Resting
                    } else if now <= best_until {
                        RestState::Ready
                    } else {
                        RestState::PastBest
                    },
                    cuppings: cupped.get(&r.id).copied().unwrap_or(0),
                    title: r.title,
                })
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use crate::files::Archive;
    use roast_core::{ColorScale, RoastDegree};

    fn at(t: &str) -> DateTime<Utc> {
        t.parse().unwrap()
    }

    fn roast(title: &str, roasted_at: &str) -> Roast {
        serde_json::from_value(serde_json::json!({
            "id": Uuid::new_v4(),
            "title": title,
            "roasted_at": roasted_at,
            "sampling_interval_s": 1.0,
            "batch": {"batch_no": "BRYB-002"},
            "origin": {"kind": "recorded"},
            "samples": [{"t": 0.0, "bt": 180.0}, {"t": 1.0, "bt": 179.0}],
        }))
        .unwrap()
    }

    fn setup() -> (tempfile::TempDir, Archive, Cuppings) {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(Store::in_memory().unwrap());
        let archive = Archive::new(store.clone(), dir.path().join("roasts"));
        (dir, archive, Cuppings::new(store))
    }

    #[test]
    fn a_cupping_is_written_down_changed_listed_and_removed() {
        let (_dir, archive, x) = setup();
        let lot = Uuid::new_v4();
        let mut a = roast("a", "2026-11-01T18:00:00Z");
        a.green_lot_id = Some(lot);
        let b = roast("b", "2026-11-02T18:00:00Z");
        archive.save(&a).unwrap();
        archive.save(&b).unwrap();
        let now = at("2026-11-09T08:00:00Z");

        let new = |flavor: f64| NewCupping {
            scores: CuppingScores {
                flavor: Some(flavor),
                acidity: Some(7.0),
                ..CuppingScores::default()
            },
            descriptors: vec![" jasmine ".into(), "Jasmine".into(), String::new()],
            notes: Some("  clean, a little thin ".into()),
            ..NewCupping::default()
        };
        assert_eq!(
            x.create(Uuid::new_v4(), new(8.0), now).unwrap_err().status,
            404
        );
        assert_eq!(x.create(a.id, new(11.0), now).unwrap_err().status, 400);
        let early = NewCupping {
            cupped_at: Some(at("2026-10-31T08:00:00Z")),
            ..new(8.0)
        };
        assert_eq!(x.create(a.id, early, now).unwrap_err().status, 400);

        let c = x.create(a.id, new(8.0), now).unwrap();
        assert_eq!(c.cupping.rest_days, Some(7));
        assert_eq!(c.mean_score, Some(7.5));
        assert_eq!(c.cupping.descriptors, ["jasmine"]);
        assert_eq!(c.cupping.notes.as_deref(), Some("clean, a little thin"));
        assert_eq!(c.roast_title, "a");
        assert_eq!(c.batch_no.as_deref(), Some("BRYB-002"));
        let other = x.create(b.id, new(6.0), now).unwrap();

        let c = x
            .update(
                c.cupping.id,
                CuppingPatch {
                    overall_score: Some(86.5),
                    notes: Some(" ".into()),
                    brew_method: Some("V60 15 g / 250 g".into()),
                    ..CuppingPatch::default()
                },
            )
            .unwrap();
        assert_eq!(c.cupping.overall_score, Some(86.5));
        assert_eq!(c.cupping.notes, None);
        assert_eq!(c.cupping.scores.flavor, Some(8.0));
        assert_eq!(
            x.update(Uuid::new_v4(), CuppingPatch::default())
                .unwrap_err()
                .status,
            404
        );

        assert_eq!(x.list(Some(a.id), None).unwrap(), std::slice::from_ref(&c));
        assert_eq!(x.list(None, Some(lot)).unwrap(), std::slice::from_ref(&c));
        assert_eq!(x.list(None, None).unwrap().len(), 2);
        assert_eq!(x.list(Some(Uuid::new_v4()), None).unwrap_err().status, 404);

        x.delete(other.cupping.id).unwrap();
        assert_eq!(x.delete(other.cupping.id).unwrap_err().status, 404);
        // A deleted roast takes its cuppings with it.
        archive.trash(a.id).unwrap();
        assert!(x.list(None, None).unwrap().is_empty());
    }

    #[test]
    fn roasts_rest_become_ready_and_pass_their_best() {
        let (_dir, archive, x) = setup();
        let now = at("2026-11-20T12:00:00Z");
        let fresh = roast("fresh", "2026-11-18T12:00:00Z");
        let ready = roast("ready", "2026-11-10T12:00:00Z");
        let mut light = roast("light", "2026-11-14T12:00:00Z");
        light.roast_degree = Some(RoastDegree {
            scale: ColorScale::Agtron,
            whole_bean: Some(76.0),
            ground: None,
        });
        let past = roast("past", "2026-11-01T12:00:00Z");
        let gone = roast("long gone", "2026-10-01T12:00:00Z");
        let mut sim = roast("simulated", "2026-11-12T12:00:00Z");
        sim.origin = roast_core::RoastOrigin::Simulated;
        for r in [&fresh, &ready, &light, &past, &gone, &sim] {
            archive.save(r).unwrap();
        }
        x.create(ready.id, NewCupping::default(), now).unwrap();

        let rest = x.rest(now, false).unwrap();
        let line = |r: &RestEntry| (r.title.clone(), r.state, r.level, r.rest_days, r.cuppings);
        assert_eq!(
            rest.iter().map(line).collect::<Vec<_>>(),
            [
                ("fresh".into(), RestState::Resting, RoastLevel::Medium, 2, 0),
                ("light".into(), RestState::Resting, RoastLevel::Light, 6, 0),
                ("ready".into(), RestState::Ready, RoastLevel::Medium, 10, 1),
                (
                    "past".into(),
                    RestState::PastBest,
                    RoastLevel::Medium,
                    19,
                    0
                ),
            ]
        );
        assert_eq!(rest[1].level_from, LevelFrom::Colour);
        assert_eq!(rest[1].ready_from, at("2026-11-21T12:00:00Z"));
        assert_eq!(rest[2].level_from, LevelFrom::Default);
        assert_eq!(rest[2].best_until, at("2026-11-24T12:00:00Z"));
        assert_eq!(x.rest(now, true).unwrap().len(), 5);

        // The person's own windows replace the defaults.
        let mut own = RestWindows {
            medium: RestWindow {
                min_days: 1,
                max_days: 5,
            },
            ..RestWindows::default()
        };
        assert_eq!(x.set_rest_windows(own).unwrap(), own);
        assert_eq!(x.rest_windows().unwrap(), own);
        let rest = x.rest(now, false).unwrap();
        assert_eq!(rest[0].state, RestState::Ready);
        assert_eq!(rest[2].state, RestState::PastBest);
        own.dark = RestWindow {
            min_days: 9,
            max_days: 3,
        };
        assert_eq!(x.set_rest_windows(own).unwrap_err().status, 400);
    }
}
