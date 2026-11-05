-- 0097: a block's report can go stale (Arugula #745).
--   - `board_block_states.sends_again`: 1 when the machine's last report
--     carried its age, which a daemon that sends each live block's state
--     again every few minutes does; such a report older than
--     `BLOCK_STATE_STALE_MS` counts as unknown for lanes and claims. 0 (a
--     daemon from before this) never goes stale, as before.
--   - `updated_at` is now when the machine saw the state (the report's
--     arrival less its age), not when it arrived.
-- Additive: the build before this one ignores the column.
ALTER TABLE board_block_states ADD COLUMN sends_again INTEGER NOT NULL DEFAULT 0;
