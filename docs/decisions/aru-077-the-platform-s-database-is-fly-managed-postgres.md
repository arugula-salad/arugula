---
schema: 1
id: "aru-077"
title: "The platform's database is Fly Managed Postgres, moved in announced windows"
state: "decided"
area: "Federation and control"
source:
  via: "cli"
  issue: "arugula-salad/arugula#1151"
question: "What database does the platform run on so it can run two machines, and how does its data get there from control's SQLite?"
options:
  - id: "a"
    label: "Fly Managed Postgres, plain, moved in two announced windows"
    how: "A Fly Managed Postgres cluster in ewr on control's private network; plain Postgres with no extensions; a nightly dump to R2 and a documented, drilled restore. One window moves platform data out of the hub (2a, M141), one copies it into Postgres (2b, M145): a few minutes each of 503 for chat and Cloud while terminals keep working, with the old data kept read-only for 14 days as the rollback."
    tradeoff: "A monthly bill and a processor to name on the privacy page; two short outages for chat and Cloud, announced a day ahead."
  - id: "b"
    label: "Run Postgres ourselves"
    how: "A Postgres on our own Fly machines and volumes, with our own failover and backups."
    tradeoff: "We'd own failover, upgrades and backups for the one database the platform can't lose."
  - id: "c"
    label: "Stay on SQLite and replicate it (LiteFS or similar)"
    how: "One writer, read replicas, the database file shipped between machines."
    tradeoff: "Still one writer; two platform machines need a lease and write forwarding, and the replication layer is one more thing to run."
  - id: "d"
    label: "Dual-write during the move instead of windows"
    how: "Write to old and new stores at once, backfill, compare, then switch reads."
    tradeoff: "No outage, but weeks of two sources of truth and the code to reconcile them, for a move that takes minutes."
choice:
  option: "a"
  reason: "Managed Postgres lets the platform run two machines without us running a database; plain Postgres keeps it portable; minutes of announced 503s for chat and Cloud, with terminals unaffected and a 14-day read-only rollback, cost far less than dual-write (Jake, 2026-10-20, #1150)."
rejected:
  - option: "b"
    why: "Running our own failover and backups isn't where the work should go."
  - option: "c"
    why: "Doesn't give two writers, and adds a replication layer to run."
  - option: "d"
    why: "Two sources of truth and reconciliation code, to avoid a few announced minutes that terminals don't feel."
supersedes: []
evidence:
  - title: "#1150"
    url: "https://github.com/arugula-salad/arugula/issues/1150"
  - title: "#1158 (first window)"
    url: "https://github.com/arugula-salad/arugula/issues/1158"
  - title: "#1160 (Fly Managed Postgres)"
    url: "https://github.com/arugula-salad/arugula/issues/1160"
  - title: "#1162 (second window)"
    url: "https://github.com/arugula-salad/arugula/issues/1162"
decided_by: "jhgaylor"
decided_on: "2026-10-20"
reviews: []
constrains:
  - "path:crates/platform"
  - "path:crates/control/tables.toml"
  - "path:docs/control-ops.md"
---

# The platform's database is Fly Managed Postgres, moved in announced windows

The platform's database is **Fly Managed Postgres** (Jake, 2026-10-20,
#1150): in `ewr`, on control's private network, plain Postgres with no
vendor extensions, so it stays portable. A nightly dump goes to R2, the
restore is written down in `docs/control-ops.md` and drilled once (M145),
and Fly is named on the privacy page as a processor. The hub stays on its
SQLite with Litestream.

Data moves in **announced windows, not by dual-write**. There are two:
- **M141** moves the platform's data out of the hub, still on SQLite;
- **M145** copies it into Postgres.

Each is announced a day ahead on control's notices and lasts a few
minutes. Chat and Cloud answer 503 while terminals keep working, because
the relay is the hub's. The old data stays read-only for 14 days as the
rollback: nobody drops the hub's copies of moved tables, or deletes the
SQLite file after M145, before then.

Rejected: running Postgres ourselves (we'd own failover and backups), SQLite
replication such as LiteFS (still one writer, plus a layer to run), and
dual-write (weeks of two sources of truth to skip a few minutes that
terminals don't feel).
Where: `crates/platform/`, `crates/control/tables.toml`, `docs/control-ops.md`.
From: #1150, M141 (#1158), M142–M145 (#1159–#1162).
