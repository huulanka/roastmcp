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

## The second real preheat (200 °C)

2026-11-09 afternoon, Skywalker Cubean+, empty drum, target 200 °C, app
1.13.1, the step-wait-learn preheat of ADR 0015. An AI session watched it
over MCP without sending commands. Background for
[ADR 0018](../adr/0018-preheat-air-presence-calm-hold.md).

- Both runs started at **air 100 %**: connecting and the missed presence
  confirmation before the second run each applied the safe state, and the
  preheat keeps the air it finds.
- The first run (from 23 °C) ended after 3.5 min at about 120 °C, when the
  presence confirmation was missed. The second started at about 120 °C:
  - **Ramp:** the burner reached 90 % after 84 s under the raise pace. BT
    fell about 5 °C meanwhile at air 100 %. Then it rose from 118 to 190 °C
    in 3.5 min, about 20 °C/min.
  - **Step to hold:** at 190.5 °C, 90 → 51 %. BT stood at 193.8 °C for about
    a minute, and a correction to 59 % followed.
  - **Peak and hold:** the peak was 200.5 °C. BT stayed within 198.5–200.5 °C
    for 5.5 min, starting about 2 min after the step.
  - **Burner changes:** 9 ramp steps (from the raise pace only), then 6
    changes in 7 min: 51 → 59 → 56 → 40 → 50 → 41 %. The last three came
    while BT was already in the band.
- **Learned (air 100 %):** 0.393 °C/min per burner percent (estimate 0.22)
  and 0.231 % per °C above ambient to hold (estimate 0.165). That is about
  41 % for 200 °C.
- **Presence:** the confirmation was missed twice, each time 210 s (3 min
  plus 30 s grace) after the last one. The app sounded only with the alarm.

## The third real preheat (air 20 %, ADR 0018 acceptance)

2026-11-09 evening, Skywalker Cubean+, empty drum from 23 °C, target 200 °C,
air 20 % throughout, the build of ADR 0018 (holding power estimate 39 %, no
learned values for air 20 %). A read-only watcher polled the status every
5 s.

- **Ramp:** 10 points every 10 s to 90 % in 1:25.
- **Step to hold:** at 8:36 and 191.5 °C (RoR 13.6 °C/min), 90 → 53 %:
  39 % plus a boost of 14.
- **Hold:** 53 → 50 % at 9:26 (196.1 °C, still rising at 3.2 °C/min), then
  50 → 40 → 34 → 30 → 24 % between 11:21 and 13:41, every 45–50 s, at
  202–203 °C. Peak 203.2 °C. Then 24 → 34 → 39 % at 14:51 and 15:36
  (197.7 °C), and 39 → 29 % at 18:47. CHARGE at 20:37 at 198.1 °C.
- **Acceptance:** 4 changes in the first 5 min of holding (at most 3
  wanted), 58 % of the hold within ±2 °C. Two presence confirmations with
  gong and Space; the passing driver error from before the start did not
  stick.
- **Response of the machine:** after the isolated cut 39 → 29 % the RoR
  went 2.5 → −1.4 (15 s) → −2.9 (45 s) → −3.3 °C/min (90 s). A burner change
  shows within 15–30 s, turns the curve after about 45 s and has its full
  effect after about 90 s. The cuts at 12:06 and 12:51 came while the RoR
  was already −0.2 and −0.5 °C/min.
- **Learned (air 20 %):** 0.434 °C/min per burner percent, holding power
  35.9 % for 200 °C.

## The roaster's Artisan preheats

From the roaster's 23 Artisan files of the Cubean+ (June–November 2026):

- **At CHARGE:** BT 194–204 °C, air 20 % in 21 files (10 % and 0 % in the
  other two). Before CHARGE the air was never above 20 %.
- **Two files record the whole preheat, both at air 20 % throughout:**
  - BR010: burner 100 % to about 195 °C after 8.5 min, then 43–47 % with BT
    rising slowly, 200–202 °C.
  - CR001: burner 100 % to about 191 °C after 7 min, then a PID swinging
    between 21 and 63 % while holding about 199–200 °C, on average about
    34 %.
- **Holding power:** about 34–44 % for 200 °C at air 20 %, against the
  ADR 0015 estimate of 29 % (0.165 × 178 °C). Holding 150 °C at air 30 %
  took about 21 % (the first run). The holding power grows faster than the
  temperature above ambient.
