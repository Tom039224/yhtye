-- Stage 7b: which harness × model each role uses (`core-design.md` §15).

-- Per role: candidates and default (JSON `RoleSettings`). scope '' = global,
-- otherwise a project id. No row = inherited.
CREATE TABLE agent_settings (
    scope       TEXT NOT NULL,
    role        TEXT NOT NULL,
    settings    TEXT NOT NULL,
    updated_ms  INTEGER NOT NULL,
    PRIMARY KEY (scope, role)
);

-- The orchestrator's create_task override (JSON `AgentChoice`, NULL = none).
ALTER TABLE tasks ADD COLUMN agent TEXT;
ALTER TABLE tasks ADD COLUMN review_agent TEXT;

-- What each session actually ran (restored sessions keep it).
ALTER TABLE agent_sessions ADD COLUMN harness TEXT;
ALTER TABLE agent_sessions ADD COLUMN model TEXT;
