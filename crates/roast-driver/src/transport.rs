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

/// The serial ports the operating system offers, by name.
#[must_use]
pub fn list_ports() -> Vec<String> {
    let mut ports: Vec<String> = serialport::available_ports()
        .unwrap_or_default()
        .into_iter()
        .map(|p| p.port_name)
        .collect();
    ports.sort();
    ports
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

    fn take_line(&mut self) -> Option<String> {
        let pos = self.pending.iter().position(|&b| b == b'\n')?;
        let line: Vec<u8> = self.pending.drain(..=pos).collect();
        Some(
            String::from_utf8_lossy(&line)
                .trim_end_matches(['\r', '\n'])
                .to_owned(),
        )
    }
}

impl LineIo for SerialLineIo {
    fn write_line(&mut self, line: &str, ending: &str) -> io::Result<()> {
        self.port.write_all(line.as_bytes())?;
        self.port.write_all(ending.as_bytes())?;
        self.port.flush()
    }

    fn read_line(&mut self, timeout: Duration) -> io::Result<Option<String>> {
        let deadline = Instant::now() + timeout;
        let mut buf = [0u8; 256];
        loop {
            if let Some(line) = self.take_line() {
                return Ok(Some(line));
            }
            if Instant::now() >= deadline {
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
        self.port
            .clear(serialport::ClearBuffer::Input)
            .map_err(io::Error::other)
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
