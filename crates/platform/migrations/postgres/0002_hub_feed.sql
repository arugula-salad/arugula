-- 0002: how far the platform has applied the hub's change feed
-- (`HubPort::changes`) to projects, team members and grants, by the reader
-- that applied it (#1187).
CREATE TABLE hub_feed (
    reader TEXT COLLATE pg_c_utf8 PRIMARY KEY,
    seq BIGINT NOT NULL
);
