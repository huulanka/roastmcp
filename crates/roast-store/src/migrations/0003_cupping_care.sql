-- Schema version 3: cuppings (tastings of a roast) and maintenance (the
-- recurring tasks and the log of when each was done).
--
-- A cupping belongs to its roast and a log entry to its task: they leave
-- the database with them.

CREATE TABLE cuppings (
    id        TEXT PRIMARY KEY,
    roast_id  TEXT NOT NULL REFERENCES roasts (id) ON DELETE CASCADE,
    cupped_at TEXT NOT NULL,
    cupping   TEXT NOT NULL CHECK (json_valid(cupping))
) STRICT;

CREATE INDEX cuppings_by_roast ON cuppings (roast_id, cupped_at);

CREATE TABLE maintenance_tasks (
    id       TEXT PRIMARY KEY,
    pos      INTEGER NOT NULL,
    name     TEXT NOT NULL,
    archived INTEGER NOT NULL CHECK (archived IN (0, 1)),
    task     TEXT NOT NULL CHECK (json_valid(task))
) STRICT;

CREATE TABLE maintenance_log (
    id      TEXT PRIMARY KEY,
    task_id TEXT NOT NULL REFERENCES maintenance_tasks (id) ON DELETE CASCADE,
    done_at TEXT NOT NULL,
    entry   TEXT NOT NULL CHECK (json_valid(entry))
) STRICT;

CREATE INDEX maintenance_log_by_task ON maintenance_log (task_id, done_at);
