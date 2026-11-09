# ADR 0017: Profiles from milestones and a recipe, announced while roasting

## Status
Accepted (2026-11-09). Builds on the profile model in `roast-core::profile`
and on [ADR 0014](0014-green-coffee-inventory-and-roast-plan.md) (the plan of
a roast).

## Context
Until now a roast's plan was an earlier roast: its curve as the dashed line,
its drop temperature and DTR as targets. The roaster wants to develop
profiles — change where first crack should fall, how long development runs,
when the burner comes down — and keep the history of those changes. Asked
with mockups, they chose: edit **milestones** (time and temperature) in a table
or on the curve, a **recipe** of burner and air steps beside it, every change
a new version; profiles get their own top tab; during a roast the next recipe
step is announced, and the person sets it.

## Decision
- **Storage:** `profiles.json` in the data folder (atomic writes, previous file
  as `.bak`, like the inventory) with `Profile` and immutable `ProfileVersion`s
  (`roast-core`). A new version records its parent and a change note.
- **A profile starts from a roast** the roaster liked (or from a template of
  a 200 g espresso on the Cubean+): its curve from CHARGE to DROP, smoothed
  and resampled every 2 s, its dry end, first crack and drop as milestones, and
  its burner and air moves as the recipe (moves less than 8 s apart are one
  step — Artisan logs a slider move as a short ramp — in whole percent).
- **Moving a milestone warps the curve** instead of redrawing it: time is
  stretched linearly between neighbouring milestones (CHARGE, turning point,
  dry end, first crack, drop) and the temperature shifted by an offset that
  changes linearly between them. The curve keeps the natural shape of the
  roast it came from; the rate of rise is recomputed (20 s least squares, as
  live). The app computes the curve; the daemon stores and checks it (times
  rising from 0, temperatures 0–300 °C, recipe values 0–100 %).
- **Plan:** the start dialog offers profiles (newest version) besides roasts.
  `RoastPlan.profile_version_id` makes the version's curve the plan line (the
  daemon builds a reference curve from it, with dry end, first crack and drop
  as events) and fills drop temperature and DTR from it; the roast records
  which version it followed.
- **Recipe cues:** during the roast, at the top of the controls, the next step
  whose values the machine does not have yet, with a countdown, a beep ten
  seconds before, and "Stellen", which sets it as the person's command. A step
  stays due for 30 s; one already set counts as done. Nothing moves on its own
  — that is the autopilot's job later (M7), under ADR 0004 and 0015.

## Consequences
- Profiles are versioned, so a roast always points at exactly the plan it
  followed, and versions can be compared later.
- Dragging milestones on the curve (as well as the table) and temperature
  triggers in the recipe ("air to 30 % at 160 °C", already in the model) come
  next.
- AI clients can list profiles and start a roast with one. Since ADR 0018's
  build they also create profiles and versions (`create_profile`,
  `add_profile_version`): the warp lives in `roast-core::design` as well,
  so a plan an AI writes looks like one made in the app.
