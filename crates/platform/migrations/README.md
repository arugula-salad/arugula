# The platform's migrations

The platform's tables are made and changed only by these (M142). The runner
is `src/db/migrate.rs`; its docs say how a database made before migrations
is adopted.

- `sqlite/`: SQLite, for the platform in process, self-hosted control and
  tests.
- `postgres/`: Postgres (M143: hosted control), SQLite's in Postgres's
  words, such as chat's search (FTS5 here, `tsvector` there), `BIGINT` and
  `BOOLEAN` for SQLite's integers, and `COLLATE pg_c_utf8` text. A version
  with nothing to do on Postgres is a file of comments.

Rules:
- A file is `NNNN_name.sql`, numbered from `0001` with no gaps, and listed
  in `src/db/migrate.rs`. A version means the same change in both dialects,
  with the same name.
- Never edit one that has shipped. Its checksum is recorded, and a changed
  one stops the platform at start. Add the next number.
- Each runs inside the start's one transaction: no `BEGIN` or `COMMIT`.
- Each declares `additive` in `src/db/migrate.rs`: true when it only adds
  what older code ignores (tables, columns that have a default or may be
  NULL, indexes, rows). The flag is recorded when it runs, and an older
  build (a rollback) starts on the database only if every migration it
  doesn't know is additive. **A migration that isn't additive** (a rename, a
  drop, a new `NOT NULL` column without a default, a changed meaning) **needs
  an announced window or a two-step deploy**: first ship code that works
  with both shapes, then the migration.
- A database made before migrations is adopted. If its tables match no
  version, the frozen pre-M142 schema code (`src/db/legacy.rs`, which never
  changes) runs first, so any older release upgrades straight here.
- `src/db/snapshots/` holds the schema production, main and an older
  release (v0.50.0) had before migrations, for the match test. They're never regenerated.
