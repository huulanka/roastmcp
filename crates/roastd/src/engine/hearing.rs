//! The engine's ear (ADR 0021): starts and stops listening with the
//! recording, maps pops to the roast clock, marks heard cracks and sends the
//! app what it hears.

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex};

use base64::Engine as _;
use roast_audio::crack::{CrackLogic, HeardCrack, RoastContext};
use roast_audio::{Band, Heard};
use roast_core::{
    EventKind, EventOrigin, FIRST_CRACK_WINDOW_C, RoastEvent, RoastSound, SoundLevel, SoundPop,
};
use serde::Serialize;

use super::{Engine, Msg};
use crate::audio::{self, AudioEvent, AudioMode, AudioSettings, Capture, Source};
use crate::driver_thread::DriverCmd;

/// The engine's listening state.
pub(crate) struct Hearing {
    pub(crate) settings: AudioSettings,
    capture: Option<Capture>,
    pub(crate) generation: u64,
    state: &'static str,
    error: Option<String>,
    device: Option<String>,
    rate: Option<u32>,
    logic: CrackLogic,
    /// Recording clock at sound time 0.
    pub(crate) zero: Option<f64>,
    /// Sound time scale (the simulator's speed).
    scale: f64,
    pub(crate) sound: RoastSound,
    crack_db: Option<f32>,
    high_db: Option<f32>,
    /// Pops per second of sound the synthetic roaster makes, set from the
    /// simulated bean temperature.
    synth: Option<Arc<Mutex<(f64, f64)>>>,
    /// The fan's noise per machine and air level (ADR 0021 §10).
    noise: HashMap<String, BTreeMap<u32, NoiseLevel>>,
    /// Recording time of the latest pop.
    last_pop: f64,
}

/// The settings key of the learned noise.
const NOISE_KEY: &str = "audio_noise";
/// Air is learned in steps of this many percent.
const AIR_STEP: f64 = 10.0;
/// No pop for this long: the frame is the fan's, s.
const QUIET_S: f64 = 2.0;

/// The bands' level at one air setting, learned while nothing pops.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, serde::Deserialize)]
pub struct NoiseLevel {
    pub crack_db: f32,
    #[serde(default)]
    pub high_db: Option<f32>,
    /// Seconds learned (frames of 100 ms / 10).
    pub seconds: f32,
}

impl NoiseLevel {
    fn learn(&mut self, crack: f32, high: Option<f32>) {
        // A plain mean at first, then a slow moving average.
        let w = (0.1 / self.seconds.max(0.1)).max(0.01);
        self.crack_db += w * (crack - self.crack_db);
        self.high_db = match (self.high_db, high) {
            (Some(a), Some(b)) => Some(a + w * (b - a)),
            (a, b) => b.or(a),
        };
        self.seconds += 0.1;
    }
}

/// The learned noise at one air setting, for the app.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct NoiseRow {
    pub air: u32,
    #[serde(flatten)]
    pub level: NoiseLevel,
}

impl Hearing {
    pub(crate) fn new(settings: AudioSettings, store: &roast_store::Store) -> Self {
        let noise = store.setting(NOISE_KEY).ok().flatten().unwrap_or_default();
        let logic = CrackLogic::new(settings.sensitivity);
        Hearing {
            settings,
            capture: None,
            generation: 0,
            state: "idle",
            error: None,
            device: None,
            rate: None,
            logic,
            zero: None,
            scale: 1.0,
            sound: RoastSound::default(),
            crack_db: None,
            high_db: None,
            synth: None,
            noise,
            last_pop: f64::NEG_INFINITY,
        }
    }
}

