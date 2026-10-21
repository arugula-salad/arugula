# The platform's migrations

The platform's tables are made and changed only by these (M142). The runner
is `src/db/migrate.rs`; its docs say how a database made before migrations
is adopted.

- `sqlite/`: SQLite, for the platform in process, self-hosted control and
  tests.
- `postgres/`: Postgres (M143), written where the two must differ, such as
  chat's search (FTS5 here, `tsvector` there).

Rules:
- A file is `NNNN_name.sql`, numbered from `0001` with no gaps, and listed
  in `src/db/migrate.rs`. A version means the same change in both dialects,
  with the same name.
- Never edit one that has shipped. Its checksum is recorded, and a changed
  one stops the platform at start. Add the next number.
- Each runs inside the start's one transaction: no `BEGIN` or `COMMIT`.
- `src/db/snapshots/` holds the schema production and main had before
  migrations, for the match test. They're never regenerated.
