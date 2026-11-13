# ADR 0020: The autopilot runs the recipe and trims the burner, switched on in the app

## Status
Accepted (2026-11-10), built the same day; proven in the simulator, not yet on
the machine. Amends [ADR 0004](0004-ai-sets-targets-controller-executes.md)
(what the controller follows), [ADR 0019](0019-handover-before-charge-and-recipe-runner.md)
(the recipe runner needed an AI's lease) and the lease rules in
[safety.md](../safety.md).

## Context
Since ADR 0019 the daemon sets a profile's recipe steps on time, but only
while an AI holds the lease. Without an AI client the steps are cues and the
person sets each one. ADR 0019 left "running a recipe without an AI" to the
autopilot (M7).

The roadmap described M7 as a controller that follows rate-of-rise targets an
AI sets. Two things changed that picture:

- The recipe is the plan. A profile ([ADR 0017](0017-profile-designer.md))
  already says when the burner and the air go where, and those steps come
  from a real roast that went well. A controller that derives the burner from
  a curve alone would have to rediscover them.
- The roaster asked for calm control ([ADR 0015](0015-preheat-step-wait-learn.md)):
  a step, then wait for the sluggish machine, a dead band, no constant
  fiddling. On the Cubean+ a burner change shows in the rate of rise after
  15–30 s and has its full effect after about 90 s.

The simulator could not carry a roast to first crack, so nothing automatic
could be tried in it. It is now fitted to the real machine
([research/simulator-calibration.md](../research/simulator-calibration.md)).

Decided with the roaster (question tool, the recommendations taken): the
autopilot is the recipe plus a fine control; a person switches it on in the
app, without an AI; it is tested in a calibrated simulator.

## Decision

### 1. What it does
From CHARGE to DROP the autopilot

- sets the recipe's steps at their triggers, as the recipe runner of ADR 0019
  does under an AI's lease, and
- **trims the burner** between the steps so the bean temperature stays on the
  profile's curve.

Air, drum, filter and cooling are the recipe's and the person's. The trim
only moves the burner. It does not drop: the person marks DROP (or it is
detected), and then the burner goes to 0, the cooling comes on and the
autopilot ends, as after any roast.

### 2. The trim
The trim (`roast-control::trim`) looks at each reading and decides by the
rules a careful roaster uses:

- **Look ahead.** It predicts where the bean temperature will be relative to
  the plan in 90 s: the deviation now, plus the difference between the
  measured and the planned rate of rise over that time.
- **Dead band.** Inside ±3 °C it leaves the burner alone. A roast that runs
  as planned gets no correction at all.
- **One step.** Outside the band it moves the burner once, by the amount
  that makes up the deviation over two minutes (0.25 °C/min per burner
  point with beans), at most **5 points**.
- **Wait.** After its own correction it waits **90 s** before it looks
  again. After anyone else's burner change (a recipe step, a command) it
  waits 20 s; the plan's curve already expects the recipe's steps.
- **Stay near the recipe.** All corrections together stay within **±10
  points** of the recipe. A correction carries over to the recipe's later
  burner steps: if the trim added 5 points, the next step lands 5 points
  above its planned value. If the roast is further off than 10 points can
  mend, the trim stops there and the person decides.
- **Not too early.** No correction in the first 2:30, while the probe cools
  into the beans and turns. Never below 10 % burner.

Each correction goes through the safety layer with source `Autopilot`, under
the same limits as every automatic command, and is logged with its reason
("autopilot: the roast heads 5 °C below the plan"). The app shows it as a
message and in the history.

### 3. Who switches it on
- **A person, in the app.** `POST /api/autopilot` is the switch. It needs a
  connected roaster, no latched emergency stop, and a profile as the plan.
  Switching it on is the person's word that automation may run (as
  "allow AI again") and counts as a presence confirmation, as starting the
  preheat does.
- The switch is a pill in the app's top bar, beside the other marks of who
  is in control, shown while a profile is the plan. It says "from CHARGE",
  "active" and how far the corrections moved the burner ("active +5"). A
  line of its own in the controls pushed the cooling switch out of view at
  1280×800, so it is not there.
- **There is no MCP tool for it**, for the same reason there is none for the
  preheat: starting it confirms presence, and only the app can do that.
- Before CHARGE it is **armed**: a running preheat goes on holding, and the
  autopilot takes over at CHARGE. After CHARGE it starts at once.
- The daemon holds the lease itself, as holder `autopilot`, and renews it.
  The names `autopilot` and `preheat` are reserved; an AI cannot ask for a
  lease under them or release theirs.
- **Presence** is confirmed every 3 min as under an AI. CHARGE marked by the
  person in the app counts (ADR 0019). After a detected CHARGE without a
  fresh confirmation the autopilot waits and the app asks "I'm at the
  roaster".

### 4. What ends it
- The switch in the app (`DELETE /api/autopilot`): the actuators hold.
- A person's command on burner, air or drum (the takeover rule, ADR 0019).
  Filter and cooling stay free.
- DROP, or the end of the recording.
- An emergency stop, a lost link or a missed presence confirmation, each with
  the safe state as before.

Afterwards the recipe's steps are cues for the person again, with the
autopilot's corrections still in them (marked as adjusted, like an AI's
changes). Switching the autopilot on again in the same roast takes up the
rest of the recipe, and the earlier corrections still count toward the ±10
points. Choosing another plan while it is on starts the trim afresh; a plan
that is not a profile switches it off.

### 5. With an AI
An AI may still ask for control while the autopilot is on. It then holds the
lease, and the autopilot keeps running the recipe and trimming under it. The
AI watches, comments, marks events and adjusts the remaining recipe
(`adjust_recipe`). When it lets go or its lease runs out, the autopilot takes
its own lease back. The status an AI reads shows the autopilot's state, its
offset from the recipe and the number of corrections.

The trim does not know why an AI moved the burner. It treats an AI's own
burner command like any other change: it waits, then keeps to the curve
within its ±10 points. An AI that wants to steer by itself asks the person to
switch the autopilot off.

### 6. Critical steps
ADR 0004 asks for a person's confirmation before critical steps. With this
design the start is the person's own switch, DROP stays with the person, and
the autopilot makes no large changes of its own: the recipe's steps are the
profile the person chose, and a correction is at most 5 points.

## Consequences
- A profile can be roasted without an AI client: preheat, CHARGE, and the
  daemon sets the steps and keeps the curve. The person watches, marks first
  crack and drops.
- Steps tied to first crack still wait for the mark. Until the app hears
  first crack (M6), the person marks it.
- The trim's numbers are defaults in the code, chosen in the simulator: with
  a machine five burner points weaker or stronger than on the day of the
  plan, the roast ends within 3 °C of the plan after one to three
  corrections; without the trim it ends more than 5 °C off. They are not yet
  tuned on the machine.
- The lease now has three kinds of holder: an AI, the preheat and the
  autopilot. `safety.md` says which rules each follows.
- ADR 0004's controller with a model of the machine (feed-forward from the
  twin, a correction on the rate of rise) is not built. If the trim proves
  too weak on the machine, that is the next step.

## Open
- Acceptance on the machine: three supervised roasts within the profile's
  tolerance, with no safety violation (the roadmap's condition for M7).
- The trim's tuning per machine (machine profile), and learning the burner's
  effect from roasts as the preheat does.
- Whether the trim should also move the air.