/// Listening as the snapshot shows it.
#[derive(Debug, Clone, Serialize)]
pub struct AudioStatus {
    mode: AudioMode,
    /// `off` (mode off), `idle` (not recording), `listening`, `silent` (only
    /// zeros: on macOS, no permission for the microphone), `error`.
    state: &'static str,
    device: Option<String>,
    rate: Option<u32>,
    error: Option<String>,
    /// Crack-band pops in the last 20 s.
    pub(crate) crack_20s: usize,
    /// High-band pops in the last 10 s.
    high_10s: usize,
    crack_db: Option<f32>,
    high_db: Option<f32>,
    /// The device listens to a simulator.
    simulated: bool,
    /// The cracks heard in this recording, as they were marked.
    heard: Vec<roast_core::HeardMark>,
    /// The fan's crack-band level at the current air, as learned, dBFS.
    pub(crate) fan_db: Option<f32>,
    /// How far the crack-band pops of the last 20 s rose above the noise
    /// (median), dB.
    pop_db: Option<f32>,
    /// The learned noise of this machine per air level.
    pub(crate) noise: Vec<NoiseRow>,
    /// The sound is being kept as a file.
    recording_sound: bool,
}

/// One pop in the WebSocket message.
#[derive(Debug, Clone, Copy, Serialize)]
struct WirePop {
    t: f64,
    band: Band,
    db: f32,
}

impl Engine {
    pub fn audio_status(&self) -> AudioStatus {
        let h = &self.hearing;
        AudioStatus {
            mode: h.settings.mode,
            state: if h.settings.mode == AudioMode::Off {
                "off"
            } else {
                h.state
            },
            device: h.device.clone(),
            rate: h.rate,
            error: h.error.clone(),
            crack_20s: h.logic.crack_rate(),
            high_10s: h.logic.high_rate(),
            crack_db: h.crack_db,
            high_db: h.high_db,
            simulated: h.synth.is_some(),
            heard: if self.roast.is_some() {
                h.sound.heard.clone()
            } else {
                Vec::new()
            },
            fan_db: self
                .noise_table()
                .and_then(|n| n.get(&self.air_bucket()?).map(|l| l.crack_db)),
            pop_db: self.recent_pop_db(),
            noise: self
                .noise_table()
                .map(|n| {
                    n.iter()
                        .map(|(&air, &level)| NoiseRow { air, level })
                        .collect()
                })
                .unwrap_or_default(),
            recording_sound: h.capture.is_some() && h.sound.file.is_some(),
        }
    }

    pub fn audio_settings(&self) -> AudioSettings {
        self.hearing.settings.clone()
    }

    pub(crate) fn hearing_active(&self) -> bool {
        self.hearing.capture.is_some()
    }

    fn machine_key(&self) -> Option<String> {
        self.machine.as_ref().map(|m| {
            if m.simulated {
                "simulator".to_owned()
            } else {
                m.id.clone()
            }
        })
    }

    fn noise_table(&self) -> Option<&BTreeMap<u32, NoiseLevel>> {
        self.hearing.noise.get(&self.machine_key()?)
    }

    /// The current air, in learning steps.
    fn air_bucket(&self) -> Option<u32> {
        let air = *self
            .safety
            .status(self.now())
            .actuators
            .get(&roast_core::Actuator::Air)?;
        Some(((air / AIR_STEP).round() * AIR_STEP) as u32)
    }

    fn recent_pop_db(&self) -> Option<f32> {
        let now = self.t();
        let mut v: Vec<f32> = self
            .hearing
            .sound
            .pops
            .iter()
            .rev()
            .take_while(|p| now - p.t <= 20.0)
            .filter(|p| p.band == Band::Crack)
            .map(|p| p.db)
            .collect();
        if v.is_empty() {
            return None;
        }
        v.sort_by(f32::total_cmp);
        Some(v[v.len() / 2])
    }

