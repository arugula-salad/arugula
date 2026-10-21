-- 0004: chat's live events, numbered (M144): what each live stream is sent,
-- kept a while so a page that dropped resumes after the last one it had
-- (`Last-Event-ID`), on whichever of the platform's machines it reaches.
-- `kind` is `live` (`body` is the event, `to_account` whom only it goes
-- to) or `changed` (who sees what may have changed: streams re-read their
-- rooms). Never a message's text: a stream fetches that itself. Pruned to
-- the newest few thousand (`db::chat::CHAT_EVENTS_KEPT`), and never the
-- newest, so a number is never given twice.
CREATE TABLE chat_events (
    seq INTEGER PRIMARY KEY,
    kind TEXT NOT NULL,
    body TEXT NOT NULL,
    to_account TEXT,
    at INTEGER NOT NULL
);
CREATE INDEX chat_events_to ON chat_events (to_account) WHERE to_account IS NOT NULL;
