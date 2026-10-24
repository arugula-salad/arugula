-- 0043: how an account arranges its chat sidebar (#120): SQLite's
-- 0043_chat_sidebars in Postgres's words.
CREATE TABLE chat_sidebars (
    account TEXT COLLATE pg_c_utf8 PRIMARY KEY,
    doc TEXT COLLATE pg_c_utf8 NOT NULL,
    at BIGINT NOT NULL
);
