-- 0040: emoji reactions on chat messages (#104). Times are milliseconds
-- since the epoch. `db::chat` has the rules.

-- One row per reaction: `account` reacted to message `seq` of `room` with
-- `emoji` (the character or sequence itself, as the picker gives it). When
-- an agent did it on its owner's behalf, `agent` is its machine's id and
-- `account` the owner, so a person's own reaction and their agent's are
-- two rows; '' for the person. Dropped with the message's content when it's
-- deleted, and with the room.
CREATE TABLE chat_reactions (
    room TEXT NOT NULL,
    seq INTEGER NOT NULL,
    emoji TEXT NOT NULL,
    account TEXT NOT NULL,
    agent TEXT NOT NULL DEFAULT '',
    at INTEGER NOT NULL,
    PRIMARY KEY (room, seq, emoji, account, agent)
);
CREATE INDEX chat_reactions_account ON chat_reactions (account);
