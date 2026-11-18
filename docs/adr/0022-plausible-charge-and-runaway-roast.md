# ADR 0022: A detected CHARGE must look like beans, and a roast that runs away loses its automation

## Status
Accepted (2026-11-18), after the false start before BRYB-003. Amends
[ADR 0019](0019-handover-before-charge-and-recipe-runner.md) (what happens at
a detected CHARGE) and [safety.md](../safety.md).

## Context
On 2026-11-18 the roaster opened the drum during the preheat at 200 °C to
take out some chaff. The bean probe fell by about 34 °C. The event detector
saw a peak and a fall of 5 °C within 8 s and called it CHARGE. The preheat
ended and an AI client, which held the lease, let the recipe run: burner
80 % into an empty drum. The probe turned at 166 °C after 25 s (the detector
also marked "dry end" at 168 °C) and climbed at about 50 °C/min. After 56 s
it was 89 °C above the plan, heading for 239 °C within a minute. The AI
client pressed the emergency stop. Nothing in the daemon would have acted
before the 240 °C limit.

With beans, the probe falls from 200 °C to about 126 °C within 30 s and turns
near 100 °C after about 70 s (BRYB-003, the same evening). The two are far
apart, so a check after the fact is reliable; making the detector itself
stricter would delay every real CHARGE.

Decided with the roaster (question tool): a CHARGE that was none goes back
to the preheat.

## Decision

### 1. A detected CHARGE is checked
A CHARGE the detector found (origin `auto`) stays only if, as with beans:

- 30 s later the lowest bean temperature since is below 150 °C and at least
  40 °C below the temperature at CHARGE,
- the turning point is not above 140 °C, and
- a turning point comes within 2 min.

When a turning point at or below 140 °C has come after a deep enough fall,
the check is done. A CHARGE a person marked is not checked: it is the
person's word, and section 2 covers it.

### 2. A CHARGE that was none is taken back
- The CHARGE, the detected turning point and dry end, and heard crack marks
  leave the roast; the recording goes on, waiting for CHARGE.
- The recipe starts afresh, with no trim corrections. An armed autopilot
  stays armed and takes over at the next CHARGE; its lease is released.
- The preheat takes up its target again. It does not ask for a new presence
  confirmation and does not change the air: it needs the confirmation the
  person gave before (10 min in the preheat). Under an AI's lease it holds
  under that lease, as before CHARGE.
- If it cannot (no preheat ran, presence missed, AI control suspended), the
  safety layer sets the safe state.
- A critical alarm and a note say why: "CHARGE taken back: it turned at
  166 °C after 25 s - beans turn near 100 °C. No beans went in? Charge again
  when ready."

### 3. A roast that runs away
From 20 s after CHARGE the daemon watches for a roast that runs away from
what beans do:

- more than 25 °C above the plan for 10 s, or
- heading within 10 °C of the machine's temperature limit within a minute
  (bean temperature plus one minute of rate of rise).

Under automation (an AI or the autopilot holds the lease) the safety layer
takes the safe state through a new reaction `runaway`. Unlike the emergency
stop it does not latch: the person can go on at once. Automation ends until
the person allows it again. Under a person's hand only a critical alarm
sounds: the person wins, and the temperature limit itself still cuts the
burner. It acts once per roast.

## Consequences
- The false start of 2026-11-18 would have been taken back after about
  28 s (turning point at 166 °C), with the preheat holding 200 °C again and
  the autopilot still armed. Opening the drum in the preheat is safe.
- The numbers are constants in the daemon, chosen from the two roasts of
  that evening. They are generous for a 200 g batch on the Cubean+. Smaller
  batches fall less; if a real CHARGE is ever taken back, they move into
  the machine profile.
- Tests: the probe curve of the false start and a bean curve shaped like
  BRYB-003's are engine tests; the runaway guard has one under the autopilot
  and one under a person's hand; the `runaway` reaction has a safety-layer
  test.
