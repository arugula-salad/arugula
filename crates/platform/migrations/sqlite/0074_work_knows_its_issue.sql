-- 0074: work knows its issue (Arugula #372). A machine's report of an agent
-- block may carry the name its starter gave it (`title`), for a board's
-- chip; and an agent session started on an issue records it as its
-- `work_item` (an `iss_` or `fi_` id), set at start and never changed.
-- Additive: the build before this one ignores both columns.
ALTER TABLE board_block_states ADD COLUMN title TEXT;
ALTER TABLE agent_sessions ADD COLUMN work_item TEXT;
