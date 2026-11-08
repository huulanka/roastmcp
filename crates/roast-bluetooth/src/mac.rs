//! IOBluetooth on macOS. Every IOBluetooth object lives on the main thread:
//! IOBluetooth delivers callbacks only on the main run loop (verified with a
//! probe on 2026-11-08; a background thread's run loop never got a source).
//! Other threads hand work to the main thread as jobs and receive data
//! through channels.

use std::cell::RefCell;
use std::collections::HashMap;
use std::ffi::c_void;
use std::io;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender, TryRecvError};
use std::time::Duration;

use objc2::rc::Retained;
use objc2::runtime::{NSObject, NSObjectProtocol, ProtocolObject};
use objc2::{AllocAnyThread, DefinedClass, MainThreadMarker, define_class, msg_send};
use objc2_core_foundation::{CFRunLoop, CFRunLoopRunResult, kCFRunLoopDefaultMode};
use objc2_foundation::NSString;
use objc2_io_bluetooth::{
    IOBluetoothDevice, IOBluetoothRFCOMMChannel, IOBluetoothRFCOMMChannelDelegate,
    IOBluetoothSDPUUID,
};

/// The serial port profile's service class.
const SPP_UUID16: u16 = 0x1101;
/// Channel used when the device's service record is not cached.
const DEFAULT_CHANNEL: u8 = 1;
/// Opening waits for the device to answer the page (a few seconds at most).
const OPEN_WAIT: Duration = Duration::from_secs(30);
/// Writes and listings are quick.
const SHORT_WAIT: Duration = Duration::from_secs(5);

type Job = Box<dyn FnOnce() + Send>;

/// Jobs for the main thread, set while [`run_with_main_loop`] runs.
static EXECUTOR: OnceLock<Sender<Job>> = OnceLock::new();

pub fn run_with_main_loop<T: Send + 'static>(
    work: impl FnOnce() -> T + Send + 'static,
) -> io::Result<T> {
    if MainThreadMarker::new().is_none() {
        // Not the main thread: no executor; Bluetooth direct reports that.
        return Ok(work());
    }
    let (tx, rx) = mpsc::channel::<Job>();
    if EXECUTOR.set(tx).is_err() {
        return Ok(work());
    }
    // The daemon's runtime used to run here; give it a main-thread-sized stack.
    let worker = std::thread::Builder::new()
        .name("main-work".into())
        .stack_size(8 << 20)
        .spawn(work)?;
    while !worker.is_finished() {
        while let Ok(job) = rx.try_recv() {
            job();
        }
        // Deliver IOBluetooth callbacks. Without Bluetooth sources the run
        // loop returns at once; then wait for jobs instead of spinning.
        // SAFETY: reading an immutable CoreFoundation constant.
        let mode = unsafe { kCFRunLoopDefaultMode };
        if CFRunLoop::run_in_mode(mode, 0.02, true) == CFRunLoopRunResult::Finished {
            if let Ok(job) = rx.recv_timeout(Duration::from_millis(20)) {
                job();
            }
        }
    }
    // Closing links on the way out.
    while let Ok(job) = rx.try_recv() {
        job();
    }
    match worker.join() {
        Ok(v) => Ok(v),
        Err(panic) => std::panic::resume_unwind(panic),
    }
}

/// Runs `f` on the main thread and waits for its result.
fn on_main<R: Send + 'static>(
    f: impl FnOnce() -> R + Send + 'static,
    wait: Duration,
) -> io::Result<R> {
    if MainThreadMarker::new().is_some() {
        return Ok(f());
    }
    let exec = EXECUTOR.get().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::Unsupported,
            "Bluetooth direct needs the daemon's main loop (run `roastmcp daemon`)",
        )
    })?;
    let (tx, rx) = mpsc::sync_channel(1);
    exec.send(Box::new(move || {
        let _ = tx.send(f());
    }))
    .map_err(|_| io::Error::other("the Bluetooth loop has stopped"))?;
    rx.recv_timeout(wait)
        .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "Bluetooth did not finish in time"))
}

/// Runs `f` on the main thread without waiting (closing on drop).
fn on_main_later(f: impl FnOnce() + Send + 'static) {
    if MainThreadMarker::new().is_some() {
        f();
    } else if let Some(exec) = EXECUTOR.get() {
        let _ = exec.send(Box::new(f));
    }
}

enum Event {
    Data(Vec<u8>),
    Closed,
}

struct DelegateIvars {
    events: Sender<Event>,
}

