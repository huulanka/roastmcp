//! Short-time spectrum: every 10 ms the spectral flux and the level of the
//! crack band and the high band, and a column for the spectrogram.

use std::sync::Arc;

use rustfft::num_complex::Complex32;
use rustfft::{Fft, FftPlanner};

/// The crack band, Hz: first crack's pops.
pub const CRACK_BAND_HZ: (f32, f32) = (400.0, 4000.0);
/// The high band, Hz: second crack's pops.
pub const HIGH_BAND_HZ: (f32, f32) = (6000.0, 16000.0);
/// Below this sample rate the high band is missing.
pub const MIN_RATE_FOR_HIGH_BAND: u32 = 32_000;
/// Frames per second (one every 10 ms).
pub const FRAMES_PER_S: u32 = 100;
/// Bands of a spectrogram column, log-spaced from [`COLUMN_HZ`].0 to .1.
pub const COLUMN_BANDS: usize = 64;
/// Frequency range of a spectrogram column, Hz.
pub const COLUMN_HZ: (f32, f32) = (100.0, 16000.0);
/// dBFS mapped to 0 and to 255 in a column.
const COLUMN_DB: (f32, f32) = (-120.0, -30.0);
/// Floor of the magnitude in dB, so silence is finite.
const FLOOR_DB: f32 = -160.0;

/// One analysed frame.
#[derive(Debug, Clone, PartialEq)]
pub struct Frame {
    /// Seconds since the first sample, at the frame's centre.
    pub t: f64,
    /// Mean rise of the log magnitude over the crack band since the previous
    /// frame, dB (only rises count).
    pub crack_flux: f32,
    /// The same for the high band; 0 without one.
    pub high_flux: f32,
    /// Level of the crack band, dBFS.
    pub crack_db: f32,
    /// Level of the high band, dBFS; [`FLOOR_DB`] without one.
    pub high_db: f32,
    /// Spectrogram column: [`COLUMN_BANDS`] levels, 0 (−120 dBFS) … 255 (−30 dBFS).
    pub column: [u8; COLUMN_BANDS],
}

/// Turns samples into [`Frame`]s.
pub struct Analyzer {
    rate: u32,
    n: usize,
    hop: usize,
    fft: Arc<dyn Fft<f32>>,
    window: Vec<f32>,
    /// Amplitude of a full-scale sine in a bin, for dBFS.
    scale: f32,
    pending: Vec<f32>,
    /// Samples consumed before `pending[0]`.
    consumed: u64,
    prev_db: Vec<f32>,
    scratch: Vec<Complex32>,
    db: Vec<f32>,
    crack: (usize, usize),
    high: Option<(usize, usize)>,
    columns: Vec<(usize, usize)>,
}

impl std::fmt::Debug for Analyzer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Analyzer")
            .field("rate", &self.rate)
            .field("n", &self.n)
            .field("hop", &self.hop)
            .finish_non_exhaustive()
    }
}

impl Analyzer {
    /// An analyser for this sample rate (at least 8 kHz).
    #[must_use]
    pub fn new(rate: u32) -> Self {
        let rate = rate.max(8000);
        // About 21 ms per window: 1024 points at 48 kHz.
        let n = ((rate as f32 * 0.0213) as usize).next_power_of_two();
        let hop = (rate / FRAMES_PER_S) as usize;
        let fft = FftPlanner::new().plan_fft_forward(n);
        let window: Vec<f32> = (0..n)
            .map(|i| {
                let x = std::f32::consts::TAU * i as f32 / n as f32;
                0.5 - 0.5 * x.cos()
            })
            .collect();
        let scale = window.iter().sum::<f32>() / 2.0;
        let bin_hz = rate as f32 / n as f32;
        let nyquist_bin = n / 2;
        let bin = |hz: f32| ((hz / bin_hz).round() as usize).clamp(1, nyquist_bin);
        let crack = (bin(CRACK_BAND_HZ.0), bin(CRACK_BAND_HZ.1));
        let high = (rate >= MIN_RATE_FOR_HIGH_BAND).then(|| {
            (
                bin(HIGH_BAND_HZ.0),
                bin(HIGH_BAND_HZ.1.min(rate as f32 / 2.0)),
            )
        });
        let top = COLUMN_HZ.1.min(rate as f32 / 2.0);
        let ratio = (top / COLUMN_HZ.0).ln();
        let columns = (0..COLUMN_BANDS)
            .map(|i| {
                let lo = COLUMN_HZ.0 * (ratio * i as f32 / COLUMN_BANDS as f32).exp();
                let hi = COLUMN_HZ.0 * (ratio * (i + 1) as f32 / COLUMN_BANDS as f32).exp();
                let (a, b) = (bin(lo), bin(hi));
                (a, b.max(a + 1).min(nyquist_bin + 1))
            })
            .collect();
        Analyzer {
            rate,
            n,
            hop,
            fft,
            window,
            scale,
            pending: Vec::with_capacity(n * 2),
            consumed: 0,
            prev_db: vec![FLOOR_DB; nyquist_bin + 1],
            scratch: vec![Complex32::new(0.0, 0.0); n],
            db: vec![FLOOR_DB; nyquist_bin + 1],
            crack,
            high,
            columns,
        }
    }

