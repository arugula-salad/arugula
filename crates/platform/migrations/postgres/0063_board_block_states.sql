-- 0060: what a machine says of its own agent blocks (boards, step 4), for
-- the board rows that point at them. One row per (machine, pane), set by
-- the machine under its `boards.write` grant and read only for a board of a
-- project that grant covers. It holds the state word (working, needs_input,
-- idle, closed), the branch, the short HEAD, whether it is pushed, how many
-- files are uncommitted and the diffstat as three numbers; never message
-- text, file names or a transcript (aru-116). `account` is the machine's
-- owner. Times are milliseconds since the epoch.
--
-- SQLite's file in Postgres's words: `BIGINT` for its integers and
-- `COLLATE pg_c_utf8` text.
CREATE TABLE board_block_states (
    machine TEXT COLLATE pg_c_utf8 NOT NULL,
    pane TEXT COLLATE pg_c_utf8 NOT NULL,
    account TEXT COLLATE pg_c_utf8 NOT NULL,
    state TEXT COLLATE pg_c_utf8 NOT NULL CHECK (state IN ('working', 'needs_input', 'idle', 'closed')),
    branch TEXT COLLATE pg_c_utf8,
    head TEXT COLLATE pg_c_utf8,
    pushed BIGINT,
    uncommitted BIGINT,
    diff_files BIGINT,
    diff_added BIGINT,
    diff_removed BIGINT,
    updated_at BIGINT NOT NULL,
    PRIMARY KEY (machine, pane)
);
CREATE INDEX board_block_states_account ON board_block_states (account);
