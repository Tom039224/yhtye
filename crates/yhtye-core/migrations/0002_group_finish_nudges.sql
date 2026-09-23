-- Stage 7a: reminders sent to the orchestrator for a settled group it left open
-- (`orchestration-model.md` §3). Existing groups start at 0.
ALTER TABLE task_groups ADD COLUMN finish_nudges INTEGER NOT NULL DEFAULT 0;
