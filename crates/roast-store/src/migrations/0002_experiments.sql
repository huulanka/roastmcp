-- Schema version 2: experiments (one deliberate change, what was expected,
-- what happened). The roasts of an experiment are listed in its JSON; a
-- roast names its experiment in its own JSON.

CREATE TABLE experiments (
    id         TEXT PRIMARY KEY,
    created_at TEXT NOT NULL,
    status     TEXT NOT NULL,
    experiment TEXT NOT NULL CHECK (json_valid(experiment))
) STRICT;

CREATE INDEX experiments_by_time ON experiments (created_at);
