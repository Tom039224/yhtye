-- Stage 3b: event log + current-state tables (core-design.md §6).
--
-- `events` is append-only. The other tables are the current state of each
-- project's domain `State`, written in the same transaction as the domain events
-- that produced it; on startup the state is read back from them.
-- Enum columns hold the snake_case names used in the JSON API.

CREATE TABLE projects (
    id          TEXT PRIMARY KEY NOT NULL,
    path        TEXT NOT NULL,
    config      TEXT NOT NULL,      -- DomainConfig (JSON)
    counters    TEXT NOT NULL,      -- Counters (JSON)
    created_ms  INTEGER NOT NULL
);

-- Every durable ApiEvent of a project. `seq` is ApiEvent.seq (gapless per project).
CREATE TABLE events (
    project_id  TEXT NOT NULL REFERENCES projects(id),
    seq         INTEGER NOT NULL,
    ts_ms       INTEGER NOT NULL,
    kind        TEXT NOT NULL,      -- e.g. domain.task_created, agent.turn_ended, agent_text
    session     TEXT,               -- Yhtye session key for session-scoped events
    payload     TEXT NOT NULL,      -- ApiEventBody (JSON)
    PRIMARY KEY (project_id, seq)
) WITHOUT ROWID;
CREATE INDEX events_by_session ON events (project_id, session, seq);
CREATE INDEX events_by_kind ON events (project_id, kind, seq);

-- `groups` is an SQL keyword (window frames), hence `task_groups`.
CREATE TABLE task_groups (
    project_id      TEXT NOT NULL REFERENCES projects(id),
    id              TEXT NOT NULL,
    ord             INTEGER NOT NULL,
    title           TEXT NOT NULL,
    summary         TEXT,
    base_branch     TEXT NOT NULL,
    group_branch    TEXT NOT NULL,
    status          TEXT NOT NULL,
    finish_summary  TEXT,
    detail          TEXT,
    PRIMARY KEY (project_id, id)
);

CREATE TABLE tasks (
    project_id      TEXT NOT NULL REFERENCES projects(id),
    id              TEXT NOT NULL,
    ord             INTEGER NOT NULL,
    group_id        TEXT NOT NULL,
    title           TEXT NOT NULL,
    kind            TEXT NOT NULL,
    instruction     TEXT,
    current_step    INTEGER NOT NULL,
    status          TEXT NOT NULL,
    review_rounds   INTEGER NOT NULL,
    workdir         TEXT,
    cancel_reason   TEXT,
    PRIMARY KEY (project_id, id)
);

CREATE TABLE task_deps (
    project_id  TEXT NOT NULL REFERENCES projects(id),
    task_id     TEXT NOT NULL,
    ord         INTEGER NOT NULL,
    depends_on  TEXT NOT NULL,
    PRIMARY KEY (project_id, task_id, ord)
);

CREATE TABLE steps (
    project_id  TEXT NOT NULL REFERENCES projects(id),
    task_id     TEXT NOT NULL,
    idx         INTEGER NOT NULL,
    kind        TEXT NOT NULL,
    instruction TEXT,
    status      TEXT NOT NULL,
    result      TEXT,
    verdict     TEXT,
    note        TEXT,
    nudges      INTEGER NOT NULL,
    PRIMARY KEY (project_id, task_id, idx)
);

CREATE TABLE helps (
    project_id  TEXT NOT NULL REFERENCES projects(id),
    id          TEXT NOT NULL,
    ord         INTEGER NOT NULL,
    task_id     TEXT NOT NULL,
    step        INTEGER NOT NULL,
    kind        TEXT NOT NULL,
    message     TEXT NOT NULL,
    source      TEXT NOT NULL,      -- HelpSource (JSON)
    state       TEXT NOT NULL,
    agent_lost  INTEGER NOT NULL,
    reply       TEXT,
    PRIMARY KEY (project_id, id)
);

-- Undelivered orchestrator inbox entries.
CREATE TABLE inbox (
    project_id  TEXT NOT NULL REFERENCES projects(id),
    id          INTEGER NOT NULL,
    kind        TEXT NOT NULL,
    attrs       TEXT NOT NULL,      -- [[key, value], ...] (JSON)
    body        TEXT NOT NULL,
    PRIMARY KEY (project_id, id)
);

-- Agent sessions (runtime state, outside the domain): the ACP session id is kept
-- so a session can be restored with `session/load` after a restart. MCP tokens
-- are not stored; a restored session gets a fresh token.
CREATE TABLE agent_sessions (
    project_id      TEXT NOT NULL REFERENCES projects(id),
    session_key     TEXT NOT NULL,
    role            TEXT NOT NULL,
    task_id         TEXT,
    acp_session_id  TEXT NOT NULL,
    -- live | stopped (by Yhtye or exited) | suspended (Yhtye shut down) | interrupted (seen live/suspended on restart)
    status          TEXT NOT NULL,
    turn_running    INTEGER NOT NULL,
    updated_ms      INTEGER NOT NULL,
    PRIMARY KEY (project_id, session_key)
);
