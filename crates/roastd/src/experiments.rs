//! Experiments: one deliberate change over one or more roasts, with what was
//! expected and what came of it. They live in the database; a roast that
//! belongs to one names it (`experiment_id`), so the link is in the roast's
//! open file too.

use std::sync::Arc;

use chrono::Utc;
use roast_core::{ChangedVariable, Experiment, ExperimentStatus, VariableChange};
use roast_store::Store;
use serde::Deserialize;
use uuid::Uuid;

use crate::engine::Fail;
use crate::files::Archive;

/// Body of `POST /api/experiments`.
#[derive(Debug, Clone, Deserialize)]
pub struct NewExperiment {
    /// What is believed, e.g. "A hotter charge shortens drying".
    pub hypothesis: String,
    /// The one thing that is changed, in words.
    pub change: String,
    /// The change as a variable with its old and new value.
    #[serde(default)]
    pub variable: Option<VariableChange>,
    #[serde(default)]
    pub expected_outcome: Option<String>,
    /// The profile version the change was applied to.
    #[serde(default)]
    pub profile_version_id: Option<Uuid>,
    /// Roasts that already belong to it.
    #[serde(default)]
    pub roast_ids: Vec<Uuid>,
}

/// Body of `PATCH /api/experiments/{id}`; what is left out stays.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct ExperimentPatch {
    #[serde(default)]
    pub hypothesis: Option<String>,
    #[serde(default)]
    pub change: Option<String>,
    #[serde(default)]
    pub expected_outcome: Option<String>,
    /// What came of it. Concludes the experiment unless `status` says
    /// otherwise.
    #[serde(default)]
    pub conclusion: Option<String>,
    #[serde(default)]
    pub status: Option<ExperimentStatus>,
    #[serde(default)]
    pub add_roasts: Vec<Uuid>,
    #[serde(default)]
    pub remove_roasts: Vec<Uuid>,
}

fn db(e: roast_store::Error) -> Fail {
    Fail::internal(format!("the database failed: {e}"))
}

fn io(e: std::io::Error) -> Fail {
    Fail::internal(e.to_string())
}

fn text(s: Option<String>) -> Option<String> {
    s.map(|t| t.trim().to_owned()).filter(|t| !t.is_empty())
}

/// The experiments in the database, with their links kept in the roasts.
#[derive(Debug, Clone)]
pub struct Experiments {
    store: Arc<Store>,
    archive: Archive,
}

impl Experiments {
    /// The experiments in `store`, whose roasts are in `archive`.
    #[must_use]
    pub fn new(store: Arc<Store>, archive: Archive) -> Self {
        Experiments { store, archive }
    }

    /// All experiments, newest first.
    ///
    /// # Errors
    /// On a database error.
    pub fn list(&self) -> Result<Vec<Experiment>, Fail> {
        self.store.experiments().map_err(db)
    }

    /// One experiment.
    ///
    /// # Errors
    /// When there is none with this id.
    pub fn get(&self, id: Uuid) -> Result<Experiment, Fail> {
        self.list()?
            .into_iter()
            .find(|e| e.id == id)
            .ok_or_else(|| Fail::not_found("no such experiment"))
    }

    /// Writes an experiment down.
    ///
    /// # Errors
    /// Without a hypothesis or a change, with an unknown roast, or on a
    /// database error.
    pub fn create(&self, new: NewExperiment) -> Result<Experiment, Fail> {
        let hypothesis =
            text(Some(new.hypothesis)).ok_or_else(|| Fail::bad("the hypothesis is missing"))?;
        let description =
            text(Some(new.change)).ok_or_else(|| Fail::bad("say what is changed (change)"))?;
        let mut e = Experiment::new(
            hypothesis,
            ChangedVariable {
                description,
                structured: new.variable,
            },
            Utc::now(),
        );
        e.expected_outcome = text(new.expected_outcome);
        e.profile_version_id = new.profile_version_id;
        self.link(&mut e, &new.roast_ids)?;
        if !e.roast_ids.is_empty() {
            e.status = ExperimentStatus::Running;
        }
        self.store.save_experiment(&e).map_err(db)?;
        Ok(e)
    }

    /// Changes an experiment: its words, its roasts, its conclusion.
    ///
    /// # Errors
    /// For an unknown experiment or roast, an emptied hypothesis or change,
    /// or on a database error.
    pub fn update(&self, id: Uuid, p: ExperimentPatch) -> Result<Experiment, Fail> {
        let mut e = self.get(id)?;
        if let Some(h) = p.hypothesis {
            e.hypothesis =
                text(Some(h)).ok_or_else(|| Fail::bad("the hypothesis cannot be empty"))?;
        }
        if let Some(c) = p.change {
            e.changed_variable.description =
                text(Some(c)).ok_or_else(|| Fail::bad("the change cannot be empty"))?;
        }
        if let Some(x) = p.expected_outcome {
            e.expected_outcome = text(Some(x));
        }
        self.unlink(&mut e, &p.remove_roasts)?;
        self.link(&mut e, &p.add_roasts)?;
        if e.status == ExperimentStatus::Planned && !e.roast_ids.is_empty() {
            e.status = ExperimentStatus::Running;
        }
        if let Some(c) = p.conclusion {
            e.conclusion = text(Some(c));
            if e.conclusion.is_some() {
                e.status = ExperimentStatus::Done;
            }
        }
        if let Some(s) = p.status {
            e.status = s;
        }
        self.store.save_experiment(&e).map_err(db)?;
        Ok(e)
    }

