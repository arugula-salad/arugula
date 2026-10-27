-- 0061: which model agent sessions use (Arugula #215). `agents_model` is a
-- project's setting, "agents use model …" (one of the session wire's
-- `MODELS`; NULL for the strongest). A session's `model` is the one its
-- ask named ("use sonnet"); NULL takes the project's. `ran_model` is the
-- model its box last said its agent runs on (`Opus 5.5`), from any report,
-- a heartbeat's too.
ALTER TABLE projects ADD COLUMN agents_model TEXT;
ALTER TABLE agent_sessions ADD COLUMN model TEXT;
ALTER TABLE agent_sessions ADD COLUMN ran_model TEXT;
