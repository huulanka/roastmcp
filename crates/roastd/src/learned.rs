//! What automatic control has learned about each machine (ADR 0015): how
//! strongly the burner acts and how much it takes to hold a temperature, per
//! air setting. Kept in the database (setting `control`), so every preheat
//! starts from the last one's experience.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

use roast_control::PlantEstimate;
use roast_store::Store;
use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Serialize, Deserialize)]
struct Learned {
    #[serde(default)]
    machines: BTreeMap<String, MachineLearned>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct MachineLearned {
    /// Keyed by the air setting, in steps of 10 % (`"air_30"`).
    #[serde(default)]
    preheat: BTreeMap<String, PlantEstimate>,
}

/// The learned values and their file.
#[derive(Debug)]
pub struct LearnedStore {
    store: Arc<Store>,
    data: Learned,
}

/// The setting the learned values are stored under.
const SETTING: &str = "control";

/// The air setting rounded to 10 %.
fn air_step(air: f64) -> i64 {
    // Air is 0-100 %; the cast cannot overflow.
    #[allow(clippy::cast_possible_truncation)]
    let step = ((air / 10.0).round() * 10.0) as i64;
    step
}

fn air_key(air: f64) -> String {
    format!("air_{}", air_step(air))
}

impl LearnedStore {
    /// The learned values in `store`; nothing stored or unreadable means
    /// nothing learned yet. A `control.json` of the time before the database
    /// (`legacy`) is taken over first.
    #[must_use]
    pub fn open(store: Arc<Store>, legacy: &Path) -> Self {
        let stored = store.setting::<Learned>(SETTING).unwrap_or_else(|e| {
            tracing::warn!("ignoring the stored control values: {e}");
            None
        });
        let data = match stored {
            Some(d) => d,
            None => match crate::legacy::read::<Learned>(legacy) {
                Ok(Some(old)) => {
                    if store.set_setting(SETTING, &old).is_ok() {
                        crate::legacy::retire(legacy);
                    }
                    old
                }
                Ok(None) => Learned::default(),
                Err(msg) => {
                    tracing::warn!("ignoring it: {msg}");
                    Learned::default()
                }
            },
        };
        LearnedStore { store, data }
    }

    /// What is known about preheating `machine` at this air setting: the
    /// value for this setting, else the nearest one learned within 20 points
    /// of air, else the built-in estimate. A far-off air setting tells little
    /// (air 100 % needs about as much heat at 200 °C as air 20 %, but the
    /// burner acts differently; ADR 0018).
    #[must_use]
    pub fn preheat(&self, machine: &str, air: f64) -> PlantEstimate {
        let Some(m) = self.data.machines.get(machine) else {
            return PlantEstimate::default();
        };
        if let Some(e) = m.preheat.get(&air_key(air)) {
            return *e;
        }
        m.preheat
            .iter()
            .filter_map(|(k, e)| {
                let step: i64 = k.strip_prefix("air_")?.parse().ok()?;
                Some(((step - air_step(air)).abs(), *e))
            })
            .filter(|(d, _)| *d <= 20)
            .min_by_key(|(d, _)| *d)
            .map_or_else(PlantEstimate::default, |(_, e)| e)
    }

    /// Records what a preheat learned and writes the file.
    ///
    /// # Errors
    /// When the database cannot be written.
    pub fn set_preheat(
        &mut self,
        machine: &str,
        air: f64,
        estimate: PlantEstimate,
    ) -> std::io::Result<()> {
        self.data
            .machines
            .entry(machine.to_owned())
            .or_default()
            .preheat
            .insert(air_key(air), estimate);
        self.store
            .set_setting(SETTING, &self.data)
            .map_err(std::io::Error::other)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn learned_values_survive_a_restart_and_the_nearest_air_setting_helps() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("control.json");
        let learned = PlantEstimate {
            ror_per_pct: 0.3,
            hold_pct_200: 36.0,
        };
        let db = |dir: &Path| Arc::new(Store::open(&dir.join("roastmcp.sqlite")).unwrap());
        let mut s = LearnedStore::open(db(dir.path()), &path);
        assert_eq!(s.preheat("cubean", 30.0), PlantEstimate::default());
        s.set_preheat("cubean", 31.0, learned).unwrap();

        let s = LearnedStore::open(db(dir.path()), &path);
        assert_eq!(s.preheat("cubean", 29.0), learned);
        assert_eq!(s.preheat("cubean", 50.0), learned, "nearest air setting");
        assert_eq!(
            s.preheat("cubean", 100.0),
            PlantEstimate::default(),
            "too far off"
        );
        assert_eq!(s.preheat("simulator", 30.0), PlantEstimate::default());
    }
}
