//! Platforms without Bluetooth direct: their serial ports are used instead.

use std::io;
use std::time::Duration;

pub fn run_with_main_loop<T: Send + 'static>(
    work: impl FnOnce() -> T + Send + 'static,
) -> io::Result<T> {
    Ok(work())
}

pub fn list_serial_devices() -> Vec<String> {
    Vec::new()
}

pub struct Link;

fn unsupported() -> io::Error {
    io::Error::new(
        io::ErrorKind::Unsupported,
        "Bluetooth direct is only available on macOS; use the serial port of the device",
    )
}

impl Link {
    pub fn open(_target: &str) -> io::Result<Self> {
        Err(unsupported())
    }

    #[allow(clippy::unused_self)]
    pub fn write(&mut self, _bytes: &[u8]) -> io::Result<()> {
        Err(unsupported())
    }

    #[allow(clippy::unused_self)]
    pub fn recv(&mut self, _timeout: Duration) -> io::Result<Option<Vec<u8>>> {
        Err(unsupported())
    }

    #[allow(clippy::unused_self)]
    pub fn discard_pending(&mut self) -> io::Result<()> {
        Err(unsupported())
    }
}
