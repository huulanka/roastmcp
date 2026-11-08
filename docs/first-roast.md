# First roast on real hardware

This runbook takes a machine from "never driven by RoastMCP" to a first
supervised roast. It was written for the Skywalker Cubean+ and applies to
other TC4 machines with the obvious changes. **A person stays at the roaster
for every step**, and the machine's own controls and power switch stay within
reach.

## 0. Before you start

- Build the UI and the binary (see the README) and close Artisan or any other
  program that might hold the serial port.
- Pair the roaster with the computer (Bluetooth). On macOS it appears as
  `/dev/cu.AKROASTER`; `roastmcp ports` lists the ports.
- Know where the machine's power switch is. If anything behaves unexpectedly,
  switching the machine off is always allowed.

## 1. Connect with an empty drum

Start the desktop app (`RoastMCP.app`, see the README) and connect to
`/dev/cu.AKROASTER` from the app. Without the app:

```sh
./target/release/roastmcp daemon --serial /dev/cu.AKROASTER
```

and open the printed link. Expected: the connection badge turns green within a few
seconds, and BT shows roughly room temperature. If it stays "connecting", the
port is busy or not paired.

If the roaster does not answer, RoastMCP stays at "connecting", and after three
silent attempts it stops with a message instead of retrying forever. Where to
look:

- The log: menu *Help → Show Log* (or `~/Library/Application Support/com.andreasbauer.roastmcp/logs/daemon.log`).
- Every line sent and received: start the daemon with
  `RUST_LOG=info,roast_driver::wire=trace` (the app's daemon logs to the same file).
- Who holds the port: `lsof /dev/cu.AKROASTER` (Artisan, a second daemon).
- Switch the roaster off and on, wait for a steady Bluetooth light, connect again.

What the first hardware test (2026-11-08, operator at the machine) showed:

- **After a fresh Bluetooth connect, only the first open of the port is answered.**
  A second open (another program, a restart of the daemon) sees the port but gets
  no reply until Bluetooth is disconnected and connected again in the system
  settings. In the wire trace this looks like `TX` lines without any `RX`.
  Start RoastMCP right after connecting, and reconnect Bluetooth before every
  restart of the daemon.
- **The heater runs at 100 % as soon as the machine is switched on** (Artisan
  shows the same). Set the burner to 0 before anything else; the app shows a
  banner with a one-click "Burner to 0" while the burner runs with no roast or
  preheat.
- The machine never acknowledges `CHAN`, `UNITS` or `FILT`. That is normal; the
  profile no longer waits for a reply. Readings (`READ`) and all actuator
  commands worked: burner, air, drum, cooling, emergency stop and its latch.
- The only temperature probe is BT; ET reads a constant and there is no probe in
  the cooling tray. After the drop BT therefore says nothing about the beans, so
  the drop ends and saves the recording and no "cooling end" is measured.

Check, still with an empty, cold drum:

| Step | Expected |
|---|---|
| Air to 50 %, then 100 %, then 30 % | Fan audibly changes; the air value in the UI follows the machine's report |
| Drum 80 % then 100 % | Drum speed changes |
| Burner to 20 % for 30 s, then 0 % | BT starts to rise; the reported burner value follows (note any odd values like 21.7) |
| Cooling on, then off | Cooling tray starts and stops (if it does not stop, the machine needs `CLDN,OFF` instead of `CLDN,OF`: note it) |
| Emergency stop (button or Escape) | Burner 0, air 100, drum 100; red banner; burner cannot be raised until you reset |

## 2. Link-loss test (empty drum, low power)

This answers what the machine does when the computer goes away
(`docs/safety.md`). With burner 20 % and air 40 %:

1. Disconnect only AKROASTER in the Bluetooth settings (or move out of
   range) for 30 s. Switching Bluetooth off also drops other devices, such
   as hearing aids.
   Watch the machine: does the heater stay on, switch off, or does the
   machine show an error? The app shows "link lost" and keeps trying to
   reconnect.
2. Connect AKROASTER again. RoastMCP keeps retrying and picks the link up.
3. Repeat with the daemon stopped by Ctrl-C: RoastMCP sends the safe state
   (burner 0, air 100) before it exits. Then kill it hard
   (`kill -9 <pid from daemon.json>`) and watch the machine again.

Write what you saw into `machines/skywalker-cubean-plus.toml` under
`[link_loss]` and set `tested = true` only when all three cases are known.
Until then, use AI control only while standing at the machine.

**Result for the Cubean+ (2026-11-08):** in every case the machine goes to
standby within seconds - heater, fan and drum stop, the display shows only the
temperature. It does not reconnect by itself on macOS; connect AKROASTER again
in the Bluetooth settings. On connect it starts at burner 100 %, air 20 %;
RoastMCP now sets the safe state at once (burner 0, air 100, drum 100), so set
the heat again after a reconnect.

**AI control over MCP (verified 2026-11-08):** when an AI calls
`request_control`, the top bar shows **I'm at the roaster**; only after you
press it does the AI get control. On the machine, a burner request of 25 %
from 0 became 10 %, a second raise right after was refused, air 100 → 50
became 80, a burner change by you locked the AI out, and its emergency stop
latched the safe state.

## 3. A supervised roast

1. Preheat as usual (for example burner 70 %, air 30 %) until the BT you
   normally charge at — or use **Aufheizen** with a target temperature: it
   raises the burner in bounded steps, holds the target, asks for presence
   every 3 minutes and ends at CHARGE or as soon as you change anything. Watch
   the first run closely; its control gains are untested on the machine.
   Optionally pick an earlier roast as the reference (archive → "Als
   Referenz"; your imported Artisan roasts are there).
2. Press **Start recording** (title, green weight), then **CHARGE** as the
   beans go in. Turning point and DRY END are detected automatically; correct
   them by clicking the button again.
3. Roast by hand with the sliders or the keys (Q/A burner, W/S air). Mark
   **FC start** when you hear first crack, **DROP** when the beans leave the
   drum.
4. **Stop recording** saves the roast as `.roast.json` and `.alog` (open it in
   Artisan or upload it wherever you use `.alog`). Enter the roasted weight in
   the archive.

## 4. Letting Claude watch, then help

Register the MCP server (README) and start with read-only questions: "How is
my roast going?", "What does the rate of rise tell you?". Claude can mark
events and add notes.

For control: confirm presence in the app ("I'm at the roaster"), then ask
Claude to take over. Cutting heat and adding air are never limited; raising
the burner is limited to 10 points per 10 s and lowering the air to 20 points
per 5 s. You confirm presence in the app
every 3 minutes (plus 30 s grace), otherwise the roaster goes to the safe
state. Any change you make in the app takes control back immediately. The action log is in
`logs/actions.jsonl` in the data directory.
