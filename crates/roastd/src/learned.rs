//! What automatic control has learned about each machine (ADR 0015): how
//! strongly the burner acts and how much it takes to hold a temperature, per
//! air setting. Kept in `control.json` in the data directory, so every
//! preheat starts from the last one's experience.

use std::collections::BTreeMap;
use std::path::PathBuf;

use roast_control::PlantEstimate;
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
    path: PathBuf,
    data: Learned,
}

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
    /// Loads the file; a missing or unreadable file means nothing learned yet.
    #[must_use]
    pub fn load(path: PathBuf) -> Self {
        let data = std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| match serde_json::from_str(&s) {
                Ok(d) => Some(d),
                Err(e) => {
                    tracing::warn!("ignoring {}: {e}", path.display());
                    None
                }
            })
            .unwrap_or_default();
        LearnedStore { path, data }
    }

    /// What is known about preheating `machine` at this air setting: the
    /// value for this setting, else the nearest one learned, else the
    /// built-in estimate.
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
            .min_by_key(|(d, _)| *d)
            .map_or_else(PlantEstimate::default, |(_, e)| e)
    }

    /// Records what a preheat learned and writes the file.
    ///
    /// # Errors
    /// When the file cannot be written.
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
        let json = serde_json::to_string_pretty(&self.data).map_err(std::io::Error::other)?;
        let tmp = self.path.with_extension("json.tmp");
        std::fs::write(&tmp, json)?;
        std::fs::rename(&tmp, &self.path)
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
            hold_pct_per_c: 0.2,
        };
        let mut s = LearnedStore::load(path.clone());
        assert_eq!(s.preheat("cubean", 30.0), PlantEstimate::default());
        s.set_preheat("cubean", 31.0, learned).unwrap();

        let s = LearnedStore::load(path);
        assert_eq!(s.preheat("cubean", 29.0), learned);
        assert_eq!(s.preheat("cubean", 50.0), learned, "nearest air setting");
        assert_eq!(s.preheat("simulator", 30.0), PlantEstimate::default());
    }
}
