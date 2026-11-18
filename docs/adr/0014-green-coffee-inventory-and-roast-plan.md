# ADR 0014: Green coffee inventory and the roast plan, the same for app and AI

## Status
Accepted (2026-11-09). Brings the inventory forward from M4. It refines
[ADR 0012](0012-guided-roast-flow.md), which had put bean lots off.

## Context
The roaster reviewed the mockup for the guided start (ADR 0012) and decided:

- The bean is chosen from a list, not typed. Purchases are recorded (when, how
  much of which bean), and every roast debits its green weight from the stock.
- The targets that matter during a roast are the drop temperature and the
  development time ratio (DTR). A reference roast is the plan.
- Whatever the start dialog sets, an AI client must be able to set through MCP
  too ("Claude, start a roast of the Amarelo for espresso").

The domain model exists in `roast-core` (`GreenLot`, `StockMovement`; stock is
the sum of movements). There is no storage for it yet. Roasts are stored as open
JSON files, and SQLite (ADR 0003) is not built yet.

## Decision
- **Storage:** the daemon keeps lots and movements in `inventory.json` in the
  data folder. Writes are atomic (temporary file, then rename) and keep the
  previous version as `inventory.json.bak`. The inventory moves to SQLite
  together with the roasts when ADR 0003 is built; the file stays readable for
  that migration.
- **Debit:** a roast debits its green weight from its lot when it is saved after
  CHARGE. A recording stopped before CHARGE debits nothing. Changing the green
  weight of a saved roast changes its debit, so there is exactly one debit per
  roast. Stock can go negative, and the app shows that rather than refusing to
  roast.
- **The roast file** keeps `green_lot_id` (already in the format) and the lot's
  name in `batch.beans`, so files and `.alog` exports stay readable without the
  inventory. It gains an optional `plan`: the goal (espresso, filter, omni,
  other), the reference roast's id, the drop temperature and the DTR. Older
  files stay valid.
- **One set of operations for app and AI:** the HTTP API has lots (list with
  stock, create with an opening purchase, edit, archive), purchases and
  corrections, and a recording start that takes the lot, green weight, goal,
  reference and targets. The MCP server offers the same operations as tools:
  `list_lots`, `add_lot`, `add_purchase` and `start_recording` (the start
  dialog's fields). Starting a recording moves no actuator, so it needs no lease.
- **Safety:** targets are guidance. The safety layer never reads them, and
  nothing is roasted automatically because a target exists.

## The lot's details
Reviewed with the roaster (2026-11-09): a lot records what an importer's offer
sheet states, all optional except the name. Origin: country, region, producer
(farm, cooperative or washing station), altitude range, species, varieties.
Processing, kept apart because the trade combines them freely: the base method
(washed, natural, honey with its white/yellow/red/black grade, pulped natural,
semi-washed, wet-hulled), fermentation techniques (anaerobic, carbonic
maceration, thermal shock, extended or double, inoculated, infused; several at
once) with their duration, drying, decaffeination and free text. Quality:
crop year, arrival, grade, screen size, moisture, water activity, density, the
supplier's score and tasting notes. Purchase and storage: supplier, the
supplier's lot code, price per kg, packaging, storage location,
certifications, notes. The app enters them as a short wizard (name and amount
first, then one page per group, no scrolling); MCP takes the same fields.

## Consequences
- The start dialog lists the lots that are not archived, with their stock. The
  archive gains an inventory view (built after a mockup).
- Imported Artisan roasts keep their free-text bean and debit nothing.
- A purchase recorded by the AI shows up in the app at once, and the other way
  round.

## Amendment (2026-11-18): when the two targets disagree
In BRYB-003 the plan asked for 203 °C and a DTR of 19 %. With too much energy
after first crack the bean temperature reached 203 °C at a DTR of about 14 %;
19 % would have come only at about 208 °C. The daemon did not use the DTR at
all; the cooling followed the temperature alone.

From first crack on the daemon now works out both: when the bean temperature
reaches the drop temperature (projected from the rate of rise and its trend)
and when the DTR reaches its target (`first crack / (1 − DTR)`). When they lie
more than 20 s apart it warns once ("Plan targets disagree: 203 °C at 9:45
(DTR 12 %), DTR 19 % at 10:33 (about 208 °C). DROP follows the earlier.").
The plan's `drop_by` says which ends the roast: `earlier` (the default: never
darker than planned), `later`, `bt` or `dtr`. The bean cooling starts 30 s
before that moment. The start dialog and the MCP tool `start_recording` offer
the choice; the snapshot's `drop` carries both times.

The first-crack expectation (ADR 0021 §7) changed the same day: it is the
**median** of the first cracks a person (or an AI) marked in earlier roasts of
the lot (marks only the microphone set count only when there are no others),
and the window narrows to ± 10 °C around one such mark and ± 8 °C around two
or more. Statistics over several roasts (consistency) leave out first crack,
development and DTR of roasts whose first crack only the microphone marked.

