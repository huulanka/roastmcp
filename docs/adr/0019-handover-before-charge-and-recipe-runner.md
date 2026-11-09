# ADR 0019: The AI takes over before CHARGE, the daemon runs the recipe, and filter and cooling stay free

## Status
Accepted (2026-11-09), with the three open points as proposed below. Amends [ADR 0004](0004-ai-sets-targets-controller-executes.md)
(the AI sets targets, a controller executes them), [ADR 0017](0017-profile-designer.md)
(recipe cues) and the presence and takeover rules in [safety.md](../safety.md).

## Context
The first AI-controlled roast ran on 2026-11-09 on the Cubean+: Yellow
Bourbon, 200 g, profile "Brazil Natural" v2. A Claude session controlled it
over MCP after the preheat of [ADR 0018](0018-preheat-air-presence-calm-hold.md).
It went cleanly, and the AI reported what got in the way:

- **Late takeover.** The AI's `request_control` came at CHARGE, when presence
  was 2.5 min overdue: the confirmation from the preheat counts 10 min only
  for the preheat, and 3 min for anyone else. The AI got control at 2:10. Until
  then the person set the 0:00 and 1:30 steps by hand.
- **Late recipe steps.** The AI set the steps itself, up to 25 s late, because
  every question and answer in the conversation takes time.
- **Steps fixed to the clock.** The development steps belong to first crack,
  not to a time. The AI moved them by hand when first crack came later than
  planned.
- **Afraid to touch.** There was smoke, and the person did not dare raise the
  filter, because any command of theirs ends AI control.

PR #36 already covers the smaller points: DROP sets the burner to 0, switches the
cooling on and hands control back; the cooling starts 30 s before DROP; the AI has
`set_filter` and `set_cooling`; `wait` is compact; the preheat holds more calmly.

Decided with the roaster (question tool, the recommendations taken):
the AI takes over before CHARGE; a recipe runner in the daemon with steps
tied to first crack; the person's filter and cooling changes do not end AI
control; the 0:00 step may jump at CHARGE.

## Decision

### 1. Handover before CHARGE
- `request_control` is allowed while the preheat holds. As today, without a
  fresh confirmation the app asks "Ich bin am Röster". New: **the gong sounds at
  once** (not only the toast).
- After the confirmation the AI holds the lease, and **the preheat keeps holding
  the target until CHARGE**, now under the AI's lease. The AI waits, it does not
  have to hold the temperature itself. The 3 min presence interval applies from
  then on.
- **CHARGE marked by the person in the app confirms presence.** They are
  demonstrably at the roaster. A detected CHARGE, or one the AI marks, does not
  count. So the presence check never blocks the AI right after CHARGE, as it
  did tonight.
- At CHARGE the preheat ends and the recipe runner (2.) applies the 0:00 step.

### 2. The recipe runner
- The daemon follows the plan's recipe while an AI holds the lease and the
  plan is a profile. It sets each step at its trigger with source
  `Autopilot`, through the safety layer like every command, and logs the
  reason (for example "recipe 1:30: burner 85 %"). The recipe cues in the app
  then say that the step is set automatically.
- **Triggers**: as before at a roast time or at a bean temperature (already
  in the model, `ActionTrigger::Bt`), and new **relative to an event**:
  `{ event: first_crack_start | dry_end, offset_s }`. Such a step waits until
  the event is marked or detected. Until then `next_step` shows the projected
  time. The profile designer gets an "after first crack" column (a mockup comes
  first).
- **Rate:** the safety layer's limits apply (raise 10 points per 10 s), with one
  exception: in the first 10 s after CHARGE the 0:00 step may go to its value
  at once. The cold beans take the heat, and from the holding power (~36 %) to
  85 % the ramp would take 50 s.
- **The AI:**
  - It watches and adjusts the remaining recipe for this roast with a new tool
    `adjust_recipe`: move steps by Δt, change a value, skip a step.
  - It can still command directly with `set_*`.
  - A direct command does not cancel later steps for that actuator; to cancel
    one, the AI skips it.
  - After the roast the AI may propose the adjustments as a new profile version
    (`add_profile_version`); they are not saved automatically.
- No lease, no runner: when the AI releases control, its lease expires or the
  person takes over, the runner stops and the recipe goes back to cues for the
  person. Running a recipe without an AI is the autopilot (M7), which is not
  part of this ADR.

### 3. Filter and cooling stay free
- A person's command on **filter** or **cooling** is applied and does **not**
  end AI control. Commands on burner, air and drum still do: those are the heat
  loop the AI is steering.
- The machine profile names the actuators that end AI control
  (`limits.takeover_actuators`, default burner, air, drum). The safety layer
  decides by that list.
- While an AI is in control, the app marks the controls that would end it.
- The invariant in `CLAUDE.md` changes from "A human command suspends AI
  control" to "A human command on burner, air or drum suspends AI control;
  Human and Safety commands always win."

## Consequences
- The plan's moves land on time, and the AI's time goes into judgement:
  shifting steps, cutting heat ahead of first crack, deciding when to drop
  (ADR 0004's split, with the recipe as the target).
- An event-anchored recipe follows the beans instead of the clock, and makes
  profiles easier to reuse across lots.
- The person can handle smoke and the cooling tray without taking over.
- The safety layer gets a second kind of human command, and tests and
  `safety.md` must say clearly which commands end AI control.
- Open for review:
  - Do the AI's adjustments stay per roast (proposed), or become a new version?
  - Should CHARGE marked by the person also count as presence when no AI is
    involved yet? Proposed: yes; it costs nothing.
  - Does a step tied to first crack also wait when first crack is never marked?
    Proposed: it waits; the AI marks first crack when the person reports it.
