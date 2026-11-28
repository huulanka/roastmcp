# ADR 0023: The machine's reported values are checked, and a missed presence confirmation never ends a roast

## Status
Accepted (2026-11-28), after BRYB-004. Amends
[ADR 0018](0018-preheat-air-presence-calm-hold.md) (what a missed presence
confirmation does), [ADR 0019](0019-handover-before-charge-and-recipe-runner.md)
and [safety.md](../safety.md).

## Context
On 2026-11-28, about 4:06 into BRYB-004 under the autopilot, the Cubean+
reported burner 0 % and air 100 % in its READ replies for about 30 s,
although the recipe had just set burner 80 %, air 20 %. The roaster heard
the fan go up. The audit log shows no command, no safety reaction, no
emergency stop and no lost link in that time: the machine went there on its
own. Why is not known (firmware or a Bluetooth hiccup; to be characterised).
The rate of rise fell from about 14 to 7.5 °C/min before the person set the
burner and the air by hand, which ended the autopilot (a person's command
on burner or air, ADR 0019); they switched it on again.

The daemon took the reported values as the new truth (`Safety::observe`)
without a word. Nothing was sent again, nothing sounded, so the person, who
watches the roaster and not the screen, had to find out by ear.

Earlier the same evening a missed presence confirmation during the preheat
ended it with the safe state. The roaster asked that a missed confirmation
must never end a roast or a preheat: a loud alarm at most, and a switch to
turn the questions off. They are always at the roaster; one overlooked
prompt must not cost a batch.

## Decision

### 1. Reported values are compared with the values set
The safety layer keeps the values commanded through it (the setpoints)
apart from what the machine reports. For each actuator the machine reports
(Cubean+: burner and air):

- a deviation of more than 8 points that lasts 6 s (the machine may ramp;
  the Cubean+ quantises by a few points) is a critical alarm, and the value
  set is sent again as a safety-layer command, logged with the roast;
- it is sent again at most twice, 6 s apart. If the machine still does not
  follow, a last critical alarm asks the person to check the roaster and
  take over, and the reported value counts as the value set from then on;
- a value toward the safe state (less burner, more air) is always sent
  again. A value away from it is not sent during an emergency stop or an
  overheat: then the machine's lower heat is the safer side, and the person
  is asked instead;
- until a deviation is settled, the safety layer's view keeps the value set,
  so a reading taken just before a command arrived does not become the base
  for the next step limit or autopilot trim;
- nobody's control ends: the autopilot and an AI keep the lease. A person's
  command still ends AI control as before.

A person who changes a value on the machine itself would be overruled twice
before RoastMCP gives in. The Cubean+ is run from the computer, so that is
accepted; a machine where it matters can be handled in its profile later.

### 2. A missed presence confirmation is an alarm, not a reaction
When a confirmation is overdue (interval plus 30 s grace), the safety layer
raises a critical alarm and repeats it every 30 s until a person confirms.
It sets no safe state, takes no lease and suspends nothing:

- the preheat and the autopilot go on. They were started by a person in the
  app, and they run inside their own limits, the temperature limit and the
  runaway check (ADR 0022). Their leases are the daemon's own and need no
  fresh confirmation (`acquire_own_lease`);
- an AI client keeps its lease and may still cut heat and add air, but a
  command that moves away from the safe state (more burner, less air) is
  refused with `presence_required` until a person confirms;
- an AI that takes control (a new lease) still needs a recent confirmation.
  Taking control stays a critical step, and only the app can confirm.

### 3. The person can switch the questions off
The app's settings have a switch "Presence questions" (on by default),
stored in the database. Off, nothing is due: no gongs, no prompt, no alarm,
and an AI in control may raise heat without a fresh confirmation. Taking
control still needs one confirmation. Only the app sets it
(`PUT /api/presence/checks`); the MCP has no tool for it.

### 4. The MCP bridge finds a restarted daemon
The daemon writes a new port and token to `daemon.json` at every start. The
stdio bridge read it once, so after a restart every call failed with
"missing or invalid token" until the AI client was restarted. Now a 401
answer, or a connection that cannot be made, makes the bridge read
`daemon.json` again; if port or token changed it sends the request once
more. A request that may have reached the daemon (a timeout, a broken
answer) is never sent twice.

### 5. The roasted weight can be written by an AI
`set_roast_weight` writes the roasted weight the person weighed (and a
corrected green weight) into a saved roast's fields, so the weight loss is
in the figures and not only in a note.

## Consequences
- The two invariants "AI control requires recent presence" and "a missed
  confirmation sets the safe state" become: taking control requires a
  recent confirmation; without a fresh one an AI cannot raise heat; the
  daemon's own automation is bounded by its limits, not by presence.
- A machine that falls back on its own is noticed within about 6 s, heard,
  and usually corrected without the person touching anything.
- The tolerance (8 points) and the times (6 s) are constants in
  `roast-safety`; if another machine ramps slower they become profile
  settings.
