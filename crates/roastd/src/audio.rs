//! Listening for cracks (ADR 0021): the microphone, or the simulator's
//! synthetic roaster, on a thread of its own that turns sound into pops and
//! spectrogram columns and hands them to the engine ten times a second.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc as std_mpsc;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use roast_audio::{Heard, Listener, Sensitivity, Synth};
use serde::{Deserialize, Serialize};

/// The settings key in the database.
pub const SETTINGS_KEY: &str = "audio";
/// How often the capture thread hands its results on.
const CHUNK: Duration = Duration::from_millis(100);
/// This long without a single non-zero sample counts as silence (on macOS a
/// microphone without permission delivers zeros).
const SILENT_AFTER: Duration = Duration::from_secs(3);

/// What a heard crack does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AudioMode {
    /// Marks heard cracks (origin `detector`); the person's mark wins.
    #[default]
    Mark,
    /// Shows the pops and the rate, marks nothing.
    Show,
    /// The microphone stays closed.
    Off,
}

/// The person's choices for listening.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct AudioSettings {
    pub mode: AudioMode,
    /// Input device by name; `None`: the system's default input.
    pub device: Option<String>,
    pub sensitivity: Sensitivity,
    /// Keep the sound of each roast as FLAC beside it (ADR 0021 §9).
    pub record: bool,
}

/// The settings, from the database or the defaults.
#[must_use]
pub fn load_settings(store: &roast_store::Store) -> AudioSettings {
    store
        .setting::<AudioSettings>(SETTINGS_KEY)
        .ok()
        .flatten()
        .unwrap_or_default()
}

/// The names of the input devices.
#[must_use]
pub fn input_devices() -> Vec<String> {
    use cpal::traits::HostTrait as _;
    let host = cpal::default_host();
    host.input_devices()
        .map(|d| d.map(|d| d.to_string()).collect())
        .unwrap_or_default()
}

/// Where the sound comes from.
#[derive(Clone)]
pub enum Source {
    /// A microphone by name, or the default input.
    Microphone(Option<String>),
    /// The simulator: fan noise with pops at these rates (first crack,
    /// second crack; pops per second of sound), set by the engine.
    Synthetic(Arc<Mutex<(f64, f64)>>),
}

/// What the capture thread reports.
#[derive(Debug)]
pub enum AudioEvent {
    Started {
        device: String,
        rate: u32,
    },
    /// Results of the last ~100 ms. `end_s`: seconds of sound so far.
    Chunk {
        heard: Heard,
        end_s: f64,
        silent: bool,
    },
    Failed(String),
}

/// A running capture; dropping it stops it.
pub struct Capture {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl std::fmt::Debug for Capture {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Capture").finish_non_exhaustive()
    }
}

impl Capture {
    /// Starts listening; `send` gets the events (from the capture thread).
    pub fn start(
        source: Source,
        sensitivity: Sensitivity,
        send: impl Fn(AudioEvent) + Send + 'static,
    ) -> Capture {
        let stop = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&stop);
        let thread = std::thread::Builder::new()
            .name("roast-audio".into())
            .spawn(move || match source {
                Source::Microphone(device) => {
                    microphone(device.as_deref(), sensitivity, &flag, &send)
                }
                Source::Synthetic(rates) => synthetic(&rates, sensitivity, &flag, &send),
            })
            .ok();
        Capture { stop, thread }
    }
}

impl Drop for Capture {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            // The thread notices within one chunk; do not block the engine
            // on a hanging audio driver.
            std::thread::spawn(move || {
                let _ = t.join();
            });
        }
    }
}

/// Feeds samples to the listener in chunks and reports them.
struct Feeder<'a> {
    listener: Listener,
    total: u64,
    rate: u32,
    last_sound: Instant,
    send: &'a (dyn Fn(AudioEvent) + Send),
}

