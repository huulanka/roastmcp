//! Bluetooth Classic RFCOMM links to roasters, without the operating system's
//! serial-port layer (ADR 0013).
//!
//! On macOS the serial port of a Bluetooth SPP device (`/dev/cu.AKROASTER`)
//! answers only the first open after the person connects the device in the
//! Bluetooth settings, while a direct RFCOMM channel answers every time. This
//! crate opens that channel through IOBluetooth. IOBluetooth delivers its
//! callbacks only on the main thread's run loop, so a program that uses it runs
//! its real work through [`run_with_main_loop`] and leaves the main thread to
//! this crate.
//!
//! Other platforms get stubs: their serial ports have not shown the problem.
//! This is the only crate in the workspace with `unsafe` code.

use std::io;
use std::time::Duration;

#[cfg(target_os = "macos")]
mod mac;
#[cfg(target_os = "macos")]
use mac as imp;

#[cfg(not(target_os = "macos"))]
mod other;
#[cfg(not(target_os = "macos"))]
use other as imp;

/// Runs `work` on a worker thread and, on macOS when called from the main
/// thread, serves Bluetooth on the main thread until `work` returns. Elsewhere
/// it just runs `work`.
///
/// # Errors
/// When the worker thread cannot be started.
pub fn run_with_main_loop<T: Send + 'static>(
    work: impl FnOnce() -> T + Send + 'static,
) -> io::Result<T> {
    imp::run_with_main_loop(work)
}

/// Names of paired devices that offer a serial port profile (SPP), for
/// example `AKROASTER`. Empty when Bluetooth is not available.
#[must_use]
pub fn list_serial_devices() -> Vec<String> {
    imp::list_serial_devices()
}

/// An open RFCOMM channel to one device.
pub struct RfcommLink(imp::Link);

impl RfcommLink {
    /// Opens the serial-port channel of `target`, a paired device's name (as
    /// listed by [`list_serial_devices`]) or its address (`bd-48-1c-…`).
    /// Waits for the device to answer the page, which can take a few seconds.
    ///
    /// # Errors
    /// When the device is not paired or does not answer, or Bluetooth direct is
    /// not available (not macOS, or not run through [`run_with_main_loop`]).
    pub fn open(target: &str) -> io::Result<Self> {
        imp::Link::open(target).map(RfcommLink)
    }

    /// Sends bytes.
    ///
    /// # Errors
    /// When the channel is closed or the write fails.
    pub fn write(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.0.write(bytes)
    }

    /// The next bytes received, or `None` after `timeout`.
    ///
    /// # Errors
    /// [`io::ErrorKind::BrokenPipe`] once the device closed the channel.
    pub fn recv(&mut self, timeout: Duration) -> io::Result<Option<Vec<u8>>> {
        self.0.recv(timeout)
    }

    /// Drops everything received but not read yet.
    ///
    /// # Errors
    /// [`io::ErrorKind::BrokenPipe`] once the device closed the channel.
    pub fn discard_pending(&mut self) -> io::Result<()> {
        self.0.discard_pending()
    }
}
