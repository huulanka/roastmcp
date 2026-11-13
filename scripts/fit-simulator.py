#!/usr/bin/env python3
"""Fits the simulator's heat model to recorded roasts, or checks the fit.

The model is the one in `crates/roast-driver/src/simulator.rs`; the method and
the results are in `docs/research/simulator-calibration.md`.

    uv run --with numpy --with scipy scripts/fit-simulator.py ROASTS_DIR NAME...
    uv run --with numpy --with scipy scripts/fit-simulator.py --fit ROASTS_DIR NAME...

ROASTS_DIR holds `.roast.json` files (the app's `roasts` folder); each NAME is
the start of one file name. Without `--fit` it prints how well the constants
below (the ones in the simulator) reproduce those roasts. The roasts are only
read. They are personal logs: do not commit them.
"""

import glob
import json
import math
import sys

import numpy as np
from scipy.optimize import minimize

AMBIENT_C = 22.0
HEATER_W = 1700.0
LOSS_EXPONENT = 1.8
LATENT_J_PER_KG = 2.26e6
STEP_S = 0.5

# The constants in the simulator, in the order of `simulate`'s parameters.
NAMES = [
    "LOSS_W", "AIR_LOSS", "DRUM_J_PER_K", "BEAN_TRANSFER_W_PER_K", "PROBE_BEAN_SHARE",
    "PROBE_TAU_S", "EVAPORATION_W_PER_K", "EXOTHERM_W", "BEAN_J_PER_KG_K", "AIR_TRANSFER",
]
SHIPPED = np.array([0.04766, 1.04, 3431.0, 6.967, 0.9396, 48.04, 2.429, 13.67, 6493.0, 1.602])

# What the empty Cubean+ does at air 20 % (measured 2026-11-09): the burner
# that holds 200 °C and 150 °C, and the rise at 90 % burner around 90 °C.
HOLD_200_PCT, HOLD_150_PCT, RISE_C_PER_MIN = 38.0, 21.0, 25.0


def load(folder, name):
    path = glob.glob(f"{folder}/{name}*.roast.json")[0]
    roast = json.load(open(path))
    events = {e["kind"]: e["t"] for e in roast["events"]}
    charge, drop = events["charge"], events["drop"]
    t, bt, burner, air = [], [], [], []
    last = {"burner": 0.0, "air": 20.0}
    for s in roast["samples"]:
        for k, v in (s.get("actuators") or {}).items():
            if k in last and v is not None:
                last[k] = v
        if charge <= s["t"] <= drop and s.get("bt") is not None:
            t.append(s["t"] - charge)
            bt.append(s["bt"])
            burner.append(last["burner"])
            air.append(last["air"])
    kg = (roast["batch"].get("green_weight_g") or 200) / 1000
    return dict(name=name, t=np.array(t), bt=np.array(bt), burner=np.array(burner), air=np.array(air), kg=kg)


def simulate(p, r):
    """The probe temperature at the roast's sample times, from its burner and air."""
    loss_w, air_loss, drum_j, transfer, share, tau, evap_w, exo_w, bean_j, air_transfer = p
    drum = probe = r["bt"][0]
    bean = AMBIENT_C
    moisture = r["kg"] * 0.10
    out = np.empty(len(r["t"]))
    j, now, n = 0, 0.0, len(r["t"])
    while j < n:
        while j < n and r["t"][j] <= now:
            out[j] = probe
            j += 1
        if j >= n:
            break
        i = max(j - 1, 0)
        burner, air = r["burner"][i] / 100, r["air"][i] / 100
        loss = loss_w * (1 + air_loss * air) * max(drum - AMBIENT_C, 0) ** LOSS_EXPONENT
        to_beans = transfer * (1 + air_transfer * air) * (drum - bean)
        drum += (HEATER_W * burner - loss - to_beans) * STEP_S / drum_j
        evap = evap_w * min(max(bean - 95, 0), 60) if moisture > 0 else 0.0
        exo = exo_w if 190 <= bean < 225 else 0.0
        moisture = max(moisture - evap * STEP_S / LATENT_J_PER_KG, 0)
        bean += (to_beans - evap + exo) * STEP_S / (r["kg"] * bean_j)
        probe += (share * bean + (1 - share) * drum - probe) * STEP_S / tau
        now += STEP_S
    return out


def empty_drum(p):
    """Burner that holds 200 °C and 150 °C, and the rise at 90 %, at air 20 %."""
    loss = p[0] * (1 + p[1] * 0.2)
    hold = lambda c: loss * (c - AMBIENT_C) ** LOSS_EXPONENT / HEATER_W * 100
    rise = (HEATER_W * 0.9 - loss * 68**LOSS_EXPONENT) / p[2] * 60
    return hold(200), hold(150), rise


def cost(p, roasts, report=False):
    if min(p[0], p[2], p[3], p[5], p[8]) <= 0 or not 0.3 <= p[4] <= 1.0 or min(p[1], p[6], p[7], p[9]) < 0:
        return 1e9
    total = 0.0
    for r in roasts:
        sim = simulate(p, r)
        rms = math.sqrt(np.mean((sim - r["bt"]) ** 2))
        total += rms**2
        if report:
            print(
                f"{r['name']}: RMS {rms:.1f} °C, DROP {sim[-1]:.1f} °C (real {r['bt'][-1]:.1f}), "
                f"turning point {sim.min():.0f} °C at {r['t'][sim.argmin()]:.0f} s "
                f"(real {r['bt'].min():.0f} °C at {r['t'][r['bt'].argmin()]:.0f} s)"
            )
    h200, h150, rise = empty_drum(p)
    total += 4 * ((h200 - HOLD_200_PCT) / 2) ** 2 + 4 * ((h150 - HOLD_150_PCT) / 2) ** 2
    total += 4 * ((rise - RISE_C_PER_MIN) / 3) ** 2
    if report:
        print(f"empty drum: holds 200 °C at {h200:.1f} %, 150 °C at {h150:.1f} %, rises {rise:.1f} °C/min at 90 %")
    return total


def main():
    args = sys.argv[1:]
    fit = "--fit" in args
    args = [a for a in args if a != "--fit"]
    if len(args) < 2:
        sys.exit(__doc__)
    roasts = [load(args[0], name) for name in args[1:]]
    best = SHIPPED
    if fit:
        starts = [SHIPPED, np.array([0.058, 0.5, 3300.0, 6.0, 0.85, 25.0, 1.5, 40.0, 1600.0, 0.5])]
        runs = [
            minimize(cost, s, args=(roasts,), method="Nelder-Mead",
                     options=dict(maxiter=6000, xatol=1e-4, fatol=1e-4, adaptive=True))
            for s in starts
        ]
        best = min(runs, key=lambda r: r.fun).x
    for name, value in zip(NAMES, best):
        print(f"{name} = {value:.4g}")
    cost(best, roasts, report=True)


if __name__ == "__main__":
    main()
