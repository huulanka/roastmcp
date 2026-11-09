# ADR 0018: Preheat at the roast's start air, a presence warning, a calmer hold

## Status
Accepted (2026-11-09), built the same day (see "As built" below). Amends
[ADR 0015](0015-preheat-step-wait-learn.md) (the preheat controller) and the
presence rules in [safety.md](../safety.md).

## Context
The step-wait-learn preheat of ADR 0015 ran on the real Cubean+ for the first
time on 2026-11-09 (empty drum, target 200 °C, app 1.13.1). An AI session
watched it over MCP without sending commands. The numbers are in
[research/preheat-control.md](../research/preheat-control.md).

What worked: one step down to the holding power, no overshoot (peak 200.5 °C),
5.5 min within 198.5–200.5 °C, 6 burner changes in 7 min against 39 with
the old PI preheat. The rest:

- **It ran at air 100 %, not at the roaster's 20 %.** The preheat has no air
  setting of its own. It keeps whatever air is applied, and files what it
  learns under that air. Connecting applies the safe state (air high), and so
  does a missed presence confirmation. Any manual command, an air change
  included, ends the preheat. So there was no way to run it at 20 %. Both
  runs started at air 100 %. The roaster's 23 Artisan roasts started at air
  20 % in 21 cases (10 % and 0 % in the other two), never above 20 % before
  CHARGE. The two with a recorded preheat (BR010, CR001) ran at 20 %
  throughout. A preheat at another air brings the drum to another state than
  the roast starts in, and what the controller learns is filed under an air
  setting the roast never uses.
- **The step to the holding power came at the right time but was too low.**
  The step came 10 °C before the target, as intended: the prediction looks
  30 s ahead, and at 20 °C/min that is 10 °C. It went to 51 % where about
  59 % was needed, and BT stood still at 193.8 °C for about a minute. The
  starting estimate (0.165 % per °C above ambient, from the 150 °C run at
  air 30 %) gives 29 % for 200 °C. The roaster's Artisan preheats at air
  20 % held 200 °C at about 34 % (CR001) and 44 % (BR010, still rising
  0.5 °C/min). The holding power grows faster than the temperature above
  ambient, so a factor fitted at 150 °C falls short at 200 °C.
- **The hold still corrected too often.** It made three corrections of 16, 10
  and 9 points (56 → 40 → 50 → 41 %), all while BT was already within ±2 °C.
  The dead band is checked on the predicted temperature only. A swing of the
  rate of rise by ±4 °C/min moves the 30 s prediction by ±2 °C, and that
  alone leaves the band.
- **Presence ended the preheat twice without warning.** Each time it came
  exactly 210 s (3 min plus 30 s grace) after the last confirmation. The app
  shows "I'm at the roaster" 120 s before that, but it sounds only with the
  alarm, when the safe state is already set. A person watching the roaster,
  not the screen, misses it. The safe state then left the air at 100 %, and
  the next start took it over.
- **The ramp to 90 % took 84 s** under the pace for automatic raises (10
  points per 10 s). In Artisan the roaster goes to 100 % at once. Kept, see
  below.

## Decision
1. **Air and target come from the plan.** The preheat card gets an air field
   next to the target. Both are filled in from the plan of the roast: the
   target from the profile's target curve at CHARGE (0:00), the air from its
   recipe at 0:00. Without a plan the card uses the last values, air 20 % by
   default. Starting first sets the air as the person's own command (source
   `Human`, through the safety layer), then the preheat takes its lease as
   before. The start request carries the air (`POST /api/preheat
   {target_c, air}`). Without `air`, the daemon keeps the air that is
   applied, as before.
2. **A person's command still ends it.** Any manual command, an air change
   included, ends the preheat (as in ADR 0015): whoever steps in has control.
   Since the air is now set at the start, there is little reason to change it
   on the way.
3. **Presence: a warning before, a longer interval before CHARGE.**
   - Whenever an AI or the preheat holds the lease, a gong sounds 60 s and
     20 s before the confirmation is due, beside the countdown that is
     already shown.
   - The space bar confirms presence while the app has focus.
   - While the preheat runs before CHARGE, the interval is 10 min instead of
     3 min, set in the machine profile next to the presence interval.
   - The grace period stays 30 s, a miss still sets the safe state, and
     presence still cannot be switched off.

   The longer interval applies only to the empty drum before CHARGE. There
   the preheat is already bounded: it never goes above the profile's
   `max_bt_c − 10`, cuts the burner above target + 10 °C, and the safety
   layer paces its raises. A preheat takes 8–12 min, so 3 min sent the
   roaster to the screen up to four times.
4. **A calmer hold.** While holding, a correction needs the measured BT
   *and* the predicted BT outside ±2 °C, on the same side. A single
   correction is at most 10 points (was 20). Unchanged: the step to the
   holding power, the urgent re-decision when the prediction misses by more
   than 8 °C, the 45 s settle time and the cut above target + 10 °C.
