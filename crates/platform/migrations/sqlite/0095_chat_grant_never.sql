-- 0095: an owner said "never for this room" to a mention of their machine
-- that its agents can't read (Arugula #578). One row per (room, machine):
-- later mentions there raise no card and say "not granted". Times are
-- milliseconds since the epoch. Dropped with the room.
CREATE TABLE chat_grant_never (
    room TEXT NOT NULL,
    machine TEXT NOT NULL,
    at INTEGER NOT NULL,
    PRIMARY KEY (room, machine)
);
