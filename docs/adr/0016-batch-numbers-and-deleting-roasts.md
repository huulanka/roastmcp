# ADR 0016: Batch numbers per lot, and deleting roasts into a trash folder

## Status
Accepted (2026-11-09). Extends [ADR 0014](0014-green-coffee-inventory-and-roast-plan.md).

## Context
The roaster numbers every roast and writes the number on the bag's label by
hand. Their Artisan files show the scheme so far: two letters of the origin
and a running number per country (`BR001`…`BR011`, `CO001`…`CO010`,
`CR001`, `CR002`). Asked how RoastMCP should do it, they chose a counter per
bean: the number should say which coffee it is and how many times it has been
roasted, which matters when developing a profile. Their tests in the
simulator left test roasts in the archive and debited real stock.

## Decision
- **Batch code per lot:** a lot has an optional `batch_code`. The app
  suggests the origin's country code and the initials of the lot's name
  (Yellow Bourbon / Natural from Brazil → `BRYB`); the roaster can change it.
- **Batch number per roast:** `BRYB-002` — the code and the lot's running
  number, three digits. The start dialog suggests one more than the lot's real
  roasts so far (imports whose bean text names the lot included) or than the
  highest number used with the code, whichever is larger; it stays editable.
  It is stored in `batch.batch_no`, starts the file names, goes into Artisan's
  `roastbatchprefix`/`roastbatchnr` in the `.alog`, and a lot without a code
  takes the code of the first number used for it.
- **Old numbers stay:** imported Artisan roasts show the number their file
  name starts with (`BR011`); nothing is rewritten.
- **Label:** after DROP and in the archive the app offers the label's two
  lines to copy: number and bean, goal and roast date. Printing stays manual.
- **Simulator:** a simulated roast gets no batch number and debits no stock.
- **Deleting:** the archive's ⋯ menu deletes a roast after a confirmation.
  Its files move to `roasts/trash/` (restore by moving them back), its stock
  debit is taken back, and the action log records it. AI clients get no
  delete tool.

## Consequences
- The country series (`BR012` …) ends; the per-lot numbers start where the
  lot's history is (Yellow Bourbon: `BRYB-002` after `BR011`).
- Matching imports to lots by bean text is a heuristic; an import without
  bean text is not counted, and the roaster corrects the suggested number.