    /// Removes an experiment; its roasts stay and forget it.
    ///
    /// # Errors
    /// For an unknown experiment or on a database error.
    pub fn delete(&self, id: Uuid) -> Result<(), Fail> {
        let mut e = self.get(id)?;
        let roasts = e.roast_ids.clone();
        self.unlink(&mut e, &roasts)?;
        self.store.delete_experiment(id).map_err(db)?;
        Ok(())
    }

    /// Adds roasts to the experiment and names it in each of them. A roast
    /// belongs to one experiment: it leaves the one it was in.
    fn link(&self, e: &mut Experiment, roasts: &[Uuid]) -> Result<(), Fail> {
        for &rid in roasts {
            let mut roast = self
                .archive
                .find(rid)
                .map_err(io)?
                .ok_or_else(|| Fail::not_found(format!("no roast {rid}")))?;
            if let Some(other) = roast.experiment_id.filter(|o| *o != e.id)
                && let Ok(mut was) = self.get(other)
            {
                was.roast_ids.retain(|r| *r != rid);
                self.store.save_experiment(&was).map_err(db)?;
            }
            if roast.experiment_id != Some(e.id) {
                roast.experiment_id = Some(e.id);
                self.archive.update(&roast).map_err(io)?;
            }
            if !e.roast_ids.contains(&rid) {
                e.roast_ids.push(rid);
            }
        }
        Ok(())
    }

    /// Takes roasts out of the experiment; a roast that is gone from the
    /// archive is simply dropped from the list.
    fn unlink(&self, e: &mut Experiment, roasts: &[Uuid]) -> Result<(), Fail> {
        for &rid in roasts {
            e.roast_ids.retain(|r| *r != rid);
            if let Some(mut roast) = self.archive.find(rid).map_err(io)?
                && roast.experiment_id == Some(e.id)
            {
                roast.experiment_id = None;
                self.archive.update(&roast).map_err(io)?;
            }
        }
        Ok(())
    }

    /// A roast left the archive: no experiment lists it any more.
    ///
    /// # Errors
    /// On a database error.
    pub fn roast_deleted(&self, roast: Uuid) -> Result<(), Fail> {
        for mut e in self.list()? {
            if e.roast_ids.contains(&roast) {
                e.roast_ids.retain(|r| *r != roast);
                self.store.save_experiment(&e).map_err(db)?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use roast_core::Roast;

    fn roast(title: &str) -> Roast {
        serde_json::from_value(serde_json::json!({
            "id": Uuid::new_v4(),
            "title": title,
            "roasted_at": "2026-11-09T18:04:00Z",
            "sampling_interval_s": 1.0,
            "batch": {},
            "origin": {"kind": "recorded"},
            "samples": [{"t": 0.0, "bt": 180.0}, {"t": 1.0, "bt": 179.0}],
        }))
        .unwrap()
    }

    fn new(hypothesis: &str, roast_ids: Vec<Uuid>) -> NewExperiment {
        NewExperiment {
            hypothesis: hypothesis.into(),
            change: "charge 10 °C hotter".into(),
            variable: Some(VariableChange {
                name: "charge_bt_c".into(),
                from: "190".into(),
                to: "200".into(),
            }),
            expected_outcome: Some(" shorter drying ".into()),
            profile_version_id: None,
            roast_ids,
        }
    }

    #[test]
    fn an_experiment_runs_from_plan_to_conclusion_with_its_roasts() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(Store::in_memory().unwrap());
        let archive = Archive::new(store.clone(), dir.path().join("roasts"));
        let x = Experiments::new(store, archive.clone());
        let (a, b) = (roast("a"), roast("b"));
        archive.save(&a).unwrap();
        archive.save(&b).unwrap();

        assert_eq!(x.create(new("  ", vec![])).unwrap_err().status, 400);
        assert_eq!(
            x.create(new("h", vec![Uuid::new_v4()])).unwrap_err().status,
            404
        );
        let e = x
            .create(new("A hotter charge shortens drying", vec![]))
            .unwrap();
        assert_eq!(e.status, ExperimentStatus::Planned);
        assert_eq!(e.expected_outcome.as_deref(), Some("shorter drying"));

        // Its first roast starts it, and the roast names it.
        let e = x
            .update(
                e.id,
                ExperimentPatch {
                    add_roasts: vec![a.id, b.id, a.id],
                    ..ExperimentPatch::default()
                },
            )
            .unwrap();
        assert_eq!(e.status, ExperimentStatus::Running);
        assert_eq!(e.roast_ids, [a.id, b.id]);
        assert_eq!(
            archive.find(a.id).unwrap().unwrap().experiment_id,
            Some(e.id)
        );

        // A roast belongs to one experiment: a second one takes it over.
        let other = x.create(new("Another idea", vec![b.id])).unwrap();
        assert_eq!(other.status, ExperimentStatus::Running);
        assert_eq!(x.get(e.id).unwrap().roast_ids, [a.id]);
        assert_eq!(
            archive.find(b.id).unwrap().unwrap().experiment_id,
            Some(other.id)
        );

        // A conclusion ends it.
        let e = x
            .update(
                e.id,
                ExperimentPatch {
                    conclusion: Some("drying 20 s shorter, no scorching".into()),
                    ..ExperimentPatch::default()
                },
            )
            .unwrap();
        assert_eq!(e.status, ExperimentStatus::Done);
        assert_eq!(x.list().unwrap().len(), 2);

        // A deleted roast leaves its experiment; a deleted experiment
        // leaves its roasts.
        x.roast_deleted(b.id).unwrap();
        assert!(x.get(other.id).unwrap().roast_ids.is_empty());
        x.delete(e.id).unwrap();
        assert_eq!(archive.find(a.id).unwrap().unwrap().experiment_id, None);
        assert_eq!(x.get(e.id).unwrap_err().status, 404);
    }
}
