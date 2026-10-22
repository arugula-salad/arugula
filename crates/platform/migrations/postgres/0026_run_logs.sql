-- 0026: run logs (M213, platform step 7). A hosted run's log, in chunks
-- in the run-log bucket (the files are there; these say where). Its
-- artifacts are `run_artifacts` (0016). Times are milliseconds since the
-- epoch. `db::run_logs` has the rules.
-- SQLite's file in Postgres's words: `BIGINT` for its integers, and
-- `COLLATE pg_c_utf8` text.

-- Chunk `n` (from 0) of run `run_id`'s log: `size` bytes from `byte_offset` in
-- its pane's output, whose SHA-256 (hex) its machine said is `sha256`, at
-- `bucket_key` (`<project>/<run>/log/<n>`). Chunks follow each other: `n`
-- is the run's count of chunks before it, and `byte_offset` is at or past the
-- end of the one before. `created_at` is when the platform saw it in the
-- bucket: after the run-log days (30) the row goes, as the bucket's rule
-- takes the file.
CREATE TABLE run_log_chunks (
    run_id TEXT COLLATE pg_c_utf8 NOT NULL,
    n BIGINT NOT NULL,
    byte_offset BIGINT NOT NULL,
    size BIGINT NOT NULL,
    sha256 TEXT COLLATE pg_c_utf8 NOT NULL,
    bucket_key TEXT COLLATE pg_c_utf8 NOT NULL,
    created_at BIGINT NOT NULL,
    PRIMARY KEY (run_id, n)
);
CREATE INDEX run_log_chunks_created ON run_log_chunks (created_at);

-- Every upload URL the platform signed for a run (`id`, random hex): for
-- which key, what (`log` or `artifact`), how many bytes, when, and when it
-- stops working. They count against the run's caps and its project's daily
-- bytes while they're out, so a machine can't fill the bucket by asking
-- and never saying it put anything; and the sweep deletes the key of one
-- that expired with nothing recorded there. Gone a day after they expire.
CREATE TABLE run_upload_urls (
    id TEXT COLLATE pg_c_utf8 PRIMARY KEY,
    run_id TEXT COLLATE pg_c_utf8 NOT NULL,
    project_id TEXT COLLATE pg_c_utf8 NOT NULL,
    bucket_key TEXT COLLATE pg_c_utf8 NOT NULL,
    kind TEXT COLLATE pg_c_utf8 NOT NULL CHECK (kind IN ('log', 'artifact')),
    size BIGINT NOT NULL,
    issued_at BIGINT NOT NULL,
    expires_at BIGINT NOT NULL
);
CREATE INDEX run_upload_urls_run ON run_upload_urls (run_id, issued_at);
CREATE INDEX run_upload_urls_project ON run_upload_urls (project_id, issued_at);
CREATE INDEX run_upload_urls_expires ON run_upload_urls (expires_at);
CREATE INDEX run_artifacts_created ON run_artifacts (created_at);
