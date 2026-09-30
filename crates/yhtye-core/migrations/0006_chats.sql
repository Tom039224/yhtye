-- Stage 8: chats and branches (`core-design.md` §17).
--
-- Existing data is discarded (user decision): the old single-orchestrator
-- history has no chat to belong to. Projects, the agent settings and the secret
-- env names stay.
DELETE FROM task_deps;
DELETE FROM steps;
DELETE FROM helps;
DELETE FROM tasks;
DELETE FROM task_groups;
DELETE FROM inbox;
DELETE FROM agent_sessions;
DELETE FROM events;
UPDATE projects SET counters = '{"chats":0,"groups":0,"tasks":0,"helps":0,"inbox":0}';

-- One row per chat. `created_ms` / `last_used_ms` are derived from the event log
-- (`chat_created`, and the last `prompted` of the chat's session).
CREATE TABLE chats (
    project_id    TEXT NOT NULL REFERENCES projects(id),
    id            TEXT NOT NULL,
    ord           INTEGER NOT NULL,
    branch        TEXT NOT NULL,
    title         TEXT,
    created_ms    INTEGER NOT NULL,
    last_used_ms  INTEGER NOT NULL,
    PRIMARY KEY (project_id, id)
);

-- SQLite needs a default to add a NOT NULL column; the tables are empty now.
ALTER TABLE task_groups ADD COLUMN chat_id TEXT NOT NULL DEFAULT '';
ALTER TABLE inbox ADD COLUMN chat_id TEXT NOT NULL DEFAULT '';

-- The working directory a session was started in: a stored session is only
-- restored (`session/load`) when it would start in the same directory again.
ALTER TABLE agent_sessions ADD COLUMN cwd TEXT;
