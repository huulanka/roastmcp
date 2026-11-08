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
/// Connection attempts without a single reading after which the thread gives
/// up. Only applies before the machine first answered: once it has, a lost link
/// is retried for as long as the connection is wanted (a roast may be running).
const GIVE_UP_AFTER_SILENT_ATTEMPTS: u32 = 3;
/// Pause between reconnect attempts.
const RECONNECT_EVERY: Duration = if cfg!(test) {
    Duration::from_millis(50)
} else {
    Duration::from_secs(3)
};

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
    /// The machine never answered since the connection was started, several
    /// attempts in a row. The thread has stopped; the person has to connect
    /// again.
    GaveUp(String),
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

    // Attempts so far, and whether the machine ever answered. Giving up only
    // makes sense for a machine that never answered (wrong port, BT session
    // already used up); a link lost later is retried indefinitely.
    let mut attempts = 0u32;
    let mut answered = false;
    let mut last_error: Option<String> = None;
    'connect: loop {
        if !answered && attempts >= GIVE_UP_AFTER_SILENT_ATTEMPTS {
            let detail = last_error
                .map(|e| format!(" (last: {e})"))
                .unwrap_or_default();
            send(DriverEvent::GaveUp(format!(
                "the roaster did not answer in {attempts} attempts{detail}"
            )));
            break 'connect;
        }
        attempts += 1;
        match driver.connect() {
            Ok(warnings) => {
                if !send(DriverEvent::Connected(warnings)) {
                    break 'connect;
                }
            }
            Err(e) => {
                last_error = Some(e.to_string());
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
                    answered = true;
                    if !send(DriverEvent::Reading(r)) {
                        break 'connect;
                    }
                }
                Err(e) => {
                    failures += 1;
                    let msg = e.to_string();
                    last_error = Some(msg.clone());
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

#[cfg(test)]
mod tests {
    use super::*;
    use roast_driver::DriverError;

    /// Opens fine, acknowledges nothing, never answers a read.
    struct Silent;

    impl RoasterDriver for Silent {
        fn connect(&mut self) -> Result<Vec<String>, DriverError> {
            Ok(vec!["no acknowledgement for \"CHAN;2100\"".into()])
        }
        fn read(&mut self) -> Result<Reading, DriverError> {
            Err(DriverError::Timeout(600))
        }
        fn apply(&mut self, _cmd: &ActuatorCommand) -> Result<(), DriverError> {
            Ok(())
        }
        fn close(&mut self) {}
    }

    #[test]
    fn gives_up_when_the_machine_never_answers() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let _cmd = spawn_driver(Box::new(Silent), Duration::from_millis(10), 1, tx);
        let deadline = Instant::now() + Duration::from_secs(30);
        let mut connects = 0;
        loop {
            assert!(Instant::now() < deadline, "never gave up");
            match rx.blocking_recv() {
                Some(Msg::Driver(_, DriverEvent::Connected(_))) => connects += 1,
                Some(Msg::Driver(_, DriverEvent::GaveUp(_))) => break,
                Some(_) => {}
                None => panic!("channel closed before GaveUp"),
            }
        }
        assert_eq!(connects, GIVE_UP_AFTER_SILENT_ATTEMPTS);
    }

    /// Answers a few reads, then falls silent for good (BT switched off).
    struct AnswersThenSilent(u32);

    impl RoasterDriver for AnswersThenSilent {
        fn connect(&mut self) -> Result<Vec<String>, DriverError> {
            Ok(Vec::new())
        }
        fn read(&mut self) -> Result<Reading, DriverError> {
            if self.0 == 0 {
                return Err(DriverError::Timeout(600));
            }
            self.0 -= 1;
            Ok(Reading::default())
        }
        fn apply(&mut self, _cmd: &ActuatorCommand) -> Result<(), DriverError> {
            Ok(())
        }
        fn close(&mut self) {}
    }

    #[test]
    fn keeps_reconnecting_after_the_machine_has_answered() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let _cmd = spawn_driver(
            Box::new(AnswersThenSilent(2)),
            Duration::from_millis(10),
            1,
            tx,
        );
        let deadline = Instant::now() + Duration::from_secs(30);
        let (mut readings, mut connects, mut lost) = (0, 0, 0);
        while connects < GIVE_UP_AFTER_SILENT_ATTEMPTS + 3 {
            assert!(Instant::now() < deadline, "stopped reconnecting");
            match rx.blocking_recv() {
                Some(Msg::Driver(_, DriverEvent::Connected(_))) => connects += 1,
                Some(Msg::Driver(_, DriverEvent::Reading(_))) => readings += 1,
                Some(Msg::Driver(_, DriverEvent::Lost(_))) => lost += 1,
                Some(Msg::Driver(_, DriverEvent::GaveUp(e))) => panic!("gave up: {e}"),
                Some(_) => {}
                None => panic!("channel closed"),
            }
        }
        assert_eq!(readings, 2);
        assert!(lost >= GIVE_UP_AFTER_SILENT_ATTEMPTS, "{lost}");
    }
}
