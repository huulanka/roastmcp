//! The engine's ear (ADR 0021): starts and stops listening with the
//! recording, maps pops to the roast clock, marks heard cracks and sends the
//! app what it hears.

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
}

impl Hearing {
    pub(crate) fn new(settings: AudioSettings) -> Self {
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
        }
    }

    pub fn audio_settings(&self) -> AudioSettings {
        self.hearing.settings.clone()
    }

    /// Starts listening for the running recording (unless switched off).
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
        let generation = self.hearing.generation;
        let tx = self.tx.clone();
        self.hearing.capture = Some(Capture::start(
            source,
            self.hearing.settings.sensitivity,
            move |ev| {
                let _ = tx.send(Msg::Audio(generation, ev));
            },
        ));
    }

    /// Stops listening; what was heard stays for the roast.
    pub(crate) fn stop_hearing(&mut self) {
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
        (!s.pops.is_empty() || !s.levels.is_empty()).then(|| s.clone())
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
        if self.roast.is_none() {
            return;
        }
        let scale = self.hearing.scale;
        let now = self.t();
        let zero = *self.hearing.zero.get_or_insert(now - end_s * scale);
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
        if self.hearing.settings.mode != AudioMode::Mark {
            return;
        }
        let bt = self.bt_at(h.t);
        let charge = self.roast.as_ref().and_then(roast_core::Roast::charge_t);
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
