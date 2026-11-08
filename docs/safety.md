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
| Roaster link lost / stale data | Alarm; reconnect attempts; AI control suspended |
| Emergency stop (button, global shortcut, MCP tool) | Safe state, latched: AI and autopilot locked out until a human resets |
| App crashes | Daemon keeps running with all rules; app reconnects |
| Daemon crashes | App raises an alarm; machine behaves as tested below |

The emergency stop path does not depend on the AI, the MCP connection or the web
view: the app's native side calls the daemon directly, and the daemon's safe
state is a single driver call.

## Presence confirmation

While the AI or the autopilot is in control, the app asks for a short
confirmation at a regular interval (default every 3 minutes; configurable,
cannot be disabled) — one key or one click. It is also required before every
critical step. The AI cannot confirm presence; only the app's UI can.

## Limits (defaults, per machine profile)

| | Human | AI / autopilot |
|---|---|---|
| Burner step | any | ≤ 5 percentage points |
| Burner rate | any | ≤ 1 change per 5 s |
| Burner range | profile range | profile range, optionally narrowed per roast |
| Air step | any | ≤ 10 points |

## What the hardware does when the link drops

This depends on the machine and must be known before AI control is used on it.
Each machine profile records the tested behaviour.

**Test protocol** (empty drum, low power, person present, extinguisher at hand):

1. Connect, start recording, set burner 20 %, air 50 %, drum on.
2. Break the link (switch off Bluetooth on the computer, or unplug).
3. Observe for 60 s: does the burner stay on, does the machine shut down, does
   the fan keep running, does the display show a warning?
4. Restore the link: does the machine accept commands again without a restart?
5. Repeat with the software killed instead of the link broken.

| Machine | Link lost | Software killed | Tested |
|---|---|---|---|
| Skywalker Cubean+ (BT) | not yet tested | not yet tested | — |

## Verifying the safety layer

- Unit and property tests for arbitration, limits, rate limits, lease and
  presence (for any sequence of commands, no AI command ever exceeds a limit;
  a human command is never blocked except by an emergency-stop latch, which
  only blocks AI).
- Fault injection in the simulator: link loss, stale data, overheat, AI
  disconnect, missed presence.
- No test, script or agent sends commands to a real roaster without a person
  explicitly confirming it on the spot.
