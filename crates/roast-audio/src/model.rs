//! The detector's own small model (ADR 0021 §10): from one second of the
//! spectrogram and the pops in it, the chance that first crack is going on.
//! Pure Rust, trained locally in seconds from the person's recordings and
//! mixed sound; it replaces the rate rule for first crack start only when it
//! hits the person's marks better ([`Evaluation::better`]).

use std::collections::VecDeque;

use serde::{Deserialize, Serialize};

use crate::listener::{COLUMNS_PER_S, Heard, Listener};
use crate::pops::{Band, Sensitivity};
use crate::spectrum::COLUMN_BANDS;
use crate::synth::Rng;

/// The settings key the trained model is kept under in the database.
pub const SETTINGS_KEY: &str = "audio_model";

/// Bands of the spectrogram pooled into one feature band.
const POOL: usize = 4;
/// Feature bands.
const FBANDS: usize = COLUMN_BANDS / POOL;
/// Length of a feature: mean and max per band, mean and max flux per band,
/// crack-band pops in the last 1, 5, 10 and 20 s, high-band pops in 1 and
/// 10 s.
pub const FEATURES: usize = FBANDS * 4 + 6;
/// Pops are remembered this long for the rates, s.
const POP_MEMORY_S: f64 = 20.0;
/// Hidden units.
const HIDDEN: usize = 16;
/// One window is one second of columns.
const WINDOW: usize = COLUMNS_PER_S as usize;
/// A window every half second.
const HOP: usize = WINDOW / 2;

/// Turns spectrogram columns and pops into one feature vector per window.
#[derive(Debug, Clone, Default)]
pub struct Featurizer {
    cols: VecDeque<[f32; FBANDS]>,
    pops: VecDeque<(f64, Band)>,
    since: usize,
    /// Sound time of the newest column, s.
    t: f64,
}

impl Featurizer {
    /// Takes a listener's output; returns `(t, features)` for each window
    /// completed (`t` in sound seconds at the window's end).
    pub fn push(&mut self, heard: &Heard, end_s: f64) -> Vec<(f64, Vec<f32>)> {
        let mut out = Vec::new();
        for p in &heard.pops {
            self.pops.push_back((p.t, p.band));
        }
        let n = heard.columns.len();
        for (i, c) in heard.columns.iter().enumerate() {
            let mut pooled = [0f32; FBANDS];
            for (b, v) in pooled.iter_mut().enumerate() {
                *v = c[b * POOL..(b + 1) * POOL]
                    .iter()
                    .map(|&x| f32::from(x) / 255.0)
                    .sum::<f32>()
                    / POOL as f32;
            }
            self.cols.push_back(pooled);
            while self.cols.len() > WINDOW + 1 {
                self.cols.pop_front();
            }
            self.t = end_s - (n - 1 - i) as f64 / f64::from(COLUMNS_PER_S);
            self.since += 1;
            if self.cols.len() == WINDOW + 1 && self.since >= HOP {
                self.since = 0;
                while self
                    .pops
                    .front()
                    .is_some_and(|p| p.0 < self.t - POP_MEMORY_S)
                {
                    self.pops.pop_front();
                }
                out.push((self.t, self.features()));
            }
        }
        out
    }

    fn features(&self) -> Vec<f32> {
        let mut f = Vec::with_capacity(FEATURES);
        let cols: Vec<&[f32; FBANDS]> = self.cols.iter().collect();
        for b in 0..FBANDS {
            let vals = cols[1..].iter().map(|c| c[b]);
            let mean = vals.clone().sum::<f32>() / WINDOW as f32;
            let max = vals.fold(0.0f32, f32::max);
            let flux: Vec<f32> = cols
                .windows(2)
                .map(|w| (w[1][b] - w[0][b]).max(0.0))
                .collect();
            let fmean = flux.iter().sum::<f32>() / flux.len().max(1) as f32;
            let fmax = flux.iter().copied().fold(0.0f32, f32::max);
            f.extend([mean, max, fmean, fmax]);
        }
        let count = |band, s: f64| {
            self.pops
                .iter()
                .filter(|p| p.1 == band && p.0 >= self.t - s && p.0 <= self.t)
                .count() as f32
        };
        for s in [1.0, 5.0, 10.0, 20.0] {
            f.push(count(Band::Crack, s));
        }
        f.push(count(Band::High, 1.0));
        f.push(count(Band::High, 10.0));
        f
    }
}

