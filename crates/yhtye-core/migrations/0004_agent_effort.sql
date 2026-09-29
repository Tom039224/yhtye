-- Stage 7d: the effort (thought level) each session ran with (`core-design.md` §15).
-- Agent settings and the tasks' `agent` / `review_agent` are JSON: the new
-- `effort` and candidate `note` fields read as NULL / empty in older rows
-- (serde defaults), so no rewrite is needed.
ALTER TABLE agent_sessions ADD COLUMN effort TEXT;
