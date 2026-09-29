-- Stage 7e: names of the secret environment variables (`core-design.md` §16).
-- The values live only in the OS credential store, never in this database.
CREATE TABLE secret_env_names (
    name        TEXT NOT NULL PRIMARY KEY,
    updated_ms  INTEGER NOT NULL
);
