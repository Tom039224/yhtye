-- Stage 8e: a chat is bound to a worktree, not a branch (`core-design.md` §17.8).
--
-- Existing data is discarded, as in 0006 (Stage 8 was not released): the event
-- log of Stage 8-8d has `chat_created` with a branch and `chat_branch_changed`,
-- which the new types cannot read. Projects, the agent settings and the secret
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

-- `branch` becomes `worktree` (the path as `git worktree list` shows it).
DROP TABLE chats;
CREATE TABLE chats (
    project_id    TEXT NOT NULL REFERENCES projects(id),
    id            TEXT NOT NULL,
    ord           INTEGER NOT NULL,
    worktree      TEXT NOT NULL,
    title         TEXT,
    created_ms    INTEGER NOT NULL,
    last_used_ms  INTEGER NOT NULL,
    PRIMARY KEY (project_id, id)
);