/// Plays a whole sound through a listener and the featurizer, in chunks of
/// 100 ms like the daemon: `(t, features)` per window, `t` in sound seconds.
#[must_use]
pub fn featurize(samples: &[f32], rate: u32, sensitivity: Sensitivity) -> Vec<(f64, Vec<f32>)> {
    let mut listener = Listener::new(rate, sensitivity);
    let mut f = Featurizer::default();
    let mut out = Vec::new();
    let mut done = 0usize;
    for chunk in samples.chunks((rate / 10).max(1) as usize) {
        done += chunk.len();
        let heard = listener.push(chunk);
        out.extend(f.push(&heard, done as f64 / f64::from(rate)));
    }
    out
}

/// Windows ending this soon after the marked start are left out of
/// training: the mark itself is uncertain by a few seconds, s.
const LABEL_BEFORE_S: f64 = 3.0;
const LABEL_AFTER_S: f64 = 1.0;

/// The label of a window ending at `t` when first crack ran `from`–`to`:
/// `None` near the start, where a mark is too uncertain to learn from.
#[must_use]
pub fn label(t: f64, from: f64, to: f64) -> Option<bool> {
    if t > from - LABEL_BEFORE_S && t < from + LABEL_AFTER_S {
        None
    } else {
        Some(t >= from + LABEL_AFTER_S && t <= to)
    }
}

/// The trained model: a standardiser and a network with one hidden layer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Model {
    pub mean: Vec<f32>,
    pub std: Vec<f32>,
    w1: Vec<f32>,
    b1: Vec<f32>,
    w2: Vec<f32>,
    b2: f32,
    /// What it was trained on, for people.
    pub trained_on: String,
    /// Accuracy on held-out windows, 0…1.
    pub accuracy: f32,
    /// How it did on the person's marked roasts against the rules.
    #[serde(default)]
    pub evaluation: Evaluation,
}

/// First crack heard within ±10 s of the person's mark, by the rules and by
/// the model, on roasts the model was not trained on.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Evaluation {
    /// Marked roasts compared.
    pub roasts: usize,
    pub rules_within: usize,
    pub model_within: usize,
}

impl Evaluation {
    /// The model hits more marks than the rules: it decides first crack.
    #[must_use]
    pub fn better(&self) -> bool {
        self.roasts > 0 && self.model_within > self.rules_within
    }
}

fn sigmoid(x: f32) -> f32 {
    1.0 / (1.0 + (-x).exp())
}

impl Model {
    /// The chance that first crack is going on in this window, 0…1.
    #[must_use]
    pub fn predict(&self, x: &[f32]) -> f32 {
        let z = self.standardise(x);
        self.forward(&z).1
    }

    fn standardise(&self, x: &[f32]) -> Vec<f32> {
        x.iter()
            .zip(&self.mean)
            .zip(&self.std)
            .map(|((v, m), s)| (v - m) / s)
            .collect()
    }

    fn forward(&self, z: &[f32]) -> (Vec<f32>, f32) {
        let h: Vec<f32> = (0..HIDDEN)
            .map(|j| {
                let row = &self.w1[j * FEATURES..(j + 1) * FEATURES];
                (row.iter().zip(z).map(|(w, x)| w * x).sum::<f32>() + self.b1[j]).tanh()
            })
            .collect();
        let o = h.iter().zip(&self.w2).map(|(a, w)| a * w).sum::<f32>() + self.b2;
        (h, sigmoid(o))
    }

