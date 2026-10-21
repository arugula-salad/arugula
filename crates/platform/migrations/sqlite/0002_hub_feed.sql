-- 0002: how far the platform has applied the hub's change feed
-- (`HubPort::changes`) to projects, team members and grants, by the reader
-- that applied it (#1187, after v0.53.2).
CREATE TABLE hub_feed (
    reader TEXT PRIMARY KEY,
    seq INTEGER NOT NULL
);
