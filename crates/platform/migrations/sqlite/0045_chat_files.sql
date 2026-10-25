-- 0045: files and images in chat (#107). Additive: one new table, nothing
-- existing is touched. Times are milliseconds since the epoch.

-- One row per uploaded file, bytes included (control serves it to the
-- room's readers and no one else; there is no public address). `seq` is
-- NULL until a message is posted with the file: such a row belongs to its
-- uploader alone and is swept after an hour; `ord` is its place among the
-- message's files. `scope` is the room's (so a
-- team's total is one query, and the room's erase finds it); `kind` is
-- what the bytes were sniffed as ('image', 'video', 'audio' or 'file'),
-- never what the upload claimed. `width` and `height` are an image's,
-- when its header said.
CREATE TABLE chat_files (
    id TEXT PRIMARY KEY,
    room TEXT NOT NULL,
    seq INTEGER,
    ord INTEGER NOT NULL DEFAULT 0,
    account TEXT NOT NULL,
    scope TEXT NOT NULL,
    name TEXT NOT NULL,
    mime TEXT NOT NULL,
    kind TEXT NOT NULL,
    size INTEGER NOT NULL,
    width INTEGER,
    height INTEGER,
    at INTEGER NOT NULL,
    data BLOB NOT NULL
);
CREATE INDEX chat_files_message ON chat_files (room, seq);
CREATE INDEX chat_files_scope ON chat_files (scope);
CREATE INDEX chat_files_account ON chat_files (account);
