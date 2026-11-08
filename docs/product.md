# Product scope

## What RoastMCP is for

One open, modern tool for everything around roasting coffee at home or in
small batches: running the roast, keeping every roast, managing green coffee,
developing profiles and tasting the result — with an AI that can read all of
it, learn from it and, under supervision, take the controls.

It replaces Artisan for its author on day one of 1.0 and is meant to be just
as useful to anyone else with a supported roaster.

## Principles

1. **A person supervises every roast.** Manual input always wins. AI control is
   bounded, leased and needs regular confirmation of presence
   ([safety](safety.md)).
2. **Your data is yours.** Local database, one open file per roast, automatic
   backups, full export. Nothing leaves the machine unless you send it.
3. **Open formats and open source.** Apache-2.0. Reads and writes Artisan
   `.alog`; the native roast file is documented JSON.
4. **AI is a first-class interface, not a requirement.** Everything works
   without AI; with AI, everything is reachable through MCP.
5. **Good for beginners, deep for experts.** Guided where it helps, out of the
   way where it does not.
6. **Generic, not tied to one roaster.** Drivers and machine profiles; a
   simulator for everyone without hardware.

## Who it is for

- **The beginner** has a small roaster, a bag of green coffee and no idea when
  to drop. Needs: a setup that works, guidance during the roast, plain-language
  feedback afterwards, reminders (rest time, cleaning), safety.
- **The profile developer** roasts small batches deliberately, changes one
  variable at a time and tastes systematically. Needs: reference curves with
  live deviation, versions and experiments, comparisons, consistency numbers,
  roast degree and ambient data, clean data to analyse.
- **The AI** (through MCP) needs the same data as the profile developer, plus
  safe, well-defined ways to act.

## Scope of 1.0

### Roasting
- Live bean temperature (and environment temperature where the machine has
  one), rate of rise with robust smoothing, phase display (drying, Maillard,
  development) and development time ratio.
- Events: CHARGE, turning point, DRY END, FC start/end, SC start/end, DROP,
  COOL END — detected automatically where possible, always markable by hand.
- Controls for burner, air, drum and machine extras (cooling, filter fan):
  sliders, step buttons, configurable keyboard shortcuts.
- Alarms by time, temperature or rate of rise; sound cues and on-screen cues.
- Reference curve (background profile) with live ΔBT/ΔRoR and projection.
- First-crack detection from a microphone, shown visually; first-crack
  prediction.
- Safety layer: limits, rate limits, emergency stop, overheat guard, AI lease,
  presence confirmation, audit log.

### AI
- MCP server: status, curves, archive, profiles, inventory and cupping data;
  event marking; actuator control; emergency stop; autopilot.
- Autopilot with guardrails: the AI plans and adjusts targets, a deterministic
  controller executes them every second.
- Post-roast debrief and next-roast suggestions (via the MCP client).

### Profiles and analysis
- Profiles with version history; experiments (hypothesis, changed variable,
  outcome) attached to roasts.
- Overlay and compare roasts aligned on CHARGE or FC; consistency statistics
  per bean or profile.
- Roast degree (Agtron/Lighttells, entered by hand) and ambient conditions
  (entered by hand) per roast.

### Digital twin
- A physical thermal model of the machine, calibrated from recorded roasts,
  used for prediction, what-if runs, autopilot testing and training.
- A 3D view of the drum (rotation, bean colour, heat and airflow) for the
  simulator and for live roasts.

### Management
- Roast archive with search; import of Artisan `.alog` files.
- Green coffee inventory: lots with origin, variety, process, altitude, harvest,
  supplier, price, moisture, density, screen size, purchase date and stock;
  stock is debited automatically by each roast.
- Cupping: structured scores and descriptors per roast.
- Rest-time reminders ("ready to drink from…", "cup on…").
- Maintenance log and reminders (chaff, drum, probe) and a pre-roast safety
  checklist.

### Onboarding
- Setup wizard: choose or import a machine profile (`.aset`), connect, test,
  or start the simulator.
- Guided roasting with starter profiles per process (washed, natural, honey).
- Glossary and contextual explanations.

### Platform and data
- macOS (official), Windows and Linux (preview).
- Local SQLite database, open JSON file per roast, `.alog` export, automatic
  versioned backups to a chosen folder (for example a NAS), restore that has
  been tested.
- English and German.

## After 1.0, in this order

1. **In-app AI assistant** — chat and autopilot inside the app, with an API key
   or a local model, using the same tools and guardrails as MCP.
2. Then, in an order still to be decided:
   - **Enjoyment** — brewing log linked to roasts; blind cupping sessions with
     sample codes following the SCA Coffee Value Assessment.
   - **Management** — suppliers, orders, cost per roast; blends.
   - **Community and sensors** — sharing and importing profiles, labels and
     traceability, transferring profiles to other batch sizes or machines;
     sensors for ambient conditions, roast colour and weight.

## Not in scope

- Running a commercial roastery (production planning, sales, accounting,
  multi-user, compliance).
- A cloud service or account system.
- A mobile app (the desktop app is the roaster's companion).
- Unsupervised roasting, in any form.
