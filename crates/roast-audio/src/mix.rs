//! The crack mixer (ADR 0021 §10): real crack pops, cut from recordings,
//! mixed into a roaster's fan noise with a first crack that swells and fades,
//! an optional second crack and voices as distractors. The result has exact
//! labels, for testing the detector and later for training a model.

use serde::Serialize;

use crate::file::Sound;
use crate::listener::Listener;
use crate::pops::{Band, Sensitivity};
use crate::synth::{Rng, Synth, voice};

/// What to mix.
#[derive(Debug, Clone)]
pub struct MixSpec {
    pub seconds: f64,
    /// First crack starts here, s.
    pub fc_at: f64,
    /// … and lasts this long, s.
    pub fc_s: f64,
    /// Pops per second at its height.
    pub fc_rate: f64,
    /// Second crack starts here, if any, s.
    pub sc_at: Option<f64>,
    /// Pop peaks above the noise RMS, dB (each pop varies by ±6 dB).
    pub pop_db: f32,
    /// Voices before first crack.
    pub voices: bool,
    pub seed: u64,
}

impl Default for MixSpec {
    fn default() -> Self {
        MixSpec {
            seconds: 180.0,
            fc_at: 60.0,
            fc_s: 80.0,
            fc_rate: 0.7,
            sc_at: None,
            pop_db: 20.0,
            voices: true,
            seed: 1,
        }
    }
}

/// Where the cracks are in a mix.
#[derive(Debug, Clone, Default, Serialize)]
pub struct MixLabels {
    pub first_crack_start: Option<f64>,
    pub first_crack_end: Option<f64>,
    pub second_crack_start: Option<f64>,
    /// Every pop placed: `(t, band)`.
    pub pops: Vec<(f64, Band)>,
    /// Voices: `(from, to)`.
    pub voices: Vec<(f64, f64)>,
}

/// A mixed sound and its labels.
#[derive(Debug, Clone)]
pub struct Mixed {
    pub sound: Sound,
    pub labels: MixLabels,
}

/// Cuts the strongest crack-band pops out of a recording: 10 ms before to
/// 40 ms after each, faded in and out, scaled to a peak of 1. At most `max`.
#[must_use]
pub fn cut_pops(sound: &Sound, max: usize) -> Vec<Vec<f32>> {
    let mut l = Listener::new(sound.rate, Sensitivity::Normal);
    let mut found = Vec::new();
    for chunk in sound.samples.chunks((sound.rate / 10).max(1) as usize) {
        found.extend(
            l.push(chunk)
                .pops
                .into_iter()
                .filter(|p| p.band == Band::Crack),
        );
    }
    found.sort_by(|a, b| b.strength_db.total_cmp(&a.strength_db));
    let rate = f64::from(sound.rate);
    found
        .iter()
        .take(max)
        .filter_map(|p| {
            let from = ((p.t - 0.010) * rate).max(0.0) as usize;
            let to = (((p.t + 0.040) * rate) as usize).min(sound.samples.len());
            (to > from + 16).then(|| {
                let mut v = sound.samples[from..to].to_vec();
                let n = v.len();
                let fade = (n / 8).max(1);
                for i in 0..fade {
                    let g = i as f32 / fade as f32;
                    v[i] *= g;
                    v[n - 1 - i] *= g;
                }
                let peak = v.iter().fold(0.0f32, |m, x| m.max(x.abs())).max(1e-6);
                v.iter_mut().for_each(|x| *x /= peak);
                v
            })
        })
        .collect()
}

/// Synthetic pops, when no recording to cut from is at hand.
#[must_use]
pub fn synthetic_pops(rate: u32, n: usize, second_crack: bool, seed: u64) -> Vec<Vec<f32>> {
    let mut s = Synth::new(rate, seed);
    s.noise_rms = 1e-5;
    s.fc_pop_db = 80.0;
    s.sc_pop_db = 80.0;
    (0..n)
        .map(|_| {
            s.pop(second_crack);
            let mut v = s.take(0.05);
            let peak = v.iter().fold(0.0f32, |m, x| m.max(x.abs())).max(1e-6);
            v.iter_mut().for_each(|x| *x /= peak);
            v
        })
        .collect()
}

fn rms(v: &[f32]) -> f32 {
    if v.is_empty() {
        return 0.0;
    }
    (v.iter().map(|x| x * x).sum::<f32>() / v.len() as f32).sqrt()
}

