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