    /// Starts listening: for the running recording, or during the preheat
    /// to learn the fan (unless switched off).
    pub(crate) fn start_hearing(&mut self) {
        self.stop_hearing();
        let h = &mut self.hearing;
        h.logic = CrackLogic::new(h.settings.sensitivity);
        h.sound = RoastSound::default();
        h.zero = None;
        h.crack_db = None;
        h.high_db = None;
        h.error = None;
        if h.settings.mode == AudioMode::Off {
            return;
        }
        let simulated = self.machine.as_ref().is_some_and(|m| m.simulated);
        let source = if simulated {
            let rates = Arc::new(Mutex::new((0.0, 0.0)));
            self.hearing.synth = Some(Arc::clone(&rates));
            Source::Synthetic(rates)
        } else {
            self.hearing.synth = None;
            Source::Microphone(self.hearing.settings.device.clone())
        };
        self.hearing.generation += 1;
        self.hearing.state = "starting";
        self.hearing.scale = if simulated { self.time_scale } else { 1.0 };
        self.hearing.last_pop = f64::NEG_INFINITY;
        // The sound is kept only for a recording, from a microphone, when
        // the person switched it on (ADR 0021 §9).
        let record = match (&self.roast, self.hearing.settings.record && !simulated) {
            (Some(r), true) => {
                let name = format!("{}.flac", r.id);
                self.hearing.sound.file = Some(name.clone());
                Some(self.opts.roasts_dir.join("sound").join(name))
            }
            _ => None,
        };
        let generation = self.hearing.generation;
        let tx = self.tx.clone();
        self.hearing.capture = Some(Capture::start(
            source,
            self.hearing.settings.sensitivity,
            record,
            move |ev| {
                let _ = tx.send(Msg::Audio(generation, ev));
            },
        ));
    }

    /// Stops listening; what was heard stays for the roast, the learned
    /// noise is saved.
    pub(crate) fn stop_hearing(&mut self) {
        if self.hearing.capture.is_some()
            && let Err(e) = self.opts.store.set_setting(NOISE_KEY, &self.hearing.noise)
        {
            tracing::warn!("the learned noise could not be saved: {e}");
        }
        let h = &mut self.hearing;
        h.capture = None;
        h.generation += 1;
        h.synth = None;
        h.state = "idle";
        h.crack_db = None;
        h.high_db = None;
    }

    /// New settings: saved, and listening starts again with them.
    pub fn set_audio_settings(&mut self, settings: AudioSettings) -> AudioSettings {
        if let Err(e) = self.opts.store.set_setting(audio::SETTINGS_KEY, &settings) {
            tracing::warn!("the audio settings could not be saved: {e}");
        }
        let restart = self.roast.is_some()
            && (settings.mode != self.hearing.settings.mode
                || settings.device != self.hearing.settings.device
                || settings.sensitivity != self.hearing.settings.sensitivity);
        let keep = std::mem::take(&mut self.hearing.sound);
        self.hearing.settings = settings.clone();
        if restart {
            let logic = self.hearing.logic.clone();
            self.start_hearing();
            // What was heard before the change stays.
            self.hearing.sound = keep;
            self.hearing.logic = logic;
            self.hearing.logic.set_sensitivity(settings.sensitivity);
        } else {
            self.hearing.sound = keep;
        }
        self.publish(None);
        settings
    }

    /// What the roast's sound looks like when saved.
    pub(crate) fn roast_sound(&self) -> Option<RoastSound> {
        let s = &self.hearing.sound;
        (!s.pops.is_empty() || !s.levels.is_empty() || !s.heard.is_empty()).then(|| s.clone())
    }

    pub(crate) fn on_audio(&mut self, ev: AudioEvent) {
        match ev {
            AudioEvent::Started { device, rate } => {
                self.hearing.state = "listening";
                self.hearing.sound.device = Some(device.clone());
                self.hearing.device = Some(device);
                self.hearing.rate = Some(rate);
                self.publish(None);
            }
            AudioEvent::Failed(message) => {
                self.hearing.state = "error";
                self.hearing.error = Some(message.clone());
                self.hearing.capture = None;
                self.alarm(
                    "warning",
                    "microphone",
                    format!("Not listening for first crack: {message}."),
                );
                self.publish(None);
            }
            AudioEvent::Chunk {
                heard,
                end_s,
                silent,
            } => self.on_heard(heard, end_s, silent),
        }
    }

