# Safety

A roaster is a heat source that can start a fire. Coffee chaff burns; beans that
stop moving in a hot drum can ignite. RoastMCP's safety design starts from that.

## Non-negotiables

1. **A person supervises every roast and can always take over.** Anything done
   by hand wins over the AI and the autopilot, immediately.
2. **Emergency stop** at any time: burner off, air up, drum keeps turning.
3. **Burner changes by the AI in small steps only**, within a range and at a
   limited rate.
4. **No operation without supervision.** If the AI disconnects, the roaster must
   not be left in an unsafe state.
5. **Everything is logged** and visible as events on the roast.

## The safe state

Defined per machine profile; for a drum roaster it is:

- burner **0 %**
- air **high** (machine profile, typically 100 %)
- drum **running** (never stopped while beans may be hot)
- cooling as the machine profile says

Switching everything off is **not** a safe state: with hot beans and no airflow
or drum movement the beans can ignite.

## Layers

| Situation | Reaction |
|---|---|
| Human command while AI/autopilot is in control | Human command applied; AI control suspended until a human resumes it |
| AI command outside range / step / rate | Clamped to the limit (reported to the AI) or rejected |
| AI lease expires (no AI activity for the lease time, default 60 s) | Autopilot stops, actuators hold, loud alert: "AI disconnected — you have control" |
| Presence confirmation missed while AI/autopilot is in control (interval default 3 min + 30 s grace) | Safe state, alarm |
| Critical step requested by AI/autopilot (start, DROP, step larger than the normal limit) | Waits for human confirmation |
| Bean temperature above the machine's limit, or implausible rise | Burner cut by the safety layer, alarm |
| Roaster link lost / stale data | Alarm with the machine's tested advice; reconnect attempts for as long as the connection is wanted; AI control suspended |
| Roaster connects or reconnects | Safe state set at once (a machine may start at its own defaults: the Cubean+ heats at 100 %); the person sets the heat again |
| Emergency stop (button, Escape in the app, global shortcut ⌘⇧⎋ / Ctrl+Alt+Esc, MCP tool) | Safe state, latched: AI and autopilot locked out until a human resets |
| App crashes | Daemon keeps running with all rules; app reconnects |
| Daemon stopped (Ctrl-C, SIGTERM) | Safe state is sent first if the burner is on; the running recording is saved |
| Daemon crashes | App raises an alarm; machine behaves as tested below; the recording is recovered from its autosave (every 20 s) on the next start |

The emergency stop path does not depend on the AI, the MCP connection or the web
view: the app's native side calls the daemon directly, and the daemon's safe
state is a single driver call.

## Presence confirmation

While the AI or the autopilot is in control, the app asks for a short
confirmation at a regular interval (default every 3 minutes; configurable,
cannot be disabled) — one key or one click. It is also required before every
critical step. The AI cannot confirm presence; only the app's UI can.

## Preheat

Preheating to a target temperature (started by a person in the app) is an
autopilot: it holds the lease as `preheat`, its burner commands pass the safety
layer under the AI rules (bounded raises, cuts free, never above the profile's
`max_bt_c - 10`), and presence must be confirmed while it runs. Starting it
counts as a presence confirmation and allows automation again. It ends at
CHARGE, on any manual command, an emergency stop, a lost link or a missed
presence confirmation; the burner then holds its value and the person has
control. The controller is a PI controller on the bean probe with anti-windup;
its gains are a first guess and need checking on each machine.

## Limits (defaults, per machine profile)

| | Human | AI / autopilot |
|---|---|---|
| Burner down (toward safe) | any | any size, any time |
| Burner up | any | ≤ 10 percentage points per command, ≤ 1 raise per 10 s |
| Burner range | profile range | profile range, optionally narrowed per roast |
| Air up (toward safe) | any | any size, any time |
| Air down | any | ≤ 20 points per command, ≤ 1 per 5 s |

Limits only bound moves away from the safe state. Experienced roasters cut
heat by 15-50 % at once before and during first crack; a symmetric step limit
would make the AI too slow exactly where timing matters, without making
anything safer. The bean-temperature limit, presence confirmation and the
lease still apply to every AI command. Background:
[research/heat-control-practice.md](research/heat-control-practice.md).

## What the hardware does when the link drops

This depends on the machine and must be known before AI control is used on it.
Each machine profile records the tested behaviour.

**Test protocol** (empty drum, low power, person present, extinguisher at hand):

1. Connect, start recording, set burner 20 %, air 50 %, drum on.
2. Break the link (unplug, or disconnect only the roaster in the Bluetooth
   settings: switching Bluetooth off also drops other devices, such as
   hearing aids).
3. Observe for 60 s: does the burner stay on, does the machine shut down, does
   the fan keep running, does the display show a warning?
4. Restore the link: does the machine accept commands again without a restart?
5. Repeat with the software killed instead of the link broken.

| Machine | Link lost | Software killed | Tested |
|---|---|---|---|
| Skywalker Cubean+ (BT) | Standby within seconds: heater, fan **and drum** stop. Does not come back by itself on macOS: connect AKROASTER again in the Bluetooth settings. On connect it runs at burner 100 %, air 20 % until commanded | Same standby as soon as the port closes (also `kill -9`) | 2026-11-08 |

Because the Cubean+ stops the drum and fan, a link lost mid-roast leaves hot
beans standing in a heated drum: drop them, then reconnect. The banner says so.

## Verifying the safety layer

- Unit and property tests for arbitration, limits, rate limits, lease and
  presence (for any sequence of commands, no AI command ever exceeds a limit;
  a human command is never blocked except by an emergency-stop latch, which
  only blocks AI).
- Fault injection in the simulator: link loss, stale data, overheat, AI
  disconnect, missed presence.
- No test, script or agent sends commands to a real roaster without a person
  explicitly confirming it on the spot.
