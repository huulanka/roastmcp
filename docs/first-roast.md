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

```sh
./target/release/roastmcp daemon --serial /dev/cu.AKROASTER
```

Open the printed link. Expected: the connection badge turns green within a few
seconds, and BT shows roughly room temperature. If it stays "connecting", the
port is busy or not paired.

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

1. Switch off Bluetooth on the computer (or move out of range) for 30 s.
   Watch the machine: does the heater stay on, switch off, or does the
   machine show an error? The app shows "link lost" and keeps trying to
   reconnect.
2. Switch Bluetooth back on. Expected: RoastMCP reconnects by itself.
3. Repeat with the daemon stopped by Ctrl-C: RoastMCP sends the safe state
   (burner 0, air 100) before it exits. Then kill it hard
   (`kill -9 <pid from daemon.json>`) and watch the machine again.

Write what you saw into `machines/skywalker-cubean-plus.toml` under
`[link_loss]` and set `tested = true` only when all three cases are known.
Until then, use AI control only while standing at the machine.

## 3. A supervised roast

1. Preheat as usual (for example burner 70 %, air 30 %) until the BT you
   normally charge at.
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
