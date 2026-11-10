//! Roast profiles and their versions (`roast-core::profile`), kept in the
//! database like the inventory (ADR 0003). Every change to a profile's plan
//! is a new, immutable version.

use std::path::Path;
use std::sync::Arc;

use chrono::Utc;
use roast_core::{ActionTrigger, MAX_EVENT_OFFSET_S, Profile, ProfileTarget, ProfileVersion};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use roast_store::Store;

use crate::engine::Fail;

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
struct ProfilesFile {
    #[serde(default)]
    profiles: Vec<Profile>,
    #[serde(default)]
    versions: Vec<ProfileVersion>,
}

/// A profile with its newest version, for lists.
#[derive(Debug, Clone, Serialize)]
pub struct ProfileSummary {
    #[serde(flatten)]
    profile: Profile,
    latest: Option<VersionInfo>,
    versions: usize,
}

/// A version without its curve.
#[derive(Debug, Clone, Serialize)]
pub struct VersionInfo {
    id: Uuid,
    version: u32,
    created_at: chrono::DateTime<Utc>,
    change_note: Option<String>,
}

/// A profile with all its versions, newest first.
#[derive(Debug, Clone, Serialize)]
pub struct ProfileDetail {
    #[serde(flatten)]
    profile: Profile,
    versions: Vec<ProfileVersion>,
}

/// Body of `POST /api/profiles`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NewProfile {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub intended_beans: Option<String>,
    #[serde(default)]
    pub batch_size_g: Option<f64>,
    /// The first version's plan.
    pub target: ProfileTarget,
    #[serde(default)]
    pub change_note: Option<String>,
}

/// Body of `POST /api/profiles/{id}/versions`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NewVersion {
    pub target: ProfileTarget,
    #[serde(default)]
    pub change_note: Option<String>,
    /// The version this one was made from; the newest if left out.
    #[serde(default)]
    pub parent_version: Option<u32>,
}

/// Body of `PATCH /api/profiles/{id}`.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProfilePatch {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub intended_beans: Option<String>,
    #[serde(default)]
    pub batch_size_g: Option<f64>,
    #[serde(default)]
    pub archived: Option<bool>,
}

/// The profiles and their file.
#[derive(Debug)]
pub struct ProfileStore {
    store: Arc<Store>,
    data: ProfilesFile,
    broken: Option<String>,
}

fn text(s: Option<String>) -> Option<String> {
    s.map(|t| t.trim().to_owned()).filter(|t| !t.is_empty())
}

/// Checks a plan: a curve in time order with plausible values, actions in range.
fn check(target: &ProfileTarget) -> Result<(), Fail> {
    if target.points.len() < 2 {
        return Err(Fail::bad(
            "a profile needs a target curve of at least two points",
        ));
    }
    if target.points.len() > 5000 {
        return Err(Fail::bad(
            "the target curve has too many points (at most 5000)",
        ));
    }
    let mut last = f64::NEG_INFINITY;
    for p in &target.points {
        if !(p.roast_time_s.is_finite() && p.roast_time_s >= 0.0 && p.roast_time_s > last) {
            return Err(Fail::bad("target curve times must rise from 0 s"));
        }
        if !(0.0..=300.0).contains(&p.bt_c) {
            return Err(Fail::bad(
                "target temperatures must be between 0 and 300 °C",
            ));
        }
        last = p.roast_time_s;
    }
    for a in &target.actions {
        if !(0.0..=100.0).contains(&a.value) {
            return Err(Fail::bad("recipe values must be between 0 and 100 %"));
        }
        let fits = match a.trigger {
            ActionTrigger::Time { roast_time_s } => (0.0..=1800.0).contains(&roast_time_s),
            ActionTrigger::Bt { bt_c } => (50.0..=300.0).contains(&bt_c),
            ActionTrigger::Event { offset_s, .. } => (0.0..=MAX_EVENT_OFFSET_S).contains(&offset_s),
        };
        if !fits {
            return Err(Fail::bad(
                "recipe steps come at 0-30 min, at 50-300 °C, or 0-15 min after dry end or first crack",
            ));
        }
    }
    Ok(())
}

