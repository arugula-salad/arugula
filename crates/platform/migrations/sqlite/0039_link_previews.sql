-- 0039: link previews in chat (#108). Additive: three new tables, nothing
-- existing is touched. Times are milliseconds since the epoch.

-- What control read of a web page for a preview, by the link as typed
-- (normalised: no fragment). `ok` 0 is a page that had nothing to show
-- (not HTML, too big, refused, an error): kept as long as a good one is, so
-- a link posted to a busy room is fetched once, not once per reader.
-- `image` is the proxied image's bytes, fetched the first time a reader
-- asks for it (so a page nobody opens never costs the image), with its
-- `image_type`; `image_url` is the page's own address for it.
CREATE TABLE chat_link_previews (
    url TEXT PRIMARY KEY,
    fetched_at INTEGER NOT NULL,
    ok INTEGER NOT NULL,
    site TEXT NOT NULL DEFAULT '',
    title TEXT NOT NULL DEFAULT '',
    description TEXT NOT NULL DEFAULT '',
    image_url TEXT,
    image_type TEXT,
    image BLOB
);
CREATE INDEX chat_link_previews_fetched ON chat_link_previews (fetched_at);

-- A message whose author took its previews off ("remove preview"). Rows
-- for deleted rooms are harmless: nothing reads them.
CREATE TABLE chat_preview_hidden (
    room_id TEXT NOT NULL,
    seq INTEGER NOT NULL,
    at INTEGER NOT NULL,
    PRIMARY KEY (room_id, seq)
);

-- A team's chat settings. No row: the defaults (web previews on). Only
-- web links are governed by it; a link to the team's own objects is
-- drawn from the platform's data and fetches nothing.
CREATE TABLE chat_team_prefs (
    team_id TEXT PRIMARY KEY,
    web_previews INTEGER NOT NULL DEFAULT 1
);
