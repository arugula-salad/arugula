-- 0065: a board row may point at a link (boards, evidence on PROOFS rows,
-- Arugula #191). `target_kind` gains `link`, and `target` holds the URL.
-- Additive: see the SQLite file.
ALTER TABLE board_entries DROP CONSTRAINT board_entries_target_kind_check;
ALTER TABLE board_entries ADD CONSTRAINT board_entries_target_kind_check
    CHECK (target_kind IN ('forge_item', 'issue', 'run', 'workspace', 'block', 'link'));
