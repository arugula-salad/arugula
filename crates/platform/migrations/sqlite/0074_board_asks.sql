-- 0074: boards, asks (Arugula #367). A question on a row that a person
-- answers on the board. `who` is the account it is for; `asked_by` the
-- account that asked, and `asked_by_agent` the machine whose agent asked for
-- them, NULL for a person. `options` is a JSON list of 0 to 4 strings (none:
-- free text) and `recommend` is one of them. An ask is open until `answer` is
-- set; `answered_by` is the account that answered (the person it names, or
-- an owner or editor for them) and `answered_at` when. An answered ask stays
-- as the row's history. Times are milliseconds since the epoch.
--
-- A row has at most one open ask. The asks the board raises itself (a PR's
-- red checks, an agent waiting) are worked out on every read and not here.
CREATE TABLE board_asks (
    id TEXT PRIMARY KEY,
    board_id TEXT NOT NULL,
    row_id TEXT NOT NULL,
    who TEXT NOT NULL,
    question TEXT NOT NULL,
    why TEXT NOT NULL DEFAULT '',
    options TEXT NOT NULL DEFAULT '[]',
    recommend TEXT,
    asked_by TEXT NOT NULL,
    asked_by_agent TEXT,
    answer TEXT,
    answered_by TEXT,
    answered_at INTEGER,
    created_at INTEGER NOT NULL,
    CHECK ((answer IS NULL) = (answered_at IS NULL))
);
CREATE INDEX board_asks_board ON board_asks (board_id, created_at, id);
CREATE INDEX board_asks_who ON board_asks (who) WHERE answer IS NULL;
CREATE UNIQUE INDEX board_asks_open ON board_asks (row_id) WHERE answer IS NULL;