5. **A better starting estimate.**
   - The holding power is modelled as rising faster than the temperature
     above ambient: an exponent above 1, chosen when it is built and checked
     against the fitted Cubean+ model.
   - The Cubean+ estimate at air 20 % is anchored on the roaster's Artisan
     preheats: about 39 % to hold 200 °C.
   - Learning stays per machine and air setting (10 % steps). What is learned
     scales that curve.
6. **Related: an AI's CHARGE needs a recording.** `mark_event charge` from
   the AI without a running recording is refused ("start a recording
   first"). Until now it silently started an empty recording without beans
   or weight. The CHARGE button in the app keeps starting one.

**Not changed:** the pace for automatic raises. 84 s to full heat is little
against an 8–12 min preheat. One rule for every automation stays simpler and
safer than a faster path for one of them.

## Consequences
- Preheats learn under the air the roast starts with (20 % on the
  roaster's Cubean+). The `air_100` entry this test left in `control.json`
  is not used by them. It does no harm, and deleting it tidies up.
- The preheat API takes `air`. The MCP still has no preheat tool, because
  starting it counts as a presence confirmation.
- The machine profile gets a preheat presence interval. `docs/safety.md`
  describes the warning, the space bar and the interval once they are built.
- Tests: the hold with the new band rule and the 10-point step on the fitted
  model and in the simulator; a start at applied air 100 % that sets the
  plan's air first; the AI's CHARGE without a recording refused.
- Acceptance on the machine: a preheat at air 20 % to about 200 °C that
  steps to about 39 %, makes at most 3 changes while holding and needs one
  presence confirmation.

## As built
- **Holding power:** `hold(ΔT) = hold_pct_200 × (ΔT / 178 °C)^1.8`, with
  `hold_pct_200` = 39 % by default for every air setting. 1.8 fits both
  real points: 21 % for 150 °C at air 30 % and about 39 % for 200 °C at
  air 20 % (the air 100 % run learned the equivalent of 41 %).
  `control.json` keeps `hold_pct_200`; an older entry with
  `hold_pct_per_c` is read as that factor × 178 °C.
- **Learning the holding power:** with the new band rule the curve tends to
  turn only at the band's edges, where the old rule (rate of rise below
  1 °C/min, inside the band) never learned. It now learns from every
  settled reading inside the band: the burner that would give a rate of
  rise of zero, `burner − RoR / burner strength`.
- **The step to hold:** its boost above the holding power (to close the
  remaining error within 2 min) is capped at 15 points. At a high rate of
  rise the measured error is large when the prediction reaches the band,
  and with corrections of 10 points the old uncapped boost (up to 39
  points) overshot by 11 °C in the simulator.
- **Learned values of a far-off air setting** are not used: a preheat takes
  the values of its own air setting, else the nearest within 20 points,
  else the built-in estimate. So the `air_100` entry no longer reaches a
  preheat at air 20 %.
- **Presence:** the safety layer gives the longer interval
  (`limits.preheat_presence_interval_s`, 600 s on the Cubean+) only to the
  lease holder the daemon names for the preheat. An AI that takes over
  mid-preheat needs a confirmation within the normal 3 min.
- **Test model:** the Cubean+ model of ADR 0015 is linear and would hold
  200 °C at 24 %. Its losses now grow with the temperature so that it holds
  as the machine does (39 % at 200 °C), with the burner acting as fitted. On
  it the preheat reaches 200 °C after about 10.5 min, peaks at 203.1 °C and
  makes 3 changes in the first 5 min of holding; the step is about 53 %
  (39 % plus the boost) and the hold settles at about 42 % and then lower as
  the drum soaks up heat. A machine 40 % stronger than the estimate peaks at
  205.3 °C: the 10-point corrections trade a few degrees for calm.
- **App:** the preheat card is one row: target (°C), air (%), and a
  start/stop icon button like the burner's −/+ (the column has no room for a
  second row without scrolling at 1280×800). The plan fills in target and
  air; the status then reads "per plan".

## On the machine (2026-11-09 evening)
The acceptance run ([research](../research/preheat-control.md#the-third-real-preheat-air-20--adr-0018-acceptance))
kept air 20 %, stepped once to 53 %, peaked at 203.2 °C and needed one
presence confirmation before the hold, but made 4 changes in the first 5
min of holding: it cut every 45 s (the settle time) while the curve was
already turning, and once cut below the band although the curve was rising
back into it. Two rules were added to the hold:

- **A second correction in the same direction waits 90 s** after the last
  change (`repeat_after_s`): the machine needs about that long to show a
  change's full effect. A gross miss (more than 8 °C predicted) still acts
  after 20 s.
- **No correction while the rate of rise already carries the temperature
  back towards the target** at 1 °C/min or more (`returning_ror`).

In the test model this gives 2 changes in the first 5 min of holding at the
estimate (peak 203.5 °C), 1–3 for machines 30 % weaker or 20–40 % stronger
and with 10 s more delay (peaks 202.1–205.6 °C), and never below 198 °C.
The machine has since learned 35.9 % for 200 °C at air 20 %.
