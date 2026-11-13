# Simulator calibration: the Cubean+ with beans

Background for [ADR 0020](../adr/0020-autopilot-recipe-and-trim.md). Done
2026-11-10.

## Why

The simulator's heat model was tuned by eye to give plausible curves. On
2026-11-09 a practice roast in it stalled near 185 °C at 55–80 % burner
(first crack and DROP both marked at 183.9 °C), so a whole roast could not be
practised, and automatic control could not be tried on anything like the real
machine's lag. The autopilot has to be proven in the simulator before it
touches the machine, so the simulator has to behave like the machine first.

## The model

The model keeps its two heat stores, the drum (with the air in it) and the
beans, and a probe that lags:

- The heater puts `1700 W × burner` into the drum.
- The drum loses heat to the room faster than in proportion to its
  temperature: `LOSS_W × (1 + AIR_LOSS × air) × ΔT^1.8`. A linear loss cannot
  hold both 150 °C and 200 °C at the burner values measured.
- Drum to beans: `BEAN_TRANSFER_W_PER_K × (1 + AIR_TRANSFER × air) × (T_drum − T_bean)`.
  More air moves more heat into the beans.
- The beans lose heat to evaporation above 95 °C while they hold moisture
  (10 % of the batch), and gain a little between 190 and 225 °C.
- The probe reads mostly the beans (`PROBE_BEAN_SHARE`) and a little of the
  drum, through a first-order lag (`PROBE_TAU_S`).

Burner and air are fractions (0–1) in these formulas.

## The data

- Four real roasts on the Cubean+ (three of 200 g, one of 222 g), from CHARGE
  to DROP, with the burner and air as they were set (29 September, 2 October,
  8 and 9 November 2026; DROP between 196 and 204 °C after 9:19 to 10:53).
- The empty drum at air 20 % (2026-11-09): 38 % burner holds 200 °C, 21 %
  holds 150 °C, and at 90 % the probe rises about 25 °C/min around 90 °C.

The roasts are personal logs and are not in the repository.

## The fit

Ten constants, fitted by least squares (Nelder–Mead from several starting
points): the squared RMS error of the probe over each roast, plus the three
empty-drum facts, each weighted so that 2 points of burner or 3 °C/min count
like 4 °C RMS on a roast. The loss exponent was set to 1.8 by hand.
`scripts/fit-simulator.py` repeats the fit, or checks the constants in the
simulator against a set of roasts.

| Constant | Value |
|---|---|
| `DRUM_J_PER_K` | 3431 J/K |
| `LOSS_W`, exponent | 0.04766 W, 1.8 |
| `AIR_LOSS` | 1.04 |
| `BEAN_TRANSFER_W_PER_K` | 6.967 W/K |
| `AIR_TRANSFER` | 1.602 |
| `BEAN_J_PER_KG_K` | 6493 J/(kg·K) |
| `EVAPORATION_W_PER_K` | 2.429 W/K |
| `EXOTHERM_W` | 13.67 W |
| `PROBE_BEAN_SHARE` | 0.9396 |
| `PROBE_TAU_S` | 48.04 s |

## How close it is

| Roast | RMS error | DROP, model / real | Turning point, model / real |
|---|---|---|---|
| 29 September | 3.7 °C | 202.0 / 203.7 °C | 102 °C at 1:21 / 98 °C at 1:08 |
| 2 October | 3.8 °C | 203.2 / 202.9 °C | 100 °C at 1:24 / 95 °C at 1:14 |
| 8 November | 3.9 °C | 196.9 / 196.1 °C | 102 °C at 1:21 / 98 °C at 1:10 |
| 9 November | 3.6 °C | 203.1 / 202.9 °C | 100 °C at 1:24 / 94 °C at 1:13 |

Empty drum: 200 °C at 38.1 %, 150 °C at 21.0 %, 24.8 °C/min at 90 %.

The simulator's tests hold it to this: the empty drum's rise and hold, and a
roast with the recipe of the 9 November roast, which must pass the real
milestones (turning point, 160 °C, 191 °C, 203 °C) within about half a
minute.

## What it is not

- The turning point comes about 12 s late and 4–6 °C high in the model: one
  lag for the probe is too simple for the first minute, when the hot probe
  cools into the cold beans.
- `BEAN_J_PER_KG_K` is four times the heat capacity of coffee. It stands for
  everything that warms up with the beans, as the probe sees it, and should
  not be read as a property of the beans.
- All four roasts are batches of about 200 g with similar profiles, so the model says
  little about other batch sizes, very different air settings or dark roasts.
- There is no first crack as an event, and no smoke or chaff.

The simulator is for practising and for testing control. Predicting a
real roast stays the digital twin's job (`roast-twin`, M5).