    /// Trains on labelled windows (`true`: first crack going on). A fifth
    /// is held out to measure the accuracy.
    #[must_use]
    pub fn train(data: &[(Vec<f32>, bool)], epochs: usize, seed: u64, trained_on: &str) -> Model {
        let mut rng = Rng::new(seed);
        let n = data.len().max(1) as f32;
        let mut mean = vec![0f32; FEATURES];
        for (x, _) in data {
            for (m, v) in mean.iter_mut().zip(x) {
                *m += v / n;
            }
        }
        let mut std = vec![0f32; FEATURES];
        for (x, _) in data {
            for ((s, v), m) in std.iter_mut().zip(x).zip(&mean) {
                *s += (v - m).powi(2) / n;
            }
        }
        let std: Vec<f32> = std.iter().map(|s| s.sqrt().max(1e-3)).collect();
        let mut m = Model {
            mean,
            std,
            w1: (0..HIDDEN * FEATURES).map(|_| rng.signed() * 0.1).collect(),
            b1: vec![0.0; HIDDEN],
            w2: (0..HIDDEN).map(|_| rng.signed() * 0.1).collect(),
            b2: 0.0,
            trained_on: trained_on.to_owned(),
            accuracy: 0.0,
            evaluation: Evaluation::default(),
        };
        let mut order: Vec<usize> = (0..data.len()).collect();
        // Shuffle once, then hold out every fifth.
        for i in (1..order.len()).rev() {
            let j = (rng.next_f32() * (i + 1) as f32) as usize % (i + 1);
            order.swap(i, j);
        }
        let (test, train): (Vec<usize>, Vec<usize>) = order.iter().partition(|&&i| i % 5 == 0);
        let pos = train.iter().filter(|&&i| data[i].1).count().max(1) as f32;
        let neg = (train.len() as f32 - pos).max(1.0);
        let lr = 0.02;
        let mut idx = train.clone();
        for _ in 0..epochs {
            for i in (1..idx.len()).rev() {
                let j = (rng.next_f32() * (i + 1) as f32) as usize % (i + 1);
                idx.swap(i, j);
            }
            for &i in &idx {
                let (x, y) = &data[i];
                let z = m.standardise(x);
                let (h, p) = m.forward(&z);
                // Balanced classes: few windows are first crack.
                let weight = if *y { neg / pos } else { 1.0 }.min(20.0);
                let g = (p - if *y { 1.0 } else { 0.0 }) * weight * lr;
                for (j, &hj) in h.iter().enumerate() {
                    let gh = g * m.w2[j] * (1.0 - hj * hj);
                    m.w2[j] -= g * hj;
                    m.b1[j] -= gh;
                    let row = &mut m.w1[j * FEATURES..(j + 1) * FEATURES];
                    for (w, xv) in row.iter_mut().zip(&z) {
                        *w -= gh * xv;
                    }
                }
                m.b2 -= g;
            }
        }
        let right = test
            .iter()
            .filter(|&&i| (m.predict(&data[i].0) >= 0.5) == data[i].1)
            .count();
        m.accuracy = right as f32 / test.len().max(1) as f32;
        m
    }
}

/// Decides first crack from the model's windows: most of the last five
/// seconds above one half.
#[derive(Debug, Clone, Default)]
pub struct ModelVote {
    recent: VecDeque<(f64, f32)>,
}

/// Windows (half a second each) looked at.
const VOTE_WINDOWS: usize = 10;
/// … of which this many must say first crack.
const VOTE_NEEDED: usize = 7;

impl ModelVote {
    /// Takes a window's chance; returns the time of the first window of a
    /// run that says first crack.
    pub fn push(&mut self, t: f64, p: f32) -> Option<f64> {
        self.recent.push_back((t, p));
        while self.recent.len() > VOTE_WINDOWS {
            self.recent.pop_front();
        }
        let yes: Vec<f64> = self
            .recent
            .iter()
            .filter(|w| w.1 >= 0.5)
            .map(|w| w.0 - 1.0)
            .collect();
        (yes.len() >= VOTE_NEEDED).then(|| yes[0])
    }
}

/// The model listening along: from a listener's output, the sound time first
/// crack started at, once the vote says so (again with every window while it
/// does; [`crate::CrackLogic::model_heard`] takes it once).
#[derive(Debug, Clone)]
pub struct ModelEar {
    model: Model,
    featurizer: Featurizer,
    vote: ModelVote,
}