impl ProfileStore {
    /// The profiles in `store`. A `profiles.json` of the time before the
    /// database (`legacy`) is taken over first; an unreadable one is
    /// reported and left alone.
    #[must_use]
    pub fn open(store: Arc<Store>, legacy: &Path) -> Self {
        let loaded =
            store
                .profiles()
                .map_err(|e| e.to_string())
                .and_then(|(profiles, versions)| {
                    if !profiles.is_empty() {
                        return Ok(ProfilesFile { profiles, versions });
                    }
                    match crate::legacy::read::<ProfilesFile>(legacy)? {
                        Some(old) => {
                            store
                                .save_profiles(&old.profiles, &old.versions)
                                .map_err(|e| {
                                    format!("{} could not be taken over: {e}", legacy.display())
                                })?;
                            crate::legacy::retire(legacy);
                            Ok(old)
                        }
                        None => Ok(ProfilesFile { profiles, versions }),
                    }
                });
        match loaded {
            Ok(data) => ProfileStore {
                store,
                data,
                broken: None,
            },
            Err(msg) => {
                tracing::error!("{msg}");
                ProfileStore {
                    store,
                    data: ProfilesFile::default(),
                    broken: Some(msg),
                }
            }
        }
    }

    fn change<T>(
        &mut self,
        f: impl FnOnce(&mut ProfilesFile) -> Result<T, Fail>,
    ) -> Result<T, Fail> {
        if let Some(msg) = &self.broken {
            return Err(Fail::conflict(msg.clone(), None));
        }
        let before = self.data.clone();
        let out = f(&mut self.data)?;
        if let Err(e) = self.save() {
            self.data = before;
            return Err(Fail::internal(format!(
                "the profiles could not be saved: {e}"
            )));
        }
        Ok(out)
    }

    fn save(&self) -> roast_store::Result<()> {
        self.store
            .save_profiles(&self.data.profiles, &self.data.versions)
    }

    fn versions_of(&self, id: Uuid) -> Vec<ProfileVersion> {
        let mut v: Vec<ProfileVersion> = self
            .data
            .versions
            .iter()
            .filter(|v| v.profile_id == id)
            .cloned()
            .collect();
        v.sort_by_key(|v| std::cmp::Reverse(v.version));
        v
    }

    /// Profiles with their newest version, newest change first.
    #[must_use]
    pub fn list(&self, include_archived: bool) -> Vec<ProfileSummary> {
        let mut out: Vec<ProfileSummary> = self
            .data
            .profiles
            .iter()
            .filter(|p| include_archived || !p.archived)
            .map(|p| {
                let versions = self.versions_of(p.id);
                ProfileSummary {
                    profile: p.clone(),
                    latest: versions.first().map(|v| VersionInfo {
                        id: v.id,
                        version: v.version,
                        created_at: v.created_at,
                        change_note: v.change_note.clone(),
                    }),
                    versions: versions.len(),
                }
            })
            .collect();
        out.sort_by_key(|s| {
            std::cmp::Reverse(
                s.latest
                    .as_ref()
                    .map_or(s.profile.created_at, |l| l.created_at),
            )
        });
        out
    }

    /// One profile with all its versions.
    ///
    /// # Errors
    /// When there is no such profile.
    pub fn get(&self, id: Uuid) -> Result<ProfileDetail, Fail> {
        let profile = self
            .data
            .profiles
            .iter()
            .find(|p| p.id == id)
            .cloned()
            .ok_or_else(|| Fail::not_found(format!("no profile {id}")))?;
        Ok(ProfileDetail {
            versions: self.versions_of(id),
            profile,
        })
    }

    /// A version and its profile, by the version's id.
    ///
    /// # Errors
    /// When there is no such version.
    pub fn version(&self, version_id: Uuid) -> Result<(Profile, ProfileVersion), Fail> {
        let v = self
            .data
            .versions
            .iter()
            .find(|v| v.id == version_id)
            .cloned()
            .ok_or_else(|| Fail::not_found(format!("no profile version {version_id}")))?;
        let p = self
            .data
            .profiles
            .iter()
            .find(|p| p.id == v.profile_id)
            .cloned()
            .ok_or_else(|| Fail::not_found(format!("no profile {}", v.profile_id)))?;
        Ok((p, v))
    }

    /// Creates a profile with its first version.
    ///
    /// # Errors
    /// A missing name, an implausible plan, or a failed save.
    pub fn create(
        &mut self,
        req: NewProfile,
        machine_id: Option<String>,
    ) -> Result<ProfileDetail, Fail> {
        let name = req.name.trim().to_owned();
        if name.is_empty() {
            return Err(Fail::bad("a profile needs a name"));
        }
        check(&req.target)?;
        let now = Utc::now();
        let mut profile = Profile::new(name, now);
        profile.description = text(req.description);
        profile.intended_beans = text(req.intended_beans);
        profile.batch_size_g = req.batch_size_g.filter(|g| g.is_finite() && *g > 0.0);
        profile.machine_id = machine_id;
        let version = ProfileVersion {
            id: Uuid::new_v4(),
            profile_id: profile.id,
            version: 1,
            parent_version: None,
            created_at: now,
            change_note: text(req.change_note),
            target: req.target,
        };
        let id = profile.id;
        self.change(|d| {
            d.profiles.push(profile);
            d.versions.push(version);
            Ok(())
        })?;
        self.get(id)
    }

