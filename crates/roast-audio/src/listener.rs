//! From samples to pops and spectrogram columns.

use crate::pops::{Band, Pop, PopFinder, Sensitivity};
use crate::spectrum::{Analyzer, COLUMN_BANDS, FRAMES_PER_S};

/// Frames pooled into one spectrogram column (20 columns a second).
pub const FRAMES_PER_COLUMN: usize = 5;
/// Spectrogram columns per second.
pub const COLUMNS_PER_S: u32 = FRAMES_PER_S / FRAMES_PER_COLUMN as u32;
/// A high-band pop this close to a crack-band pop is the same pop, s.
const SAME_POP_S: f64 = 0.025;

/// What a chunk of sound gave.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Heard {
    /// Completed spectrogram columns (the loudest of their frames).
    pub columns: Vec<[u8; COLUMN_BANDS]>,
    /// Pops found, `t` in seconds since the start of the sound.
    pub pops: Vec<Pop>,
    /// Crack band level of the latest frame, dBFS.
    pub crack_db: Option<f32>,
    /// High band level of the latest frame, dBFS.
    pub high_db: Option<f32>,
}

/// Listens to one stream of sound.
#[derive(Debug)]
pub struct Listener {
    analyzer: Analyzer,
    crack: PopFinder,
    high: PopFinder,
    pool: Vec<[u8; COLUMN_BANDS]>,
    /// High-band pops held back until no crack-band pop can claim them.
    held: Vec<Pop>,
    recent_crack: Vec<f64>,
}

impl Listener {
    #[must_use]
    pub fn new(rate: u32, sensitivity: Sensitivity) -> Self {
        Listener {
            analyzer: Analyzer::new(rate),
            crack: PopFinder::new(Band::Crack, sensitivity),
            high: PopFinder::new(Band::High, sensitivity),
            pool: Vec::with_capacity(FRAMES_PER_COLUMN),
            held: Vec::new(),
            recent_crack: Vec::new(),
        }
    }

    #[must_use]
    pub fn rate(&self) -> u32 {
        self.analyzer.rate()
    }

    #[must_use]
    pub fn has_high_band(&self) -> bool {
        self.analyzer.has_high_band()
    }

    pub fn set_sensitivity(&mut self, s: Sensitivity) {
        self.crack.set_sensitivity(s);
        self.high.set_sensitivity(s);
    }

    /// Extra dB a rise needs per band (from the calibration).
    pub fn set_extra_db(&mut self, crack: f32, high: f32) {
        self.crack.extra_db = crack;
        self.high.extra_db = high;
    }

    /// Feeds mono samples.
    pub fn push(&mut self, samples: &[f32]) -> Heard {
        let mut out = Heard::default();
        let has_high = self.analyzer.has_high_band();
        for f in self.analyzer.push(samples) {
            if let Some(p) = self.crack.push(f.t, f.crack_flux, f.crack_db) {
                self.recent_crack.push(p.t);
                self.held.retain(|h| (h.t - p.t).abs() > SAME_POP_S);
                out.pops.push(p);
            }
            if has_high
                && let Some(p) = self.high.push(f.t, f.high_flux, f.high_db)
                && !self
                    .recent_crack
                    .iter()
                    .any(|&c| (c - p.t).abs() <= SAME_POP_S)
            {
                self.held.push(p);
            }
            // A held high-band pop is released once a crack-band pop at its
            // time would have been found (the finders lag equally).
            let now = f.t;
            let (ready, wait): (Vec<Pop>, Vec<Pop>) = self
                .held
                .drain(..)
                .partition(|h| now - h.t > SAME_POP_S + 0.011);
            self.held = wait;
            out.pops.extend(ready);
            self.recent_crack.retain(|&c| now - c < 1.0);

            out.crack_db = Some(f.crack_db);
            out.high_db = has_high.then_some(f.high_db);
            self.pool.push(f.column);
            if self.pool.len() == FRAMES_PER_COLUMN {
                let mut col = [0u8; COLUMN_BANDS];
                for c in &self.pool {
                    for (a, b) in col.iter_mut().zip(c) {
                        *a = (*a).max(*b);
                    }
                }
                out.columns.push(col);
                self.pool.clear();
            }
        }
        out.pops.sort_by(|a, b| a.t.total_cmp(&b.t));
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::synth::{Synth, voice};

    fn listen(samples: &[f32]) -> Vec<Pop> {
        let mut l = Listener::new(48_000, Sensitivity::Normal);
        let mut pops = Vec::new();
        for chunk in samples.chunks(4800) {
            pops.extend(l.push(chunk).pops);
        }
        pops
    }

    #[test]
    fn fan_noise_alone_gives_almost_no_pops() {
        let mut s = Synth::new(48_000, 7);
        let pops = listen(&s.take(60.0));
        assert!(
            pops.len() <= 3,
            "{} pops in a minute of fan: {pops:?}",
            pops.len()
        );
    }

    #[test]
    fn first_crack_pops_are_found_in_the_crack_band() {
        let mut s = Synth::new(48_000, 11);
        let mut sound = s.take(3.0);
        let mut at = Vec::new();
        for i in 0..10 {
            at.push(sound.len() as f64 / 48_000.0);
            s.pop(false);
            sound.extend(s.take(0.7 + 0.1 * f64::from(i % 3)));
        }
        let pops = listen(&sound);
        let crack: Vec<_> = pops.iter().filter(|p| p.band == Band::Crack).collect();
        let found = at
            .iter()
            .filter(|&&t| crack.iter().any(|p| (p.t - t).abs() < 0.04))
            .count();
        assert!(found >= 9, "found {found} of 10: {pops:?}");
        assert!(crack.len() <= 12, "{} crack pops", crack.len());
    }

    #[test]
    fn second_crack_pops_are_found_in_the_high_band() {
        let mut s = Synth::new(48_000, 13);
        let mut sound = s.take(3.0);
        for _ in 0..20 {
            s.pop(true);
            sound.extend(s.take(0.2));
        }
        let pops = listen(&sound);
        let high = pops.iter().filter(|p| p.band == Band::High).count();
        let crack = pops.iter().filter(|p| p.band == Band::Crack).count();
        assert!(high >= 16, "{high} high-band pops of 20: {pops:?}");
        assert!(crack <= 4, "{crack} crack-band pops from second crack");
    }

    #[test]
    fn a_voice_is_not_taken_for_cracks() {
        let mut s = Synth::new(48_000, 17);
        let fan = s.take(20.0);
        let v = voice(48_000, 20.0, 0.05);
        let sound: Vec<f32> = fan.iter().zip(&v).map(|(a, b)| a + b).collect();
        let pops = listen(&sound);
        let crack = pops.iter().filter(|p| p.band == Band::Crack).count();
        // 80 syllables in 20 s.
        assert!(crack <= 8, "{crack} crack-band pops from a voice: {pops:?}");
    }

    #[test]
    fn columns_come_twenty_a_second() {
        let mut s = Synth::new(48_000, 3);
        let mut l = Listener::new(48_000, Sensitivity::Normal);
        let mut n = 0;
        for _ in 0..10 {
            n += l.push(&s.take(0.1)).columns.len();
        }
        assert!((18..=20).contains(&n), "{n}");
    }
}
