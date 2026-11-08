//! Line-based transports: a real serial port and a scripted fake for tests.

use std::collections::VecDeque;
use std::io::{self, Read, Write};
use std::time::{Duration, Instant};

use roast_machine::{Parity, SerialConfig};

/// A line-oriented, half-duplex connection.
pub trait LineIo: Send {
    /// Writes one line; `ending` is appended.
    fn write_line(&mut self, line: &str, ending: &str) -> io::Result<()>;
    /// Reads one line (without its ending), or `None` after `timeout`.
    fn read_line(&mut self, timeout: Duration) -> io::Result<Option<String>>;
    /// Discards everything received but not read yet.
    fn clear_input(&mut self) -> io::Result<()>;
}

/// Port names that mean a direct Bluetooth link instead of a serial port:
/// `bluetooth:AKROASTER` (ADR 0013).
pub const BLUETOOTH_PREFIX: &str = "bluetooth:";

/// The serial ports the operating system offers, by name, followed on macOS by
/// paired Bluetooth serial devices as `bluetooth:<name>`.
#[must_use]
pub fn list_ports() -> Vec<String> {
    let mut ports: Vec<String> = serialport::available_ports()
        .unwrap_or_default()
        .into_iter()
        .map(|p| p.port_name)
        .collect();
    ports.sort();
    let mut bluetooth = roast_bluetooth::list_serial_devices();
    bluetooth.sort();
    ports.extend(
        bluetooth
            .into_iter()
            .map(|n| format!("{BLUETOOTH_PREFIX}{n}")),
    );
    ports
}

/// Takes one complete line (without its ending) off the front of `pending`.
fn take_line(pending: &mut Vec<u8>) -> Option<String> {
    let pos = pending.iter().position(|&b| b == b'\n')?;
    let line: Vec<u8> = pending.drain(..=pos).collect();
    Some(
        String::from_utf8_lossy(&line)
            .trim_end_matches(['\r', '\n'])
            .to_owned(),
    )
}

fn trace_incomplete(pending: &[u8]) {
    if !pending.is_empty() {
        tracing::trace!(
            target: "roast_driver::wire",
            "RX (incomplete, {} bytes) {:?}",
            pending.len(),
            String::from_utf8_lossy(pending)
        );
    }
}

/// A direct Bluetooth RFCOMM link (macOS; ADR 0013). Same line protocol as
/// the serial port, without the operating system's serial layer.
pub struct RfcommLineIo {
    link: roast_bluetooth::RfcommLink,
    pending: Vec<u8>,
}

impl RfcommLineIo {
    /// Opens the serial channel of the paired device `target` (its name, for
    /// example `AKROASTER`, or its address).
    ///
    /// # Errors
    /// When the device is not paired or does not answer, or Bluetooth direct is
    /// not available on this system.
    pub fn open(target: &str) -> io::Result<Self> {
        Ok(RfcommLineIo {
            link: roast_bluetooth::RfcommLink::open(target)?,
            pending: Vec::new(),
        })
    }
}

impl LineIo for RfcommLineIo {
    fn write_line(&mut self, line: &str, ending: &str) -> io::Result<()> {
        tracing::trace!(target: "roast_driver::wire", "TX {line:?}");
        let mut bytes = Vec::with_capacity(line.len() + ending.len());
        bytes.extend_from_slice(line.as_bytes());
        bytes.extend_from_slice(ending.as_bytes());
        self.link.write(&bytes)
    }

    fn read_line(&mut self, timeout: Duration) -> io::Result<Option<String>> {
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(line) = take_line(&mut self.pending) {
                tracing::trace!(target: "roast_driver::wire", "RX {line:?}");
                return Ok(Some(line));
            }
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                trace_incomplete(&self.pending);
                return Ok(None);
            }
            match self.link.recv(left)? {
                Some(bytes) => self.pending.extend_from_slice(&bytes),
                None => {
                    trace_incomplete(&self.pending);
                    return Ok(None);
                }
            }
        }
    }

    fn clear_input(&mut self) -> io::Result<()> {
        self.pending.clear();
        self.link.discard_pending()
    }
}

/// A serial port (USB serial or Bluetooth SPP).
pub struct SerialLineIo {
    port: Box<dyn serialport::SerialPort>,
    pending: Vec<u8>,
}

