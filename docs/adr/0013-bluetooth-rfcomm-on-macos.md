# ADR 0013: Talk to Bluetooth roasters over RFCOMM directly on macOS

## Status
Accepted (2026-11-08). Bluetooth direct is a choice in the connect dialog. It
becomes the default once it has passed the hardware checks below on the
Cubean+.

## Context
The Cubean+ is a Bluetooth Classic (SPP) device. macOS exposes it as the serial
port `/dev/cu.AKROASTER`, and RoastMCP has used that port so far. Tests on the
machine on 2026-11-08 showed:

- Only the first port open after the person connects AKROASTER in the Bluetooth
  settings gets answers. Every later open stays silent: bytes go out, nothing
  comes back.
- After a lost link (Bluetooth dropped, daemon killed) the roaster goes to
  standby (heater, fan and drum stop) and does not come back through the port.
  The person has to connect AKROASTER again in the settings, mid-roast with hot
  beans in the drum.
- Opening the SPP RFCOMM channel directly through IOBluetooth (SDP record for
  UUID 0x1101, channel 1) answered `READ` every time the roaster was reachable,
  also while the serial port stayed silent. Once, a port open about 25 s after
  such a direct open worked again. That was timing-dependent and is no basis
  for a fix.
- A probe with `objc2-io-bluetooth` showed that IOBluetooth delivers its
  callbacks (connection results, received RFCOMM data) only on the main
  thread's run loop. A background thread's run loop gets no sources and never
  sees a callback.

## Options
1. **Keep the serial port.** The person reconnects by hand after every lost
   link. This fails exactly when it matters most.
2. **"Kick" before opening the port.** Open and close RFCOMM directly, wait,
   then open the port. Flaky and slow.
3. **Native RFCOMM transport in the daemon** with `objc2-io-bluetooth`, which
   is already in the dependency tree's family: Tauri uses `objc2`. IOBluetooth
   calls and callbacks run on the daemon's main thread; the async runtime moves
   to a worker thread.
4. **A Swift helper process** that bridges RFCOMM to stdin/stdout. This needs
   a second toolchain, signing and process supervision, and the threading is no
   different.

## Decision
Option 3.

- On macOS, `roastmcp` runs its command on a worker thread and keeps the main
  thread as a small Bluetooth executor. The executor runs queued jobs and pumps
  the main run loop in short slices. Without Bluetooth jobs it sleeps.
- `roast-driver` gains an `RfcommLineIo` (macOS only) behind the existing
  `LineIo` trait, so the TC4 driver, the safety layer and the reconnect logic
  stay as they are. The device and channel objects live on the main thread.
  Received bytes reach the driver thread through a channel, and writes are jobs
  on the main thread. A channel the roaster closes reads as a broken link.
- Ports are strings, as before. Paired devices that offer SPP are listed as
  `bluetooth:<name>` (for example `bluetooth:AKROASTER`) next to the serial
  ports. Choosing one in the connect dialog is the setting. The app remembers
  the last choice. `--serial bluetooth:AKROASTER` works on the command line.
- Closing the transport closes the RFCOMM channel and then the baseband
  connection, so the next open starts fresh.

## Consequences
- After a lost link the driver's reconnect loop opens a new RFCOMM channel by
  itself. The person no longer has to go to the Bluetooth settings when the
  roaster is still reachable.
- macOS only. Windows and Linux keep the serial port (COM port, rfcomm device),
  which has not shown this problem so far.
- Running a command off the main thread changes nothing for the daemon, which
  already used a multi-threaded runtime.
- Unsafe FFI is confined to one module, and the bindings are generated
  (`objc2`).

## Hardware checks before Bluetooth direct becomes the default
With an empty drum and the person at the machine:
1. Connect via `bluetooth:AKROASTER` without touching the Bluetooth settings.
   Expect readings, then the safe state on connect.
2. Burner 20 %, then drop the link by disconnecting only AKROASTER for 30 s.
   Expect the "link lost" banner, then readings again without any action by
   the person, then the safe state again.
3. Stop the daemon with Ctrl-C and start it again. It must connect without
   any step in the Bluetooth settings.