define_class!(
    // SAFETY: NSObject has no subclassing requirements and the class has no
    // Drop implementation.
    #[unsafe(super(NSObject))]
    #[name = "RoastMcpRfcommDelegate"]
    #[ivars = DelegateIvars]
    struct Delegate;

    unsafe impl NSObjectProtocol for Delegate {}

    unsafe impl IOBluetoothRFCOMMChannelDelegate for Delegate {
        #[unsafe(method(rfcommChannelData:data:length:))]
        fn data(&self, _channel: Option<&IOBluetoothRFCOMMChannel>, ptr: *mut c_void, len: usize) {
            if ptr.is_null() || len == 0 {
                return;
            }
            // SAFETY: IOBluetooth passes `len` readable bytes, valid during
            // this call; they are copied before returning.
            let bytes = unsafe { std::slice::from_raw_parts(ptr.cast::<u8>(), len) }.to_vec();
            let _ = self.ivars().events.send(Event::Data(bytes));
        }

        #[unsafe(method(rfcommChannelClosed:))]
        fn closed(&self, _channel: Option<&IOBluetoothRFCOMMChannel>) {
            let _ = self.ivars().events.send(Event::Closed);
        }
    }
);

impl Delegate {
    fn new(events: Sender<Event>) -> Retained<Self> {
        let this = Self::alloc().set_ivars(DelegateIvars { events });
        // SAFETY: NSObject's designated initialiser.
        unsafe { msg_send![super(this), init] }
    }
}

/// The objects of one open link; main thread only.
struct Open {
    device: Retained<IOBluetoothDevice>,
    channel: Retained<IOBluetoothRFCOMMChannel>,
    _delegate: Retained<Delegate>,
}

thread_local! {
    static OPEN: RefCell<HashMap<u64, Open>> = RefCell::new(HashMap::new());
}

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

fn looks_like_address(s: &str) -> bool {
    let parts: Vec<&str> = s.split([':', '-']).collect();
    parts.len() == 6
        && parts
            .iter()
            .all(|p| p.len() == 2 && p.chars().all(|c| c.is_ascii_hexdigit()))
}

fn device_name(device: &IOBluetoothDevice) -> Option<String> {
    // SAFETY: a getter on a live device object, on the main thread.
    unsafe { device.nameOrAddress() }.map(|n| n.to_string())
}

fn paired_devices() -> Vec<Retained<IOBluetoothDevice>> {
    // SAFETY: a class getter, on the main thread.
    let Some(list) = (unsafe { IOBluetoothDevice::pairedDevices() }) else {
        return Vec::new();
    };
    (0..list.count())
        .map(|i| list.objectAtIndex(i))
        .filter_map(|obj| obj.downcast::<IOBluetoothDevice>().ok())
        .collect()
}

/// The RFCOMM channel of the device's cached serial-port record, if any.
fn spp_channel(device: &IOBluetoothDevice) -> Option<u8> {
    // SAFETY: getters on live objects, on the main thread; `id` outlives the
    // call that writes it.
    unsafe {
        let uuid = IOBluetoothSDPUUID::uuid16(SPP_UUID16)?;
        let record = device.getServiceRecordForUUID(Some(&uuid))?;
        let mut id: u8 = 0;
        (record.getRFCOMMChannelID(&mut id) == 0).then_some(id)
    }
}

fn find_device(target: &str) -> io::Result<Retained<IOBluetoothDevice>> {
    if looks_like_address(target) {
        let address = NSString::from_str(target);
        // SAFETY: a class constructor, on the main thread.
        return unsafe { IOBluetoothDevice::deviceWithAddressString(Some(&address)) }.ok_or_else(
            || {
                io::Error::new(
                    io::ErrorKind::NotFound,
                    format!("no Bluetooth device {target}"),
                )
            },
        );
    }
    paired_devices()
        .into_iter()
        .find(|d| device_name(d).as_deref() == Some(target))
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                format!("{target} is not paired with this Mac; pair it in the Bluetooth settings"),
            )
        })
}

fn open_on_main(id: u64, target: &str, events: Sender<Event>) -> io::Result<()> {
    let device = find_device(target)?;
    let channel_id = spp_channel(&device).unwrap_or(DEFAULT_CHANNEL);
    let delegate = Delegate::new(events);
    let mut channel = None;
    let as_delegate = ProtocolObject::<dyn IOBluetoothRFCOMMChannelDelegate>::from_ref(&*delegate);
    // SAFETY: on the main thread; `channel` and the delegate outlive the call,
    // and the delegate is kept alive with the channel in `OPEN`.
    let status = unsafe {
        device.openRFCOMMChannelSync_withChannelID_delegate(
            Some(&mut channel),
            channel_id,
            Some(as_delegate.as_ref()),
        )
    };
    let Some(channel) = channel.filter(|_| status == 0) else {
        // SAFETY: on the main thread; drops a half-open baseband link.
        unsafe { device.closeConnection() };
        return Err(io::Error::new(
            io::ErrorKind::ConnectionRefused,
            format!(
                "{target} did not open its serial channel {channel_id} (IOReturn {status:#x}); \
                 is the roaster switched on and in range?"
            ),
        ));
    };
    tracing::info!("Bluetooth: opened {target}, RFCOMM channel {channel_id}");
    OPEN.with(|o| {
        o.borrow_mut().insert(
            id,
            Open {
                device,
                channel,
                _delegate: delegate,
            },
        );
    });
    Ok(())
}

