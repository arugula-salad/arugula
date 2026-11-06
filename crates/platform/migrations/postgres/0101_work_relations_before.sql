-- 0101: a relation kind `before` (Arugula #761). See SQLite's file.
ALTER TABLE work_relations DROP CONSTRAINT work_relations_kind_check;
ALTER TABLE work_relations ADD CONSTRAINT work_relations_kind_check
    CHECK (kind IN ('blocks', 'relates', 'duplicate_of', 'discovered_from', 'before'));
