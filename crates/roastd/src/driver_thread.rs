//! The driver runs on its own thread: it samples at a fixed interval and
//! applies approved commands as soon as they arrive, so a slow or stuck port
//! never blocks the engine (and with it the emergency stop logic).

use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::{Duration, Instant};

use roast_core::{ActuatorCommand, EventKind};
use roast_driver::{Reading, RoasterDriver};
use tokio::sync::mpsc::UnboundedSender;

use crate::engine::Msg;

/// Failed reads in a row after which the link counts as lost.
const LOST_AFTER_FAILURES: u32 = 3;
/// Pause between reconnect attempts.
const RECONNECT_EVERY: Duration = Duration::from_secs(3);

/// From the engine to the driver thread.
pub enum DriverCmd {
    /// Send an approved command.
    Apply(ActuatorCommand),
    /// An event was marked.
    Event(EventKind),
    /// Close the connection and end the thread.
    Stop,
}

/// From the driver thread to the engine.
pub enum DriverEvent {
    /// Connected (again), with init warnings.
    Connected(Vec<String>),
    /// A reading.
    Reading(Reading),
    /// A single failure.
    Error(String),
    /// The link is lost; the thread keeps trying to reconnect.
    Lost(String),
}

/// Starts the driver thread and returns its command channel.
pub fn spawn_driver(
    driver: Box<dyn RoasterDriver>,
    interval: Duration,
    generation: u64,
    engine: UnboundedSender<Msg>,
) -> mpsc::Sender<DriverCmd> {
    let (tx, rx) = mpsc::channel();
    let spawned = std::thread::Builder::new()
        .name("roaster-driver".into())
        .spawn(move || run(driver, interval, generation, &rx, &engine));
    if let Err(e) = spawned {
        tracing::error!("could not start the driver thread: {e}");
    }
    tx
}

enum Wait {
    Elapsed,
    Stop,
}

fn run(
    mut driver: Box<dyn RoasterDriver>,
    interval: Duration,
    generation: u64,
    rx: &mpsc::Receiver<DriverCmd>,
    engine: &UnboundedSender<Msg>,
) {
    let send = |ev: DriverEvent| engine.send(Msg::Driver(generation, ev)).is_ok();

    'connect: loop {
        match driver.connect() {
            Ok(warnings) => {
                if !send(DriverEvent::Connected(warnings)) {
                    break 'connect;
                }
            }
            Err(e) => {
                if !send(DriverEvent::Lost(format!("cannot connect: {e}"))) {
                    break 'connect;
                }
                driver.close();
                if let Wait::Stop = wait(&mut *driver, rx, Instant::now() + RECONNECT_EVERY, &send)
                {
                    break 'connect;
                }
                continue 'connect;
            }
        }

        let mut failures = 0;
        let mut next = Instant::now();
        loop {
            if let Wait::Stop = wait(&mut *driver, rx, next, &send) {
                break 'connect;
            }
            next += interval;
            let now = Instant::now();
            if next < now {
                next = now + interval;
            }
            match driver.read() {
                Ok(r) => {
                    failures = 0;
                    if !send(DriverEvent::Reading(r)) {
                        break 'connect;
                    }
                }
                Err(e) => {
                    failures += 1;
                    let msg = e.to_string();
                    if failures >= LOST_AFTER_FAILURES {
                        if !send(DriverEvent::Lost(msg)) {
                            break 'connect;
                        }
                        driver.close();
                        if let Wait::Stop =
                            wait(&mut *driver, rx, Instant::now() + RECONNECT_EVERY, &send)
                        {
                            break 'connect;
                        }
                        continue 'connect;
                    }
                    if !send(DriverEvent::Error(msg)) {
                        break 'connect;
                    }
                }
            }
        }
    }
    driver.close();
}

/// Applies commands until `until`; returns early on Stop.
fn wait(
    driver: &mut dyn RoasterDriver,
    rx: &mpsc::Receiver<DriverCmd>,
    until: Instant,
    send: &impl Fn(DriverEvent) -> bool,
) -> Wait {
    loop {
        let now = Instant::now();
        if now >= until {
            return Wait::Elapsed;
        }
        match rx.recv_timeout(until - now) {
            Ok(DriverCmd::Apply(cmd)) => {
                if let Err(e) = driver.apply(&cmd) {
                    send(DriverEvent::Error(format!(
                        "could not send {} {}: {e}",
                        cmd.actuator, cmd.value
                    )));
                }
            }
            Ok(DriverCmd::Event(kind)) => driver.on_event(kind),
            Ok(DriverCmd::Stop) | Err(RecvTimeoutError::Disconnected) => return Wait::Stop,
            Err(RecvTimeoutError::Timeout) => return Wait::Elapsed,
        }
    }
}
