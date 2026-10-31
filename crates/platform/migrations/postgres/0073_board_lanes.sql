-- 0073: boards, one row is one piece of work (Arugula #365, aru-125). A
-- row's lane follows the live state of its items and is never stored, so
-- nobody places a row in a lane. What a board's author sets is its groups:
-- named, ordered sections (phases, waves, areas), kept as `board_entries`
-- of `kind` `zone` with `zone_kind` `group`. A row's `zone_id` is its group,
-- or NULL for none. `db::boards_lanes` (`Db::lanes_move`) runs once, on
-- start: zones that were lanes in all but name (Next, Doing, Done, PROOFS...)
-- go, and every other zone becomes a group.
--
-- Work and proof are attached to a row in `board_attachments`: `role`
-- `work` is a PR (a `forge_item`), an agent `block` or a `run`; `proof` is a
-- `run` or a `link`. The pointer's columns are a row's own (`target_kind`,
-- `target`, and `pane` for a block); `agent` is the machine whose agent
-- attached it, NULL for a person. Times are milliseconds since the epoch.
--
-- Not additive: the build before this one reads a row without a zone as no
-- row at all. The row-needs-a-zone check is the table's first, and the
-- `zone_kind` check the one that says `proofs`, each found by what it says
-- rather than by the name Postgres gave it.
DO $$
DECLARE name TEXT;
BEGIN
    FOR name IN
        SELECT conname FROM pg_constraint
        WHERE conrelid = 'board_entries'::regclass AND contype = 'c'
          AND (pg_get_constraintdef(oid) LIKE '%zone_id IS NOT NULL%'
               OR pg_get_constraintdef(oid) LIKE '%proofs%')
    LOOP
        EXECUTE format('ALTER TABLE board_entries DROP CONSTRAINT %I', name);
    END LOOP;
END
$$;
ALTER TABLE board_entries ADD CONSTRAINT board_entries_zone_only_check CHECK (kind = 'row' OR zone_id IS NULL);
ALTER TABLE board_entries ADD CONSTRAINT board_entries_zone_kind_check CHECK (zone_kind IN ('proofs', 'group'));

CREATE TABLE board_attachments (
    id TEXT COLLATE pg_c_utf8 PRIMARY KEY,
    board_id TEXT COLLATE pg_c_utf8 NOT NULL,
    row_id TEXT COLLATE pg_c_utf8 NOT NULL,
    role TEXT COLLATE pg_c_utf8 NOT NULL CHECK (role IN ('work', 'proof')),
    target_kind TEXT COLLATE pg_c_utf8 NOT NULL CHECK (target_kind IN ('forge_item', 'run', 'block', 'link')),
    target TEXT COLLATE pg_c_utf8 NOT NULL,
    pane TEXT COLLATE pg_c_utf8,
    agent TEXT COLLATE pg_c_utf8,
    created_at BIGINT NOT NULL,
    CHECK ((target_kind = 'block') = (pane IS NOT NULL))
);
CREATE INDEX board_attachments_board ON board_attachments (board_id, created_at, id);
CREATE INDEX board_attachments_row ON board_attachments (row_id);
