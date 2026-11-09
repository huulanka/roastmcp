# Safety

A roaster is a heat source that can start a fire. Coffee chaff burns; beans that
stop moving in a hot drum can ignite. RoastMCP's safety design starts from that.

## Non-negotiables

1. **A person supervises every roast and can always take over.** Anything done
   by hand wins over the AI and the autopilot, immediately. A command on the
   heat loop (burner, air, drum) also ends AI control; the smoke filter and the
   bean cooling can be handled without ending it (ADR 0019).
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
| Human command on burner, air or drum while AI/autopilot is in control | Human command applied; AI control suspended until a human resumes it |
| Human command on the filter or the bean cooling while an AI is in control | Applied; the AI stays in control (the machine profile's `limits.takeover_actuators` names the actuators that end it; it must include the burner) |
| AI command outside range / step / rate | Clamped to the limit (reported to the AI) or rejected |
| AI lease expires (no AI activity for the lease time, default 60 s; only the AI's own commands and status calls renew it, not the recipe runner acting under it) | Recipe runner stops, actuators hold, loud alert: "AI disconnected — you have control"; before CHARGE the preheat takes its own lease back and holds on |
| Presence confirmation missed while AI/autopilot is in control (interval default 3 min, 10 min for the preheat before CHARGE, + 30 s grace) | Safe state, alarm |
| Critical step requested by AI/autopilot (start, DROP, step larger than the normal limit) | Waits for human confirmation |
| DROP (detected or marked) | Burner 0 and bean cooling on, as safety-layer commands; an AI's lease ends ("handed control back"), which is not a takeover and suspends nothing |
| The bean temperature will reach the planned DROP (the plan's drop temperature, else the reference's) within 30 s | Bean cooling on, once per roast, so the tray turns when the beans arrive; switched off by the person, it stays off until DROP |
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
confirmation at a regular interval (default every 3 minutes; cannot be
disabled) — one key or one click. It is also required before every critical
step. The AI cannot confirm presence; only the app's UI can.

- **Warning (ADR 0018):** whenever an AI or the preheat holds the lease, a
  gong sounds 60 s and 20 s before the confirmation is due (twice at 20 s),
  with a notice, and the top bar offers "I'm at the roaster" from two minutes
  before. The safe state comes only after the due time plus 30 s grace.
- **Space bar:** while the app has focus, Space confirms presence whenever
  someone holds or asks for control or a confirmation is due soon; not in a
  text field. The press is taken whole, so it never also presses a focused
  button.
- **Preheat before CHARGE:** while the preheat holds the lease, the interval
  is the machine profile's `limits.preheat_presence_interval_s` (180–900 s,
  600 s if not set). The empty drum is bounded there by the preheat's own
  limits. Anyone else, an AI that takes over included, keeps the normal
  interval and needs a confirmation within it.
- **CHARGE marked in the app (ADR 0019):** the person who presses CHARGE is
  demonstrably at the roaster, so it counts as a presence confirmation,
  whether or not an AI is involved. A detected CHARGE, or one the AI marks,
  does not.

When an AI asks for control without a fresh confirmation, the request is
refused and the app shows "I'm at the roaster" in the top bar and sounds the
gong at once, for two minutes; the AI then asks again. An AI may ask while
the preheat holds: after the confirmation it holds the lease and the preheat
keeps holding the target under that lease until CHARGE.

## Preheat

Preheating to a target temperature (started by a person in the app) is an
autopilot: it holds the lease as `preheat`, its burner commands pass the safety
layer under the AI rules (bounded raises, cuts free, never above the profile's
`max_bt_c - 10`), and presence must be confirmed while it runs (every 10 min,
see above). Starting it counts as a presence confirmation and allows
automation again. It runs at the air the roast starts with: the app fills in
target and air from the plan (its BT and air at CHARGE), and starting sets
that air first as the person's own command (ADR 0018). The MCP has no preheat
tool, because starting it confirms presence. It ends at CHARGE or on a
manual command on burner, air or drum, and then the burner holds its value
and the person has control. An AI that takes over before CHARGE does not end
it: the preheat holds under the AI's lease (and its 3 min presence interval)
until CHARGE; if the AI releases control or its lease runs out first, the
preheat takes its own lease back. An emergency stop, a lost link or a missed
presence confirmation end it with the safe state. The controller
([ADR 0015](adr/0015-preheat-step-wait-learn.md), [ADR 0018](adr/0018-preheat-air-presence-calm-hold.md))
heats fully, steps down once to the holding power before the target, then
corrects rarely: it waits 45 s after each change, corrects only when the
measured and the predicted temperature are both more than 2 °C off on the
same side, by at most 10 points, and turns the burner off at once above
target + 10 °C. What it learns about a machine is kept per air setting in
`control.json` in the data folder.

An AI cannot mark CHARGE without a running recording (it would start one
without beans, weight or plan); the app's CHARGE button still starts one.

## Recipe runner

While an AI holds the lease and the plan is a profile, the daemon sets the
recipe's steps itself ([ADR 0019](adr/0019-handover-before-charge-and-recipe-runner.md)):
each at its trigger (a roast time, a bean temperature after the turning point,
or a time after dry end or first crack, marked or detected), with source
`Autopilot`, through the safety layer and under the AI's lease and presence
rules, logged with a reason ("recipe 1:30: burner 85 %"). Raises are paced by
the limits below; a step nobody set within 30 s of its time passes. The one
exception: in the first 10 s after CHARGE the autopilot may set the CHARGE
step at once, because the cold beans take the heat (range, overheat, lease and
presence still apply; an AI's own commands keep their limits). Without a
lease there is no runner: the steps are cues for the person. The AI changes
the remaining steps for this roast only (`adjust_recipe`), which is noted with
the roast; its own `set_*` command replaces the step due on that actuator, not
later ones.

## Limits (defaults, per machine profile)

| | Human | AI / autopilot |
|---|---|---|
| Burner down (toward safe) | any | any size, any time |
| Burner up | any | ≤ 10 percentage points per command, ≤ 1 raise per 10 s |
| Burner range | profile range | profile range, optionally narrowed per roast |
| Air up (toward safe) | any | any size, any time |
| Air down | any | ≤ 20 points per command, ≤ 1 per 5 s |
| CHARGE step (recipe runner, first 10 s after CHARGE) | | at once, within the range |

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
