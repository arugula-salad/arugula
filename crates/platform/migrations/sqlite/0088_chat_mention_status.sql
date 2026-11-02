-- 0087: where an @machine mention in chat got to (Arugula #502). Times are
-- milliseconds since the epoch. `db::chat_mention_status` has the rules.

-- One row per (message, machine mentioned): what the machine's daemon said
-- when control told it (`state`: delivered, fell_back, waiting, offline),
-- the session it went to and the one the message named that wasn't there,
-- and once the machine's agent replies in the thread, `answered` and the
-- reply's number. `root` is the message's thread (its parent, or itself),
-- which is how a reply finds the mentions it answers. Dropped with the
-- message's content and with the room.
CREATE TABLE chat_mention_status (
    room TEXT NOT NULL,
    seq INTEGER NOT NULL,
    machine TEXT NOT NULL,
    machine_name TEXT NOT NULL DEFAULT '',
    root INTEGER NOT NULL,
    state TEXT NOT NULL,
    session TEXT,
    asked TEXT,
    reply INTEGER,
    at INTEGER NOT NULL,
    PRIMARY KEY (room, seq, machine)
);
CREATE INDEX chat_mention_status_root ON chat_mention_status (room, root, state);