impl ModelEar {
    #[must_use]
    pub fn new(model: Model) -> Self {
        ModelEar {
            model,
            featurizer: Featurizer::default(),
            vote: ModelVote::default(),
        }
    }

    /// `end_s`: sound time at the end of the chunk `heard` came from.
    pub fn push(&mut self, heard: &Heard, end_s: f64) -> Option<f64> {
        let mut found = None;
        for (t, x) in self.featurizer.push(heard, end_s) {
            found = self.vote.push(t, self.model.predict(&x)).or(found);
        }
        found
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mix::{MixSpec, mix, synthetic_pops};

    fn windows(seed: u64) -> (Vec<(Vec<f32>, bool)>, f64) {
        let fc = synthetic_pops(48_000, 12, false, seed);
        let spec = MixSpec {
            seconds: 120.0,
            fc_at: 40.0,
            fc_s: 60.0,
            seed,
            ..MixSpec::default()
        };
        let m = mix(None, &fc, &[], &spec, 48_000);
        let (from, to) = (
            m.labels.first_crack_start.unwrap_or(40.0),
            m.labels.first_crack_end.unwrap_or(100.0),
        );
        let out = featurize(&m.sound.samples, 48_000, Sensitivity::Normal)
            .into_iter()
            .filter_map(|(t, x)| Some((x, label(t, from, to)?)))
            .collect();
        (out, from)
    }

    #[test]
    fn a_window_every_half_second_with_all_features() {
        let (w, _) = windows(1);
        assert!((210..=240).contains(&w.len()), "{}", w.len());
        assert!(w.iter().all(|(x, _)| x.len() == FEATURES));
    }

    #[test]
    fn the_model_learns_first_crack_from_mixed_sound() {
        let mut data = Vec::new();
        for seed in 1..=4 {
            data.extend(windows(seed).0);
        }
        let m = Model::train(&data, 15, 7, "mixed sound");
        assert!(m.accuracy > 0.85, "accuracy {}", m.accuracy);
        // A fresh mix: the vote finds first crack near its start.
        let (w, from) = windows(9);
        let mut vote = ModelVote::default();
        let mut t = None;
        for (i, (x, _)) in w.iter().enumerate() {
            if let Some(at) = vote.push(i as f64 * 0.5 + 1.0, m.predict(x)) {
                t = Some(at);
                break;
            }
        }
        let t = t.unwrap_or_else(|| panic!("not found"));
        assert!((t - from).abs() < 10.0, "found {t}, labelled {from}");
        // Through the whole detector, the model deciding first crack.
        let fc = synthetic_pops(48_000, 12, false, 11);
        let spec = MixSpec {
            seconds: 120.0,
            fc_at: 50.0,
            fc_s: 50.0,
            seed: 11,
            ..MixSpec::default()
        };
        let sound = mix(None, &fc, &[], &spec, 48_000);
        let label = sound.labels.first_crack_start.unwrap_or(50.0);
        let e = roast_core::CrackExpectation::from_roasts(&[], &[]);
        let a = crate::offline::analyze(
            &sound.sound.samples,
            48_000,
            Sensitivity::Normal,
            None,
            0.0,
            &e,
            Some(&m),
        );
        let heard = a
            .heard
            .iter()
            .find(|h| h.kind == roast_core::EventKind::FirstCrackStart)
            .unwrap_or_else(|| panic!("not heard: {:?}", a.per_10s));
        assert!(
            (heard.t - label).abs() < 10.0,
            "heard {}, labelled {label}",
            heard.t
        );
        assert!(!Evaluation::default().better());
        assert!(
            Evaluation {
                roasts: 3,
                rules_within: 1,
                model_within: 2
            }
            .better()
        );
        // It survives a round trip as JSON.
        let back: Model = serde_json::from_str(&serde_json::to_string(&m).unwrap_or_default())
            .unwrap_or_else(|e| panic!("{e}"));
        assert!((back.predict(&w[0].0) - m.predict(&w[0].0)).abs() < 1e-6);
    }
}
