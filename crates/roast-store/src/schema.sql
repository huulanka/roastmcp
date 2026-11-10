-- RoastMCP database, schema version 1 (ADR 0003).
--
-- * Ids are UUIDs as lowercase hyphenated TEXT.
-- * Instants are UTC TEXT of fixed width (`2026-11-08T14:30:00.000Z`), so
--   text order is time order.
-- * Each record keeps its full roast-core value as JSON beside the columns
--   that are listed, searched or referred to; the JSON is the truth, the
--   columns are written from it on every save.
-- * `pos` keeps the order the records were created in.
-- * Tables are STRICT: SQLite enforces the declared column types.

CREATE TABLE settings (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL CHECK (json_valid(value))
) STRICT;

CREATE TABLE lots (
    id       TEXT PRIMARY KEY,
    pos      INTEGER NOT NULL,
    name     TEXT NOT NULL,
    archived INTEGER NOT NULL CHECK (archived IN (0, 1)),
    lot      TEXT NOT NULL CHECK (json_valid(lot))
) STRICT;

CREATE TABLE stock_movements (
    id       TEXT PRIMARY KEY,
    pos      INTEGER NOT NULL,
    lot_id   TEXT NOT NULL REFERENCES lots (id),
    at       TEXT NOT NULL,
    grams    REAL NOT NULL,
    movement TEXT NOT NULL CHECK (json_valid(movement))
) STRICT;

CREATE INDEX stock_movements_by_lot ON stock_movements (lot_id, at);

CREATE TABLE profiles (
    id       TEXT PRIMARY KEY,
    pos      INTEGER NOT NULL,
    name     TEXT NOT NULL,
    archived INTEGER NOT NULL CHECK (archived IN (0, 1)),
    profile  TEXT NOT NULL CHECK (json_valid(profile))
) STRICT;

CREATE TABLE profile_versions (
    id              TEXT PRIMARY KEY,
    pos             INTEGER NOT NULL,
    profile_id      TEXT NOT NULL REFERENCES profiles (id),
    version         INTEGER NOT NULL CHECK (version >= 1),
    created_at      TEXT NOT NULL,
    profile_version TEXT NOT NULL CHECK (json_valid(profile_version)),
    UNIQUE (profile_id, version)
) STRICT;

-- One row per roast. `summary` is the archive listing's line, so a listing
-- never has to read the curves; `file_name` is the roast's open
-- `.roast.json` copy in the roasts folder (ADR 0008).
CREATE TABLE roasts (
    id           TEXT PRIMARY KEY,
    roasted_at   TEXT NOT NULL,
    title        TEXT NOT NULL,
    lot_id       TEXT,
    sample_count INTEGER NOT NULL,
    file_name    TEXT,
    summary      TEXT NOT NULL CHECK (json_valid(summary)),
    roast        TEXT NOT NULL CHECK (json_valid(roast)),
    updated_at   TEXT NOT NULL
) STRICT;

CREATE INDEX roasts_by_time ON roasts (roasted_at);
CREATE INDEX roasts_by_lot ON roasts (lot_id);