    fn on_heard(&mut self, heard: Heard, end_s: f64, silent: bool) {
        let scale = self.hearing.scale;
        let now = self.t();
        if self.hearing.zero.is_none() {
            self.hearing.zero = Some(now - end_s * scale);
            if self.roast.is_some() {
                self.hearing.sound.start_t = self.hearing.zero;
            }
        }
        let zero = self.hearing.zero.unwrap_or(now);
        if let Some(p) = heard.pops.last() {
            self.hearing.last_pop = zero + p.t * scale;
        }
        self.learn_noise(&heard, now);
        if self.roast.is_none() {
            // The preheat: only the fan is learned, and the app sees the sound.
            self.hearing.state = if silent { "silent" } else { "listening" };
            self.hearing.crack_db = heard.crack_db;
            self.hearing.high_db = heard.high_db;
            self.publish_audio(&heard, &[], now);
            return;
        }
        let was = self.hearing.state;
        self.hearing.state = if silent { "silent" } else { "listening" };
        if silent && was != "silent" {
            self.alarm(
                "warning",
                "microphone_silent",
                "The microphone delivers only silence: allow RoastMCP to use the microphone in the system settings (Privacy & Security).",
            );
        }
        self.hearing.crack_db = heard.crack_db;
        self.hearing.high_db = heard.high_db;
        if let Some(c) = heard.crack_db {
            let last = self.hearing.sound.levels.last().map(|l| l.t);
            if last.is_none_or(|l| now - l >= 1.0) {
                self.hearing.sound.levels.push(SoundLevel {
                    t: now,
                    crack_db: c,
                    high_db: heard.high_db,
                });
            }
        }
        let mut wire = Vec::with_capacity(heard.pops.len());
        for p in &heard.pops {
            let t = zero + p.t * scale;
            wire.push(WirePop {
                t,
                band: p.band,
                db: p.strength_db,
            });
            self.hearing.sound.pops.push(SoundPop {
                t,
                band: p.band,
                db: p.strength_db,
            });
            let ctx = self.crack_context();
            let pop = roast_audio::Pop { t, ..*p };
            if let Some(h) = self.hearing.logic.pop(pop, &ctx) {
                self.on_crack_heard(h);
            }
        }
        let ctx = self.crack_context();
        if let Some(h) = self.hearing.logic.tick(now, &ctx) {
            self.on_crack_heard(h);
        }
        self.publish_audio(&heard, &wire, now);
    }

    /// Frames without a pop nearby are the fan's: learned per air level.
    fn learn_noise(&mut self, heard: &Heard, now: f64) {
        let (Some(crack), Some(air), Some(key)) =
            (heard.crack_db, self.air_bucket(), self.machine_key())
        else {
            return;
        };
        if !heard.pops.is_empty() || now - self.hearing.last_pop < QUIET_S * self.hearing.scale {
            return;
        }
        self.hearing
            .noise
            .entry(key)
            .or_default()
            .entry(air)
            .or_insert(NoiseLevel {
                crack_db: crack,
                high_db: heard.high_db,
                seconds: 0.0,
            })
            .learn(crack, heard.high_db);
    }

    fn crack_context(&self) -> RoastContext {
        let events = self.roast.as_ref().map_or(&[][..], |r| r.events.as_slice());
        let ev = |k: EventKind| events.iter().find(|e| e.kind == k);
        RoastContext {
            bt: self.last_bt,
            charged: ev(EventKind::Charge).is_some(),
            dry_end: ev(EventKind::DryEnd).map(|e| e.t),
            first_crack: ev(EventKind::FirstCrackStart).map(|e| (e.t, e.bt)),
            first_crack_end: ev(EventKind::FirstCrackEnd).is_some(),
            second_crack: ev(EventKind::SecondCrackStart).is_some(),
            window: self
                .first_crack
                .map_or(FIRST_CRACK_WINDOW_C, |c| (c.low, c.high)),
        }
    }

