-- Chat attachments (docs/architecture/core-design.md §20).
-- Files the user attached to a user message: their metadata (JSON, the files
-- themselves are on disk) travels with the undelivered inbox entry.
ALTER TABLE inbox ADD COLUMN attachments TEXT NOT NULL DEFAULT '[]';

-- What each harness last said it accepts in a prompt (ACP `promptCapabilities`
-- of its `initialize` answer), written when a session of it starts. It predicts
-- how an attachment will be delivered before the orchestrator runs; the
-- delivery itself is decided with the running process's capabilities.
CREATE TABLE harness_prompt_caps (
    harness          TEXT NOT NULL PRIMARY KEY,
    image            INTEGER NOT NULL,
    embedded_context INTEGER NOT NULL,
    updated_ms       INTEGER NOT NULL
);
