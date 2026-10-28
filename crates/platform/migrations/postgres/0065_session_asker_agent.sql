-- 0065: the machine whose agent started an agent session for its owner
-- (Fleet 3, aru-121: a `sessions.start` grant), so its card says "started
-- by <owner>'s agent on <machine>". The asker is still the owner, and so is
-- the bill. NULL: the asker started it themselves, as every session before.
ALTER TABLE agent_sessions ADD COLUMN asker_agent TEXT;
