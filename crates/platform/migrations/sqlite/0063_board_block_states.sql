-- 0060: what a machine says of its own agent blocks (boards, step 4), for
-- the board rows that point at them. One row per (machine, pane), set by
-- the machine under its `boards.write` grant and read only for a board of a
-- project that grant covers. It holds the state word (working, needs_input,
-- idle, closed), the branch, the short HEAD, whether it is pushed, how many
-- files are uncommitted and the diffstat as three numbers; never message
-- text, file names or a transcript (aru-116). `account` is the machine's
-- owner. Times are milliseconds since the epoch.
CREATE TABLE board_block_states (
    machine TEXT NOT NULL,
    pane TEXT NOT NULL,
    account TEXT NOT NULL,
    state TEXT NOT NULL CHECK (state IN ('working', 'needs_input', 'idle', 'closed')),
    branch TEXT,
    head TEXT,
    pushed INTEGER,
    uncommitted INTEGER,
    diff_files INTEGER,
    diff_added INTEGER,
    diff_removed INTEGER,
    updated_at INTEGER NOT NULL,
    PRIMARY KEY (machine, pane)
);
CREATE INDEX board_block_states_account ON board_block_states (account);
