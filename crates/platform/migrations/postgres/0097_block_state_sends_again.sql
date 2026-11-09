-- 0097: a block's report can go stale (Arugula #745). See SQLite's file.
ALTER TABLE board_block_states ADD COLUMN sends_again BIGINT NOT NULL DEFAULT 0;
