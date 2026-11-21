-- What the user chose for a harness besides the path of its executable
-- (`core-design.md` §15): the version of the ACP adapter Claude Code and Codex
-- run through, and whether Claude Code's adapter drives the user's `claude`.
-- No row = the defaults (the verified adapter, the user's `claude`).
CREATE TABLE harness_options (
    harness         TEXT NOT NULL PRIMARY KEY,
    adapter         TEXT NOT NULL DEFAULT 'verified',
    adapter_version TEXT,
    claude_source   TEXT NOT NULL DEFAULT 'user',
    updated_ms      INTEGER NOT NULL
);