    /// The sample rate.
    #[must_use]
    pub fn rate(&self) -> u32 {
        self.rate
    }

    /// Whether the high band (second crack) is analysed.
    #[must_use]
    pub fn has_high_band(&self) -> bool {
        self.high.is_some()
    }

    /// Feeds mono samples (full scale ±1) and returns the frames completed.
    pub fn push(&mut self, samples: &[f32]) -> Vec<Frame> {
        self.pending.extend_from_slice(samples);
        let mut frames = Vec::new();
        while self.pending.len() >= self.n {
            frames.push(self.frame());
            self.pending.drain(..self.hop);
            self.consumed += self.hop as u64;
        }
        frames
    }

    fn frame(&mut self) -> Frame {
        for (i, c) in self.scratch.iter_mut().enumerate() {
            *c = Complex32::new(self.pending[i] * self.window[i], 0.0);
        }
        self.fft.process(&mut self.scratch);
        for (k, d) in self.db.iter_mut().enumerate() {
            let amp = self.scratch[k].norm() / self.scale;
            *d = (20.0 * amp.max(1e-12).log10()).max(FLOOR_DB);
        }
        let flux = |(a, b): (usize, usize), now: &[f32], prev: &[f32]| -> f32 {
            let rise: f32 = (a..b).map(|k| (now[k] - prev[k]).max(0.0)).sum();
            rise / (b - a).max(1) as f32
        };
        let level = |(a, b): (usize, usize), now: &[f32]| -> f32 {
            let p: f32 =
                (a..b).map(|k| 10f32.powf(now[k] / 10.0)).sum::<f32>() / (b - a).max(1) as f32;
            (10.0 * p.max(1e-16).log10()).max(FLOOR_DB)
        };
        let crack_flux = flux(self.crack, &self.db, &self.prev_db);
        let high_flux = self.high.map_or(0.0, |h| flux(h, &self.db, &self.prev_db));
        let crack_db = level(self.crack, &self.db);
        let high_db = self.high.map_or(FLOOR_DB, |h| level(h, &self.db));
        let mut column = [0u8; COLUMN_BANDS];
        for (c, &(a, b)) in column.iter_mut().zip(&self.columns) {
            let d = level((a, b), &self.db);
            let x = (d - COLUMN_DB.0) / (COLUMN_DB.1 - COLUMN_DB.0);
            *c = (x.clamp(0.0, 1.0) * 255.0).round() as u8;
        }
        std::mem::swap(&mut self.prev_db, &mut self.db);
        let centre = self.consumed as f64 + self.n as f64 / 2.0;
        Frame {
            t: centre / f64::from(self.rate),
            crack_flux,
            high_flux,
            crack_db,
            high_db,
            column,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(rate: u32, hz: f32, amp: f32, seconds: f32) -> Vec<f32> {
        (0..(rate as f32 * seconds) as usize)
            .map(|i| amp * (std::f32::consts::TAU * hz * i as f32 / rate as f32).sin())
            .collect()
    }

    #[test]
    fn a_frame_every_10_ms() {
        let mut a = Analyzer::new(48_000);
        let frames = a.push(&vec![0.0; 48_000]);
        // (48000 − 1024) / 480 + 1
        assert_eq!(frames.len(), 98);
        assert!((frames[1].t - frames[0].t - 0.01).abs() < 1e-9);
        assert!(a.has_high_band());
        assert!(!Analyzer::new(16_000).has_high_band());
    }

    #[test]
    fn a_sine_in_the_crack_band_shows_there_and_in_its_column() {
        let mut a = Analyzer::new(48_000);
        let frames = a.push(&sine(48_000, 1000.0, 0.5, 0.5));
        let f = frames.last().unwrap_or_else(|| panic!("no frame"));
        assert!(
            f.crack_db > f.high_db + 40.0,
            "{} vs {}",
            f.crack_db,
            f.high_db
        );
        // Steady: little flux.
        assert!(f.crack_flux < 3.0, "{}", f.crack_flux);
        let loudest = f
            .column
            .iter()
            .enumerate()
            .max_by_key(|(_, v)| **v)
            .map(|(i, _)| i)
            .unwrap_or_default();
        let ratio = (COLUMN_HZ.1 / COLUMN_HZ.0).ln();
        let hz = COLUMN_HZ.0 * (ratio * (loudest as f32 + 0.5) / COLUMN_BANDS as f32).exp();
        assert!((800.0..1250.0).contains(&hz), "{hz} Hz");
    }

    #[test]
    fn an_onset_makes_flux() {
        let mut a = Analyzer::new(48_000);
        let mut s = vec![0.0; 24_000];
        s.extend(sine(48_000, 1500.0, 0.5, 0.2));
        let frames = a.push(&s);
        let peak = frames.iter().map(|f| f.crack_flux).fold(0.0f32, f32::max);
        assert!(peak > 20.0, "{peak}");
    }
}