/// Mixes pops into noise. `noise` is looped to length (synthetic fan noise
/// without one); `fc_bank` and `sc_bank` are the pops to draw from.
#[must_use]
pub fn mix(
    noise: Option<&Sound>,
    fc_bank: &[Vec<f32>],
    sc_bank: &[Vec<f32>],
    spec: &MixSpec,
    rate: u32,
) -> Mixed {
    let rate = noise.map_or(rate, |n| n.rate);
    let n = (spec.seconds * f64::from(rate)) as usize;
    let mut out: Vec<f32> = match noise {
        Some(src) if !src.samples.is_empty() => {
            src.samples.iter().copied().cycle().take(n).collect()
        }
        _ => Synth::new(rate, spec.seed ^ 0xfa11).take(spec.seconds),
    };
    out.resize(n, 0.0);
    let floor = rms(&out).max(1e-4);
    let mut rng = Rng::new(spec.seed);
    let mut labels = MixLabels::default();

    let place = |out: &mut Vec<f32>, t: f64, pop: &[f32], gain_db: f32| {
        let at = (t * f64::from(rate)) as usize;
        let amp = floor * 10f32.powf(gain_db / 20.0);
        for (i, &x) in pop.iter().enumerate() {
            if let Some(o) = out.get_mut(at + i) {
                *o += amp * x;
            }
        }
    };

    // First crack: rate and loudness swell over 20 s, hold, fade over 30 s.
    if !fc_bank.is_empty() {
        let mut t = spec.fc_at;
        let end = (spec.fc_at + spec.fc_s).min(spec.seconds - 0.1);
        while t < end {
            let into = t - spec.fc_at;
            let left = end - t;
            let env = (into / 20.0).min(1.0).min(left / 30.0).clamp(0.15, 1.0);
            let rate_now = spec.fc_rate * env;
            // Exponential gaps for a Poisson series.
            let gap = -(f64::from(rng.next_f32()).max(1e-6)).ln() / rate_now.max(0.01);
            let pop = &fc_bank[(rng.next_f32() * fc_bank.len() as f32) as usize % fc_bank.len()];
            let db = spec.pop_db + 6.0 * rng.signed() - 6.0 * (1.0 - env as f32);
            place(&mut out, t, pop, db);
            labels.pops.push((t, Band::Crack));
            labels.first_crack_start.get_or_insert(t);
            labels.first_crack_end = Some(t);
            t += gap.max(0.06);
        }
    }

    // Second crack: a little quieter (Wilson 2014: about 15 %), denser.
    if let (Some(sc), false) = (spec.sc_at, sc_bank.is_empty()) {
        let mut t = sc;
        while t < spec.seconds - 0.1 {
            let pop = &sc_bank[(rng.next_f32() * sc_bank.len() as f32) as usize % sc_bank.len()];
            place(&mut out, t, pop, spec.pop_db - 2.0 + 4.0 * rng.signed());
            labels.pops.push((t, Band::High));
            labels.second_crack_start.get_or_insert(t);
            t += (-(f64::from(rng.next_f32()).max(1e-6)).ln() / 3.0).max(0.06);
        }
    }

    // Voices, before first crack, at about the level of the pops.
    if spec.voices {
        let speech = voice(rate, 3.0, floor * 6.0);
        let mut t = 8.0;
        while t + 3.0 < spec.fc_at - 2.0 {
            let at = (t * f64::from(rate)) as usize;
            for (i, &x) in speech.iter().enumerate() {
                if let Some(o) = out.get_mut(at + i) {
                    *o += x;
                }
            }
            labels.voices.push((t, t + 3.0));
            t += 12.0 + 6.0 * f64::from(rng.next_f32());
        }
    }

    for x in &mut out {
        *x = x.clamp(-1.0, 1.0);
    }
    Mixed {
        sound: Sound { rate, samples: out },
        labels,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::offline::analyze;
    use roast_core::{CrackExpectation, EventKind};

    #[test]
    fn a_mixed_first_crack_with_voices_is_heard_near_its_label() {
        let fc = synthetic_pops(48_000, 12, false, 3);
        let spec = MixSpec {
            seconds: 150.0,
            fc_at: 60.0,
            ..MixSpec::default()
        };
        let m = mix(None, &fc, &[], &spec, 48_000);
        let label = m
            .labels
            .first_crack_start
            .unwrap_or_else(|| panic!("no pops"));
        assert!(!m.labels.voices.is_empty());
        let e = CrackExpectation::from_roasts(&[], &[]);
        let a = analyze(&m.sound.samples, 48_000, Sensitivity::Normal, None, 0.0, &e);
        let heard = a
            .heard
            .iter()
            .find(|h| h.kind == EventKind::FirstCrackStart)
            .unwrap_or_else(|| panic!("not heard: {:?}", a.per_10s));
        assert!(
            (heard.t - label).abs() < 10.0,
            "heard {} labelled {label}",
            heard.t
        );
    }

    #[test]
    fn pops_cut_from_a_recording_are_short_and_scaled() {
        let mut s = Synth::new(48_000, 9);
        let mut v = s.take(2.0);
        for _ in 0..6 {
            s.pop(false);
            v.extend(s.take(0.5));
        }
        let cut = cut_pops(
            &Sound {
                rate: 48_000,
                samples: v,
            },
            4,
        );
        assert_eq!(cut.len(), 4);
        for c in &cut {
            assert!(c.len() <= 2400);
            let peak = c.iter().fold(0.0f32, |m, x| m.max(x.abs()));
            assert!((peak - 1.0).abs() < 1e-3);
        }
    }
}