impl SerialLineIo {
    /// Opens `path` with the profile's settings.
    pub fn open(path: &str, cfg: &SerialConfig) -> io::Result<Self> {
        let data_bits = match cfg.data_bits {
            5 => serialport::DataBits::Five,
            6 => serialport::DataBits::Six,
            7 => serialport::DataBits::Seven,
            _ => serialport::DataBits::Eight,
        };
        let parity = match cfg.parity {
            Parity::None => serialport::Parity::None,
            Parity::Even => serialport::Parity::Even,
            Parity::Odd => serialport::Parity::Odd,
        };
        let stop_bits = if cfg.stop_bits == 2 {
            serialport::StopBits::Two
        } else {
            serialport::StopBits::One
        };
        let port = serialport::new(path, cfg.baud_rate)
            .data_bits(data_bits)
            .parity(parity)
            .stop_bits(stop_bits)
            .timeout(Duration::from_millis(50))
            .open()
            .map_err(io::Error::other)?;
        Ok(SerialLineIo {
            port,
            pending: Vec::new(),
        })
    }
}

impl LineIo for SerialLineIo {
    fn write_line(&mut self, line: &str, ending: &str) -> io::Result<()> {
        tracing::trace!(target: "roast_driver::wire", "TX {line:?}");
        self.port.write_all(line.as_bytes())?;
        self.port.write_all(ending.as_bytes())?;
        self.port.flush()
    }

    fn read_line(&mut self, timeout: Duration) -> io::Result<Option<String>> {
        let deadline = Instant::now() + timeout;
        let mut buf = [0u8; 256];
        loop {
            if let Some(line) = take_line(&mut self.pending) {
                tracing::trace!(target: "roast_driver::wire", "RX {line:?}");
                return Ok(Some(line));
            }
            if Instant::now() >= deadline {
                trace_incomplete(&self.pending);
                return Ok(None);
            }
            match self.port.read(&mut buf) {
                Ok(0) => {
                    return Err(io::Error::new(
                        io::ErrorKind::BrokenPipe,
                        "the port was closed",
                    ));
                }
                Ok(n) => self.pending.extend_from_slice(&buf[..n]),
                Err(e) if e.kind() == io::ErrorKind::TimedOut => {}
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                Err(e) => return Err(e),
            }
        }
    }

    fn clear_input(&mut self) -> io::Result<()> {
        self.pending.clear();
        // Some Bluetooth serial drivers do not support flushing; stale lines
        // are then skipped by the reply parser instead.
        if self.port.clear(serialport::ClearBuffer::Input).is_err() {
            drain_available(&mut *self.port);
        }
        Ok(())
    }
}

/// A fake transport that answers commands from a script, and records every
/// line written. For tests only; it never touches hardware.
#[derive(Debug, Default)]
pub struct ScriptedIo {
    /// Every line written, in order.
    pub written: Vec<String>,
    /// Replies queued for the next reads.
    pub queued: VecDeque<String>,
    /// Replies per command prefix: when a written line starts with the key,
    /// the value is queued.
    pub replies: Vec<(String, String)>,
}

impl ScriptedIo {
    /// A script that answers `command` (prefix) with `reply`.
    #[must_use]
    pub fn reply(mut self, command: &str, reply: &str) -> Self {
        self.replies.push((command.to_owned(), reply.to_owned()));
        self
    }
}

impl LineIo for ScriptedIo {
    fn write_line(&mut self, line: &str, _ending: &str) -> io::Result<()> {
        self.written.push(line.to_owned());
        if let Some((_, r)) = self
            .replies
            .iter()
            .find(|(c, _)| line.starts_with(c.as_str()))
        {
            self.queued.push_back(r.clone());
        }
        Ok(())
    }

    fn read_line(&mut self, _timeout: Duration) -> io::Result<Option<String>> {
        Ok(self.queued.pop_front())
    }

    fn clear_input(&mut self) -> io::Result<()> {
        self.queued.clear();
        Ok(())
    }
}

/// Reads and drops whatever is immediately available.
fn drain_available(port: &mut dyn serialport::SerialPort) {
    let mut buf = [0u8; 256];
    while port.bytes_to_read().unwrap_or(0) > 0 {
        if port.read(&mut buf).is_err() {
            break;
        }
    }
}
