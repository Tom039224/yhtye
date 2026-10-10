-- The ACP session a start meant to restore (`session/load`) but could not, so a
-- new session took its place and `acp_session_id` was overwritten. Kept so the
-- lost session can still be restored later (by hand: `session/load` of this id).
-- NULL = no restore was ever given up; older rows read as NULL.
ALTER TABLE agent_sessions ADD COLUMN lost_acp_session_id TEXT;
