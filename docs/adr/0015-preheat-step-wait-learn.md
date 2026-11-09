# ADR 0015: Preheat by step, wait and learn instead of a fast PI loop

## Status
Accepted (2026-11-09). Replaces the preheat PI controller of the first desktop
app. The same rules (a deliberate step, then a wait for the machine's lag)
are meant for the autopilot of [ADR 0004](0004-ai-sets-targets-controller-executes.md)
and for limits on AI commands; this ADR only builds the preheat. [ADR 0018](0018-preheat-air-presence-calm-hold.md)
amends it after the first run on the machine (air from the plan, a calmer
hold, a better starting estimate).

## Context
The first preheat on the real Cubean+ (2026-11-09, empty drum, air 30 %,
target 150 °C) used a PI controller that acted every 5 s (P 2.5 %/°C,
I 0.05 %/°C·s, burner off above target + 5 °C). It changed the burner 39
times in 7 min, overshot to 155.9 °C, cut to 0 twice and only then held. In the
simulator it swung between 190 and 206 °C around a 200 °C target with three
cuts to 0 and 52 changes. The roaster said it "stellt viel zu viel rum": it
should make one step, wait for the sluggish machine and keep a dead band, as
a person roasts. The roaster also asked to compare with Artisan's software
PID.

What the data say (fit and comparison in
[research/preheat-control.md](../research/preheat-control.md)):

- The empty Cubean+ behaves almost like an integrator: two first-order paths
  (gain 0.67 and 6.75 °C per burner %, time constants 252 s and 2013 s) with
  1.6 s delay fit the real run with 0.8 °C RMS error. Holding 150 °C took about
  21 % burner.
- Most of the visible lag is the measurement: the rate of rise is a 20 s
  least-squares fit, about 10 s behind.
- Artisan's PID (Kp 15 %/°C, Ki 0.07, Kd 0.06 in the vendor's Cubean+
  settings) computes on every reading and sends every change of 1 % or more —
  the constant adjusting the roaster does not want.

## Decision
A new controller in `roast-control` (`PreheatController`), pure logic without
I/O; the daemon feeds it every reading and sends its decisions through the
safety layer as source `Autopilot`, so raises stay bounded (10 points per
10 s) and anything a person does ends the preheat, as before.

1. **Heat:** full heat (90 %) while far from the target.
2. **One step to hold:** when the predicted temperature (BT + RoR × 30 s)
   reaches the target − 2 °C, one deliberate step down to the burner that holds
   the target, plus what closes the remaining error within 120 s.
3. **Wait:** after any change, no new decision for 45 s (settle time), unless
   the prediction misses by more than 8 °C — then after 20 s at the earliest.
4. **Dead band:** while the predicted temperature is within ±2 °C of the
   target, nothing changes.
5. **Correct:** outside the band, one step that aims the rate of rise at
   closing the error within 120 s; at least 3 % (a smaller step is not worth
   making), at most 20 %.
6. **Cut:** above target + 10 °C the burner goes off at once.
7. **Learn:** after each settled change of at least 5 %, the change of the
   rate of rise per burner percent updates the burner's strength (bounded to
   half or double per step); steady in the band, the burner updates the
   holding power. Both are saved per machine and air setting (in 10 % steps)
   in `control.json` in the data folder; the next preheat starts from them.

In the terms of a PID this is a velocity-form PI with derivative on the
measurement (the rate of rise, already filtered), a sample time equal to the
settle time, a dead band on the input, a minimum step on the output and an
adaptive gain. From Artisan's design it keeps derivative on measurement,
stopping the integral at the output limits and a bumpless start; it drops the
per-reading cadence and the high gain.

Checked in tests (`crates/roast-control/tests/preheat.rs`) against the fitted
Cubean+ model and the simulator: on the model from cold to 200 °C the peak is
201.5 °C and the burner changes 5 times around and after the target; it stays
within ±3 °C with the machine 30 % weaker or 40 % stronger than assumed or
with 10 s more delay, and after a roast (hot drum, burner high). The simulator
(about three times as strong, slow probe) peaks at 205 °C on the first run
and learns its burner; with that, a hot restart stays within 2 °C.

## Consequences
- The preheat makes a few large changes instead of many small ones; the
  action log shows each with its reason (full heat, step to hold, correction,
  far too hot).
- Its first run on a machine (or at a new air setting) starts from the
  Cubean+ estimate; the second run is better. Deleting `control.json` resets it.
- The tuning (lead, settle time, band, steps) is built in for the Cubean+. A
  `[control]` section in the machine profile follows when a second machine
  needs other values.
- The autopilot (M7) and the rules for AI commands should reuse the same
  pieces: prediction from the rate of rise, settle time, dead band, learned
  strength.
