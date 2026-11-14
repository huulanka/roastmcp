//! Finding pops: short, sharp rises of a band far above its recent noise
//! that fall back quickly (a crack, not a voice).

use std::collections::VecDeque;

use serde::{Deserialize, Serialize};

use crate::spectrum::FRAMES_PER_S;

/// Which band a pop was heard in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Band {
    /// 0.4–4 kHz: first crack.
    Crack,
    /// 6–16 kHz: second crack.
    High,
}

/// One pop.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Pop {
    /// Seconds since the start of the sound (the caller maps it to the roast clock).
    pub t: f64,
    pub band: Band,
    /// How far the flux rose above the band's recent median, dB.
    pub strength_db: f32,
}

/// How easily a rise counts as a pop.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Sensitivity {
    Low,
    #[default]
    Normal,
    High,
}

impl Sensitivity {
    /// Multiple of the median absolute deviation above the median.
    #[must_use]
    pub fn mad_factor(self) -> f32 {
        match self {
            Sensitivity::Low => 8.0,
            Sensitivity::Normal => 6.0,
            Sensitivity::High => 4.5,
        }
    }

    /// Smallest rise above the median that counts, dB.
    #[must_use]
    pub fn min_rise_db(self) -> f32 {
        match self {
            Sensitivity::Low => 4.0,
            Sensitivity::Normal => 3.0,
            Sensitivity::High => 2.5,
        }
    }
}

/// Seconds of flux the noise statistics look back over.
const HISTORY_S: f64 = 3.0;
/// The shortest time between two pops of a band, s.
pub const REFRACTORY_S: f64 = 0.05;
/// Frames after the peak at which the band must have fallen back.
const DECAY_FRAMES: usize = 3;
/// It must have lost at least this share of its rise (in dB) by then.
const DECAY_SHARE: f32 = 0.5;

#[derive(Debug, Clone, Copy)]
struct Slot {
    t: f64,
    flux: f32,
    level: f32,
}

/// Finds pops in one band's frames.
#[derive(Debug, Clone)]
pub struct PopFinder {
    band: Band,
    sensitivity: Sensitivity,
    /// Recent flux values for the noise statistics (excluding the newest few).
    history: VecDeque<f32>,
    /// The last frames, newest last: the candidate sits `DECAY_FRAMES` back.
    recent: VecDeque<Slot>,
    last_pop: f64,
    /// Extra rise required above the median, dB (calibration, ADR 0021 §10).
    pub extra_db: f32,
}

impl PopFinder {
    #[must_use]
    pub fn new(band: Band, sensitivity: Sensitivity) -> Self {
        PopFinder {
            band,
            sensitivity,
            history: VecDeque::new(),
            recent: VecDeque::new(),
            last_pop: f64::NEG_INFINITY,
            extra_db: 0.0,
        }
    }

    pub fn set_sensitivity(&mut self, s: Sensitivity) {
        self.sensitivity = s;
    }

    /// Feeds a frame's flux and level; returns a pop found a few frames back.
    pub fn push(&mut self, t: f64, flux: f32, level_db: f32) -> Option<Pop> {
        self.recent.push_back(Slot {
            t,
            flux,
            level: level_db,
        });
        // [pre, pre2, candidate, after1 .. after DECAY_FRAMES]
        let need = DECAY_FRAMES + 3;
        if self.recent.len() > need {
            if let Some(old) = self.recent.pop_front() {
                self.history.push_back(old.flux);
                let keep = (HISTORY_S * f64::from(FRAMES_PER_S)) as usize;
                while self.history.len() > keep {
                    self.history.pop_front();
                }
            }
        }
        if self.recent.len() < need || self.history.len() < FRAMES_PER_S as usize / 2 {
            return None;
        }
        let pre = self.recent[0];
        let before = self.recent[1];
        let c = self.recent[2];
        let after = self.recent[3];
        let late = self.recent[2 + DECAY_FRAMES];
        if c.flux < before.flux || c.flux <= after.flux {
            return None;
        }
        if c.t - self.last_pop < REFRACTORY_S {
            return None;
        }
        let (median, mad) = median_mad(&self.history);
        let rise = c.flux - median;
        let threshold = (self.sensitivity.mad_factor() * mad.max(0.05))
            .max(self.sensitivity.min_rise_db())
            + self.extra_db;
        if rise < threshold {
            return None;
        }
        // A crack is over within a few frames; a voice or a clatter is not.
        let peak_level = c.level.max(after.level);
        let lift = peak_level - pre.level;
        if lift > 0.0 && late.level - pre.level > (1.0 - DECAY_SHARE) * lift {
            return None;
        }
        self.last_pop = c.t;
        Some(Pop {
            t: c.t,
            band: self.band,
            strength_db: rise,
        })
    }
}

/// Median and median absolute deviation.
fn median_mad(v: &VecDeque<f32>) -> (f32, f32) {
    let mut a: Vec<f32> = v.iter().copied().collect();
    let m = median(&mut a);
    for x in &mut a {
        *x = (*x - m).abs();
    }
    (m, median(&mut a))
}

fn median(a: &mut [f32]) -> f32 {
    if a.is_empty() {
        return 0.0;
    }
    let mid = a.len() / 2;
    let (_, m, _) = a.select_nth_unstable_by(mid, f32::total_cmp);
    *m
}