    fn on_crack_heard(&mut self, h: HeardCrack) {
        let bt = self.bt_at(h.t);
        let charge = self.roast.as_ref().and_then(roast_core::Roast::charge_t);
        // Kept in every mode: the archive compares it with the marks.
        self.hearing.sound.heard.push(roast_core::HeardMark {
            kind: h.kind,
            t: h.t,
            bt,
            pops: h.pops,
        });
        if self.hearing.settings.mode != AudioMode::Mark {
            self.publish(None);
            return;
        }
        let Some(roast) = self.roast.as_mut() else {
            return;
        };
        // A heard mark never replaces anyone's.
        if roast.events.iter().any(|e| e.kind == h.kind) {
            return;
        }
        let event = RoastEvent {
            t: h.t,
            kind: h.kind,
            origin: EventOrigin::Detector,
            bt,
        };
        roast.events.push(event.clone());
        roast.events.sort_by(|a, b| a.t.total_cmp(&b.t));
        if let Some(d) = &self.driver {
            let _ = d.send(DriverCmd::Event(h.kind));
        }
        self.audit(serde_json::json!({"type": "event", "event": event, "pops": h.pops}));
        let clock = |t: f64| {
            let s = (t - charge.unwrap_or(0.0)).max(0.0).round() as i64;
            format!("{}:{:02}", s / 60, s % 60)
        };
        let at = match bt {
            Some(b) => format!("{} · {b:.1} °C", clock(h.t)),
            None => clock(h.t),
        };
        self.alarm(
            "info",
            "crack_heard",
            format!(
                "{} heard and marked: {at} ({} pops). Your mark replaces it.",
                h.kind, h.pops
            ),
        );
        // Steps after first crack start from the heard mark (ADR 0021 §6).
        self.run_recipe();
        self.publish(None);
    }

    /// The person undid a heard mark: it is not heard again in this roast.
    pub(crate) fn heard_mark_undone(&mut self, kind: EventKind) {
        self.hearing.logic.suppress(kind);
    }

    /// The bean temperature at recording time `t`, from the live curve.
    fn bt_at(&self, t: f64) -> Option<f64> {
        let p = &self.points;
        let i = p.partition_point(|x| x.t < t);
        let near = [i.checked_sub(1), Some(i)];
        near.iter()
            .flatten()
            .filter_map(|&j| p.get(j))
            .filter_map(|x| x.bt.map(|b| ((x.t - t).abs(), b)))
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map(|(_, b)| b)
            .or(self.last_bt)
    }

    /// The simulator's pops follow its bean temperature.
    pub(crate) fn drive_synthetic_cracks(&mut self) {
        let Some(rates) = &self.hearing.synth else {
            return;
        };
        let charged = self.roast.as_ref().is_some_and(|r| r.charge_t().is_some());
        let dropped = self
            .roast
            .as_ref()
            .is_some_and(|r| r.event(EventKind::Drop).is_some());
        let (fc, sc) = match (self.last_bt, self.first_crack) {
            (Some(bt), Some(c)) if charged && !dropped => audio::simulated_rates(bt - c.bt),
            _ => (0.0, 0.0),
        };
        if let Ok(mut r) = rates.lock() {
            *r = (fc * self.time_scale, sc * self.time_scale);
        }
    }

    fn publish_audio(&self, heard: &Heard, pops: &[WirePop], now: f64) {
        if self.updates.receiver_count() == 0 {
            return;
        }
        let bytes: Vec<u8> = heard.columns.iter().flatten().copied().collect();
        let msg = serde_json::json!({
            "type": "audio",
            "t": now,
            "columns": heard.columns.len(),
            "bands": roast_audio::COLUMN_BANDS,
            "per_s": f64::from(roast_audio::COLUMNS_PER_S) / self.hearing.scale,
            "spectrum": base64::engine::general_purpose::STANDARD.encode(bytes),
            "pops": pops,
            "crack_db": heard.crack_db,
            "high_db": heard.high_db,
            "crack_20s": self.hearing.logic.crack_rate(),
            "high_10s": self.hearing.logic.high_rate(),
        });
        let _ = self.updates.send(Arc::from(msg.to_string()));
    }
}