impl Feeder<'_> {
    fn feed(&mut self, samples: &[f32]) {
        if samples.iter().any(|&s| s != 0.0) {
            self.last_sound = Instant::now();
        }
        let heard = self.listener.push(samples);
        self.total += samples.len() as u64;
        (self.send)(AudioEvent::Chunk {
            heard,
            end_s: self.total as f64 / f64::from(self.rate),
            silent: self.last_sound.elapsed() >= SILENT_AFTER,
        });
    }
}

fn microphone(
    name: Option<&str>,
    sensitivity: Sensitivity,
    stop: &AtomicBool,
    send: &(dyn Fn(AudioEvent) + Send),
) {
    use cpal::traits::{DeviceTrait as _, HostTrait as _, StreamTrait as _};
    let host = cpal::default_host();
    let device = match name {
        Some(n) => host
            .input_devices()
            .ok()
            .and_then(|mut it| it.find(|d| d.to_string() == n)),
        None => host.default_input_device(),
    };
    let Some(device) = device else {
        send(AudioEvent::Failed(match name {
            Some(n) => format!("the microphone \"{n}\" was not found"),
            None => "no microphone was found".into(),
        }));
        return;
    };
    let config = match device.default_input_config() {
        Ok(c) => c,
        Err(e) => {
            send(AudioEvent::Failed(format!(
                "the microphone cannot be opened: {e}"
            )));
            return;
        }
    };
    let rate = config.sample_rate();
    let channels = usize::from(config.channels()).max(1);
    let (tx, rx) = std_mpsc::channel::<Vec<f32>>();
    let err = |e: cpal::Error| tracing::warn!("microphone: {e}");
    let stream = match config.sample_format() {
        cpal::SampleFormat::F32 => {
            device.build_input_stream(config.into(), mono::<f32>(channels, tx), err, None)
        }
        cpal::SampleFormat::I16 => {
            device.build_input_stream(config.into(), mono::<i16>(channels, tx), err, None)
        }
        cpal::SampleFormat::I32 => {
            device.build_input_stream(config.into(), mono::<i32>(channels, tx), err, None)
        }
        cpal::SampleFormat::U16 => {
            device.build_input_stream(config.into(), mono::<u16>(channels, tx), err, None)
        }
        other => {
            send(AudioEvent::Failed(format!(
                "the microphone's sample format {other} is not supported"
            )));
            return;
        }
    };
    let stream = match stream.and_then(|s| s.play().map(|()| s)) {
        Ok(s) => s,
        Err(e) => {
            send(AudioEvent::Failed(format!(
                "the microphone cannot be opened: {e}"
            )));
            return;
        }
    };
    send(AudioEvent::Started {
        device: device.to_string(),
        rate,
    });
    let mut feeder = Feeder {
        listener: Listener::new(rate, sensitivity),
        total: 0,
        rate,
        last_sound: Instant::now(),
        send,
    };
    let mut buf = Vec::new();
    let mut next = Instant::now() + CHUNK;
    while !stop.load(Ordering::Relaxed) {
        match rx.recv_timeout(next.saturating_duration_since(Instant::now())) {
            Ok(v) => buf.extend(v),
            Err(std_mpsc::RecvTimeoutError::Timeout) => {}
            Err(std_mpsc::RecvTimeoutError::Disconnected) => {
                send(AudioEvent::Failed("the microphone stopped".into()));
                break;
            }
        }
        if Instant::now() >= next {
            next += CHUNK;
            feeder.feed(&buf);
            buf.clear();
        }
    }
    drop(stream);
}

/// A stream callback that mixes the channels down and sends the samples on.
fn mono<T>(
    channels: usize,
    tx: std_mpsc::Sender<Vec<f32>>,
) -> impl FnMut(&[T], &cpal::InputCallbackInfo) + Send + 'static
where
    T: cpal::Sample + Send + 'static,
    f32: cpal::FromSample<T>,
{
    move |data: &[T], _| {
        let v: Vec<f32> = data
            .chunks(channels)
            .map(|frame| {
                frame
                    .iter()
                    .map(|&s| <f32 as cpal::FromSample<T>>::from_sample_(s))
                    .sum::<f32>()
                    / channels as f32
            })
            .collect();
        let _ = tx.send(v);
    }
}

