# Preheat control: the Cubean+ data and Artisan's PID

Background for [ADR 0015](../adr/0015-preheat-step-wait-learn.md). Collected
2026-11-09.

## The first real preheat

Skywalker Cubean+, empty drum, air 30 %, drum 100, target 150 °C, app 1.8.0,
the PI preheat (P 2.5, I 0.05, every 5 s). Readings every 1.5 s from the wire
trace of the daemon log.

- Ramp 0 → 90 % in about 80 s (the safety layer's pace for automatic raises).
  At 90 % the bean probe rose about 25 °C/min between 60 and 115 °C and about
  20 °C/min between 130 and 145 °C.
- 150 °C after about 3.5 min; peak 155.9 °C, burner cut to 0, ramp again, a
  second cut at 155.0 °C; then 151–152 °C at about 20–22 %.
- 39 burner changes in 7 min.
- After a big burner change the 20 s rate of rise responds after 15–20 s and
  the temperature turns after 40–60 s.

### A model of the empty drum

Fitted by least squares to the 700 s with air 30 % (burner 0–90 %):

| Model | RMS error | Parameters |
|---|---|---|
| One first-order path | 1.4 °C | 4.3 °C/%, τ 747 s, no delay; misses the holding power |
| Two parallel first-order paths + delay | 0.8 °C | 0.67 °C/% with τ 252 s, 6.75 °C/% with τ 2013 s, delay 1.6 s |

With a time constant of half an hour the drum is close to an integrator at
preheat temperatures: the burner sets the rate of rise almost at once
(0.22 °C/min per burner percent), and the temperature keeps whatever it
reached. The delay a person sees is mostly the measurement: a 20 s
least-squares slope is about 10 s behind.

The built-in simulator's empty drum is about three times as strong
(0.6 °C/min per %) and has a 25 s probe lag; it reaches only about 185 °C with
beans at 55–80 % burner, so it is not calibrated to the Cubean+ yet.

## Artisan's software PID

From reading Artisan's source (`artisanlib/pid.py`, `pid_control.py`; facts
only, in our own words — Artisan is AGPLv3):

- Runs on every reading (the caller's cadence; 1.5 s on the Cubean+), with the
  measured time step. Error = set value − measured value.
- P on a weighted set value (weight 1 by default); I as the sum of error ×
  time; D on the change of (weighted set value − measured value); with the set
  value weight for D at 0 it is derivative on measurement. Optional low-pass
  filters for the derivative (0.1 Hz) and the output (0.35 Hz), off by
  default; the derivative is capped (100 units/s by default) and damped after
  a large set value jump or a measurement jump.
- Windup protection on by default: the integral is limited to the output
  range, not integrated while the output is saturated in the same direction,
  and reduced by half the clamped excess (back-calculation). Optional integral
  reset on large set value changes.
- Output clamped to 0–100 %; sent whenever it moved by at least one step
  (1 % by default) or every third cycle — no rate limit.
- Optional gain scheduling on the set value or the measured value.
- Bumpless start: while off, its set value follows the measured value.
- The set value can be manual, a ramp/soak program, or the background
  profile read a number of seconds ahead (lookahead).

The Cubean+ vendor settings (`modified_artisan+setting (Skywalker Series -
Cubean+) 20250807.aset`): Kp 15, Ki 0.07, Kd 0.06, cycle 1000 ms, set value
weights 1, windup protection on, integral limit factor 1, output 0–100 % to
event slider 4, source bean temperature, PID on at CHARGE, set value mode 2
(follow the background) with 20 s lookahead.

Kp 15 means 15 % burner per °C of error: on this almost integrating machine a
2 °C error swings the burner by 30 points, and every reading may send a new
value. That suits following a background curve closely; it is the opposite of
the calm, deliberate changes the roaster asked for.

## What the new preheat takes and leaves

| | Artisan PID | Old preheat | New preheat |
|---|---|---|---|
| Decides | every reading | every 5 s | after each change waits 45 s |
| Output change | any ≥ 1 % | any ≥ 2 % | ≥ 3 %, ≤ 20 %, outside a ±2 °C band only |
| Derivative | on error or measurement, optional filter | none | rate of rise (20 s fit) as prediction, 30 s ahead |
| Integral | windup-protected | conditional integration | velocity form, no stored integral |
| Overshoot guard | none | burner 0 at +5 °C | burner 0 at +10 °C; prediction avoids it |
| Learns | gain scheduling, fixed | no | burner strength and holding power, saved |

On the fitted model, from cold to 200 °C (`crates/roast-control/tests/preheat.rs`),
the new preheat peaks at 201.5 °C and changes the burner 5 times around and
after the target; the old one changed it 39 times in 7 min on the machine and
52 times in the simulator.