fn write_on_main(id: u64, mut bytes: Vec<u8>) -> io::Result<()> {
    OPEN.with(|o| {
        let open = o.borrow();
        let link = open
            .get(&id)
            .ok_or_else(|| io::Error::new(io::ErrorKind::BrokenPipe, "Bluetooth link closed"))?;
        // SAFETY: a getter on a live channel, on the main thread.
        let mtu = usize::from(unsafe { link.channel.getMTU() }).max(1);
        for chunk in bytes.chunks_mut(mtu) {
            let len = u16::try_from(chunk.len()).unwrap_or(u16::MAX);
            // SAFETY: `chunk` is valid for `len` bytes during the synchronous
            // write, on the main thread.
            let status = unsafe {
                link.channel
                    .writeSync_length(chunk.as_mut_ptr().cast::<c_void>(), len)
            };
            if status != 0 {
                return Err(io::Error::new(
                    io::ErrorKind::BrokenPipe,
                    format!("Bluetooth write failed (IOReturn {status:#x})"),
                ));
            }
        }
        Ok(())
    })
}

fn close_on_main(id: u64) {
    if let Some(link) = OPEN.with(|o| o.borrow_mut().remove(&id)) {
        // SAFETY: on the main thread. Closing the baseband link too makes the
        // next open start from scratch, as a fresh connect in the settings.
        unsafe {
            link.channel.setDelegate(None);
            link.channel.closeChannel();
            link.device.closeConnection();
        }
        tracing::info!("Bluetooth: closed link {id}");
    }
}

pub fn list_serial_devices() -> Vec<String> {
    on_main(
        || {
            paired_devices()
                .iter()
                .filter(|d| spp_channel(d).is_some())
                .filter_map(|d| device_name(d))
                .collect::<Vec<_>>()
        },
        SHORT_WAIT,
    )
    .unwrap_or_default()
}

pub struct Link {
    id: u64,
    events: Receiver<Event>,
    closed: bool,
}

impl Link {
    pub fn open(target: &str) -> io::Result<Self> {
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = mpsc::channel();
        let target = target.to_owned();
        on_main(move || open_on_main(id, &target, tx), OPEN_WAIT)??;
        Ok(Link {
            id,
            events: rx,
            closed: false,
        })
    }

    fn broken() -> io::Error {
        io::Error::new(
            io::ErrorKind::BrokenPipe,
            "the roaster closed the Bluetooth link",
        )
    }

    pub fn write(&mut self, bytes: &[u8]) -> io::Result<()> {
        if self.closed {
            return Err(Self::broken());
        }
        let id = self.id;
        let bytes = bytes.to_vec();
        on_main(move || write_on_main(id, bytes), SHORT_WAIT)?
    }

    pub fn recv(&mut self, timeout: Duration) -> io::Result<Option<Vec<u8>>> {
        if self.closed {
            return Err(Self::broken());
        }
        match self.events.recv_timeout(timeout) {
            Ok(Event::Data(bytes)) => Ok(Some(bytes)),
            Ok(Event::Closed) | Err(RecvTimeoutError::Disconnected) => {
                self.closed = true;
                Err(Self::broken())
            }
            Err(RecvTimeoutError::Timeout) => Ok(None),
        }
    }

    pub fn discard_pending(&mut self) -> io::Result<()> {
        loop {
            match self.events.try_recv() {
                Ok(Event::Data(_)) => {}
                Ok(Event::Closed) | Err(TryRecvError::Disconnected) => {
                    self.closed = true;
                    return Err(Self::broken());
                }
                Err(TryRecvError::Empty) => return Ok(()),
            }
        }
    }
}

impl Drop for Link {
    fn drop(&mut self) {
        let id = self.id;
        on_main_later(move || close_on_main(id));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognises_addresses() {
        assert!(looks_like_address("bd-48-1c-65-2f-12"));
        assert!(looks_like_address("BD:48:1C:65:2F:12"));
        assert!(!looks_like_address("AKROASTER"));
        assert!(!looks_like_address("bd-48-1c-65-2f"));
    }

    #[test]
    fn without_the_main_loop_open_says_why() {
        // On the main thread (`--test-threads=1`) this would page a real
        // device: never touch hardware from a test.
        if MainThreadMarker::new().is_some() {
            return;
        }
        // Test threads are not the main thread and no executor runs.
        let err = Link::open("AKROASTER").err().map(|e| e.kind());
        assert_eq!(err, Some(io::ErrorKind::Unsupported));
    }
}
