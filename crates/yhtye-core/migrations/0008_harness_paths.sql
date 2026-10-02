-- Manual executable paths of the harnesses (`core-design.md` §15): the path the
-- user gave for a harness's main executable, used before the PATH search.
-- No row = detect automatically.
CREATE TABLE harness_paths (
    harness     TEXT NOT NULL PRIMARY KEY,
    path        TEXT NOT NULL,
    updated_ms  INTEGER NOT NULL
);
