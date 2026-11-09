-- 0086: Plane epics become boards (Arugula #483, #371). A tracker's work item
-- says whether it is an epic (`forge_items.is_epic`, worked out from its
-- type when the row is written, from Plane's own `is_epic` flag on the
-- type). A connection has a switch, `epics_as_boards`, on by default: while
-- it is on, each open epic has a board and the board follows the epic.
--
-- A board made from an epic names it: `source_item` is the epic's `fi_` id
-- (one board per epic in a project), and `source_key`, `source_url` and
-- `source_name` are what the epic and its connection were called when the
-- board was made, kept so they read after the epic is gone from Plane.
-- `source_left_out` counts the children a full board left out. NULL: a board
-- a person made. A row the sync added has `board_entries.source` `plane`;
-- NULL: a row a person (or their agent) added, which the sync never removes.
--
-- Additive: the build before this one ignores the columns.
ALTER TABLE forge_items ADD COLUMN is_epic INTEGER NOT NULL DEFAULT 0;
ALTER TABLE tracker_connections ADD COLUMN epics_as_boards INTEGER NOT NULL DEFAULT 1;
ALTER TABLE boards ADD COLUMN source_item TEXT;
ALTER TABLE boards ADD COLUMN source_key TEXT;
ALTER TABLE boards ADD COLUMN source_url TEXT;
ALTER TABLE boards ADD COLUMN source_name TEXT;
ALTER TABLE boards ADD COLUMN source_left_out INTEGER NOT NULL DEFAULT 0;
ALTER TABLE board_entries ADD COLUMN source TEXT;
CREATE UNIQUE INDEX boards_source_item ON boards (project_id, source_item) WHERE source_item IS NOT NULL;
CREATE INDEX forge_items_parent ON forge_items (connection_id, parent_source_id) WHERE parent_source_id IS NOT NULL;
