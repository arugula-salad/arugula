-- 0043: how an account arranges its chat sidebar (#120): the rooms it
-- starred, its own named sections and what's in them, which groups it
-- collapsed and how each is sorted. One row per account, so it follows
-- the person across devices; `doc` is the JSON of
-- `wire::chat_sidebar::ChatSidebar`, which `chat::sidebar` checks on the
-- way in (counts and lengths) and the page reads leniently (a room it
-- can't see is skipped). Last write wins. `at` is milliseconds since the
-- epoch. Additive: a new table nothing older reads.
CREATE TABLE chat_sidebars (
    account TEXT PRIMARY KEY,
    doc TEXT NOT NULL,
    at INTEGER NOT NULL
);
