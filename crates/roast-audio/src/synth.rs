//! A synthetic roaster: fan noise with first- and second-crack pops at a
//! given rate. It stands in for the microphone in the simulator and makes
//! test sound with exact labels (ADR 0021 §11).

/// A small, deterministic random generator (xorshift64*).
#[derive(Debug, Clone)]
pub struct Rng(u64);

impl Rng {
    #[must_use]
    pub fn new(seed: u64) -> Self {
        Rng(seed.max(1))
    }

    /// Uniform in [0, 1).
    pub fn next_f32(&mut self) -> f32 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        let x = self.0.wrapping_mul(0x2545_F491_4F6C_DD1D);
        (x >> 40) as f32 / (1u64 << 24) as f32
    }

    /// Uniform in [-1, 1).
    pub fn signed(&mut self) -> f32 {
        self.next_f32() * 2.0 - 1.0
    }
}

#[derive(Debug, Clone)]
struct Burst {
    /// Samples left.
    left: usize,
    amp: f32,
    decay: f32,
    /// Two-pole resonator, or a high-pass for second crack.
    kind: BurstKind,
    y1: f32,
    y2: f32,
    x1: f32,
}

#[derive(Debug, Clone, Copy)]
enum BurstKind {
    Resonator { a1: f32, a2: f32, gain: f32 },
    HighPass,
}

/// Fan noise with pops.
#[derive(Debug, Clone)]
pub struct Synth {
    rate: u32,
    rng: Rng,
    lp: f32,
    hum_phase: f32,
    bursts: Vec<Burst>,
    /// First-crack pops per second of sound.
    pub fc_rate: f64,
    /// Second-crack pops per second of sound.
    pub sc_rate: f64,
    /// RMS of the fan noise, full scale.
    pub noise_rms: f32,
    /// First-crack pop peak above the noise RMS, dB.
    pub fc_pop_db: f32,
    /// Second-crack pop peak above the noise RMS, dB.
    pub sc_pop_db: f32,
}

impl Synth {
    #[must_use]
    pub fn new(rate: u32, seed: u64) -> Self {
        Synth {
            rate,
            rng: Rng::new(seed),
            lp: 0.0,
            hum_phase: 0.0,
            bursts: Vec::new(),
            fc_rate: 0.0,
            sc_rate: 0.0,
            noise_rms: 0.02,
            fc_pop_db: 24.0,
            sc_pop_db: 18.0,
        }
    }

    /// Starts a pop now.
    pub fn pop(&mut self, second_crack: bool) {
        let rate = self.rate as f32;
        let r = &mut self.rng;
        let (kind, tau_s, db) = if second_crack {
            (BurstKind::HighPass, 0.0012, self.sc_pop_db)
        } else {
            let f0 = 700.0 + 1800.0 * r.next_f32();
            let w = std::f32::consts::TAU * f0 / rate;
            let radius: f32 = 0.985;
            (
                BurstKind::Resonator {
                    a1: 2.0 * radius * w.cos(),
                    a2: -radius * radius,
                    gain: 1.0 - radius,
                },
                0.002 + 0.002 * r.next_f32(),
                self.fc_pop_db + 4.0 * r.signed(),
            )
        };
        let tau = tau_s * rate;
        self.bursts.push(Burst {
            left: (tau * 8.0) as usize,
            amp: self.noise_rms * 10f32.powf(db / 20.0),
            decay: (-1.0 / tau).exp(),
            kind,
            y1: 0.0,
            y2: 0.0,
            x1: 0.0,
        });
    }

    /// Fills `out` with the next samples.
    pub fn render(&mut self, out: &mut [f32]) {
        let rate = f64::from(self.rate);
        let p_fc = self.fc_rate / rate;
        let p_sc = self.sc_rate / rate;
        let lp_a = 0.35;
        // White noise through a one-pole low-pass has about this RMS ratio.
        let norm = self.noise_rms * 1.9;
        let hum_step = std::f32::consts::TAU * 100.0 / self.rate as f32;
        for o in out.iter_mut() {
            if f64::from(self.rng.next_f32()) < p_fc {
                self.pop(false);
            }
            if f64::from(self.rng.next_f32()) < p_sc {
                self.pop(true);
            }
            let w = self.rng.signed();
            self.lp += lp_a * (w - self.lp);
            self.hum_phase = (self.hum_phase + hum_step) % std::f32::consts::TAU;
            let mut s =
                norm * (0.8 * self.lp + 0.2 * w) + self.noise_rms * 0.5 * self.hum_phase.sin();
            for b in &mut self.bursts {
                let x = self.rng.signed() * b.amp;
                let y = match b.kind {
                    BurstKind::Resonator { a1, a2, gain } => {
                        let y = gain * 6.0 * x + a1 * b.y1 + a2 * b.y2;
                        b.y2 = b.y1;
                        b.y1 = y;
                        y
                    }
                    BurstKind::HighPass => {
                        let y = x - b.x1;
                        b.x1 = x;
                        y
                    }
                };
                s += y;
                b.amp *= b.decay;
                b.left = b.left.saturating_sub(1);
            }
            self.bursts.retain(|b| b.left > 0);
            *o = s.clamp(-1.0, 1.0);
        }
    }

    /// `seconds` of sound.
    #[must_use]
    pub fn take(&mut self, seconds: f64) -> Vec<f32> {
        let mut v = vec![0.0; (seconds * f64::from(self.rate)) as usize];
        self.render(&mut v);
        v
    }
}

/// A voice-like distractor: syllables of a harmonic tone, ~150 ms each, four
/// a second. Used to check that speech is not taken for cracks.
#[must_use]
pub fn voice(rate: u32, seconds: f64, amp: f32) -> Vec<f32> {
    let n = (seconds * f64::from(rate)) as usize;
    (0..n)
        .map(|i| {
            let t = i as f32 / rate as f32;
            let syl = (t * 4.0).fract();
            let env = if syl < 0.6 {
                (syl / 0.05).min(1.0) * ((0.6 - syl) / 0.1).min(1.0)
            } else {
                0.0
            };
            let f0 = 140.0 + 30.0 * (t * 1.3).sin();
            let tone: f32 = (1..=20)
                .map(|h| (std::f32::consts::TAU * f0 * h as f32 * t).sin() / h as f32)
                .sum();
            amp * env * tone
        })
        .collect()
}