    /// Saves a changed plan as the profile's next version.
    ///
    /// # Errors
    /// An unknown profile, an implausible plan, or a failed save.
    pub fn add_version(&mut self, id: Uuid, req: NewVersion) -> Result<ProfileDetail, Fail> {
        check(&req.target)?;
        let versions = self.get(id)?.versions;
        let newest = versions.first().map_or(0, |v| v.version);
        let parent = req.parent_version.or((newest > 0).then_some(newest));
        let version = ProfileVersion {
            id: Uuid::new_v4(),
            profile_id: id,
            version: newest + 1,
            parent_version: parent,
            created_at: Utc::now(),
            change_note: text(req.change_note),
            target: req.target,
        };
        self.change(|d| {
            d.versions.push(version);
            Ok(())
        })?;
        self.get(id)
    }

    /// Changes a profile's name, description, beans, batch size or archive state.
    ///
    /// # Errors
    /// An unknown profile, an empty name, or a failed save.
    pub fn update(&mut self, id: Uuid, p: ProfilePatch) -> Result<ProfileDetail, Fail> {
        self.change(|d| {
            let profile = d
                .profiles
                .iter_mut()
                .find(|x| x.id == id)
                .ok_or_else(|| Fail::not_found(format!("no profile {id}")))?;
            if let Some(n) = p.name {
                if n.trim().is_empty() {
                    return Err(Fail::bad("a profile needs a name"));
                }
                n.trim().clone_into(&mut profile.name);
            }
            if p.description.is_some() {
                profile.description = text(p.description);
            }
            if p.intended_beans.is_some() {
                profile.intended_beans = text(p.intended_beans);
            }
            if let Some(g) = p.batch_size_g {
                profile.batch_size_g = Some(g).filter(|g| g.is_finite() && *g > 0.0);
            }
            if let Some(a) = p.archived {
                profile.archived = a;
            }
            Ok(())
        })?;
        self.get(id)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use roast_core::TargetPoint;

    fn db(dir: &Path) -> Arc<Store> {
        Arc::new(Store::open(&dir.join("roastmcp.sqlite")).unwrap())
    }

    fn target(drop_s: f64) -> ProfileTarget {
        ProfileTarget {
            points: vec![
                TargetPoint {
                    roast_time_s: 0.0,
                    bt_c: 200.0,
                    ror_c_per_min: None,
                },
                TargetPoint {
                    roast_time_s: drop_s,
                    bt_c: 204.0,
                    ror_c_per_min: Some(5.0),
                },
            ],
            actions: Vec::new(),
            events: Default::default(),
            tolerance: Default::default(),
        }
    }

    #[test]
    fn versions_count_up_and_survive_a_restart() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("profiles.json");
        let mut s = ProfileStore::open(db(dir.path()), &path);
        let created = s
            .create(
                NewProfile {
                    name: " Brasilien Espresso ".into(),
                    description: None,
                    intended_beans: Some("Yellow Bourbon".into()),
                    batch_size_g: Some(200.0),
                    target: target(630.0),
                    change_note: Some("from BR011".into()),
                },
                Some("skywalker-cubean-plus".into()),
            )
            .unwrap();
        let id = created.profile.id;
        assert_eq!(created.profile.name, "Brasilien Espresso");
        let v2 = s
            .add_version(
                id,
                NewVersion {
                    target: target(640.0),
                    change_note: Some("drop 10 s later".into()),
                    parent_version: None,
                },
            )
            .unwrap();
        assert_eq!(v2.versions[0].version, 2);
        assert_eq!(v2.versions[0].parent_version, Some(1));

        let s = ProfileStore::open(db(dir.path()), &path);
        let list = s.list(false);
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].versions, 2);
        let (p, v) = s.version(v2.versions[0].id).unwrap();
        assert_eq!((p.id, v.version), (id, 2));
    }

    #[test]
    fn an_implausible_plan_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = ProfileStore::open(db(dir.path()), &dir.path().join("profiles.json"));
        let mut bad = target(600.0);
        bad.points[1].roast_time_s = 0.0;
        let req = NewProfile {
            name: "x".into(),
            description: None,
            intended_beans: None,
            batch_size_g: None,
            target: bad,
            change_note: None,
        };
        assert_eq!(s.create(req, None).unwrap_err().status, 400);
    }
}
