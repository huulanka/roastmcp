# How roasters move the heat

Background for the AI limits in `docs/safety.md`. Collected November 2026.

## Practice

- **Big cuts before and during first crack are normal.** Roasting guidance
  commonly says to lower the gas about 45 s before first crack (for washed
  coffees) and again about 45 s after it starts, because a later cut risks a
  sharper crash of the rate of rise
  ([Barista Hustle](https://www.baristahustle.com/?p=463657)). Practitioners
  report cuts of 15 % about a minute before first crack and of 30-50 % around
  the middle of first crack
  ([Home-Barista: Scott Rao on the flick](https://www.home-barista.com/roasting/scott-rao-on-flick-t37175-20.html)).
  These figures are anecdotal and machine-specific, but they rule out a
  5-point limit for cuts.
- **Raising heat late is the risky direction.** Cropster names raising the
  drum's heat during first crack as the most damaging cause of the "flick"
  (a late rise of the RoR)
  ([Cropster: the flick, 2021](https://www.cropster.com/blog-post/the-flick-updated-for-2021/)).
  The widely used heuristic of a continually declining RoR comes from Scott
  Rao's *The Coffee Roaster's Companion*; it is popular but not universal
  ([Royal Coffee](https://royalcoffee.com/?p=165841)).
- **Thermal lag dominates.** Roasters anticipate: changes act on the bean
  temperature with a delay of tens of seconds, so a few deliberate moves at
  the right time beat many small corrections.

## Control research

- Model-predictive control with a digital twin held roaster temperature
  within about 2 % of the set point under a disturbance, against about 5 %
  for PID-type control
  ([Pierl et al., ASIC 2023 poster](https://www.alphavisa.com/asic/2023/documents/eposters/S7/S7_Dennis_Pierl.pdf)).
- A hybrid first-principles and empirical model predicts bean temperature
  from roaster inputs well enough for non-linear predictive control
  ([de Vaal, University of Pretoria](https://repository.up.ac.za/items/ca823592-c1fa-4cdc-8b7a-983a649055df)).
- Probat describes mathematical models as the basis of future roast control
  ([Probat](https://www.probat.com/newsroom/news/how-mathematical-models-are-shaping-the-future-of-roasting/)).

## Consequences for RoastMCP

1. AI limits bound only moves away from the safe state (raising the burner,
   lowering the air); cuts and more air are free.
2. The autopilot (milestone M5) controls toward a rate-of-rise target with a
   model of the machine's lag, rather than reacting step by step; the AI sets
   targets, not individual heater values.
3. Humans are never step-limited.