fn synthetic(
    rates: &Mutex<(f64, f64)>,
    sensitivity: Sensitivity,
    stop: &AtomicBool,
    send: &(dyn Fn(AudioEvent) + Send),
) {
    const RATE: u32 = 48_000;
    let mut synth = Synth::new(RATE, 0x5eed);
    send(AudioEvent::Started {
        device: "Simulator".into(),
        rate: RATE,
    });
    let mut feeder = Feeder {
        listener: Listener::new(RATE, sensitivity),
        total: 0,
        rate: RATE,
        last_sound: Instant::now(),
        send,
    };
    let per_chunk = (RATE / 10) as usize;
    let mut buf = vec![0.0; per_chunk];
    let mut next = Instant::now();
    while !stop.load(Ordering::Relaxed) {
        if let Ok(r) = rates.lock() {
            (synth.fc_rate, synth.sc_rate) = *r;
        }
        synth.render(&mut buf);
        feeder.feed(&buf);
        next += CHUNK;
        std::thread::sleep(next.saturating_duration_since(Instant::now()));
    }
}

/// Simulated pops per second of roast time at a bean temperature `d`
/// degrees above where first crack is expected: (first crack, second crack).
#[must_use]
pub fn simulated_rates(d: f64) -> (f64, f64) {
    let fc = if d < -1.0 {
        0.0
    } else if d < 2.0 {
        0.25 + 0.15 * (d + 1.0)
    } else if d < 7.0 {
        0.7
    } else if d < 11.0 {
        0.7 - 0.15 * (d - 7.0)
    } else {
        0.0
    };
    let sc = if d >= 22.0 { 3.0 } else { 0.0 };
    (fc.max(0.0), sc)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn settings_default_to_marking_with_the_default_microphone() {
        let s: AudioSettings = serde_json::from_str("{}").unwrap();
        assert_eq!(s.mode, AudioMode::Mark);
        assert_eq!(s.device, None);
        assert!(!s.record);
        let s: AudioSettings = serde_json::from_str(r#"{"mode":"show"}"#).unwrap();
        assert_eq!(s.mode, AudioMode::Show);
    }

    #[test]
    fn the_synthetic_roaster_is_heard() {
        let rates = Arc::new(Mutex::new((8.0, 0.0)));
        let pops = Arc::new(Mutex::new(0usize));
        let seen = Arc::clone(&pops);
        let started = Arc::new(AtomicBool::new(false));
        let s2 = Arc::clone(&started);
        let cap = Capture::start(
            Source::Synthetic(rates),
            Sensitivity::Normal,
            move |ev| match ev {
                AudioEvent::Started { .. } => s2.store(true, Ordering::Relaxed),
                AudioEvent::Chunk { heard, .. } => {
                    *seen.lock().unwrap() += heard.pops.len();
                }
                AudioEvent::Failed(e) => panic!("{e}"),
            },
        );
        std::thread::sleep(Duration::from_millis(1500));
        drop(cap);
        assert!(started.load(Ordering::Relaxed));
        let n = *pops.lock().unwrap();
        assert!(n >= 4, "{n} pops in 1.5 s at 8 a second");
    }

    #[test]
    fn simulated_first_crack_rises_holds_and_fades() {
        assert_eq!(simulated_rates(-5.0), (0.0, 0.0));
        assert!(simulated_rates(0.0).0 > 0.3);
        assert_eq!(simulated_rates(5.0).0, 0.7);
        assert_eq!(simulated_rates(15.0).0, 0.0);
        assert_eq!(simulated_rates(25.0).1, 3.0);
    }
}
