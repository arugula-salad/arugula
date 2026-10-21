---
schema: 1
id: "aru-063"
title: "One switch for what a stranger doesn't get: the `labs` flag"
state: "decided"
area: "Build and release"
source:
  via: "cli"
question: "One switch for what a stranger doesn't get: the `labs` flag"
options:
  - id: "a"
    label: "One switch for what a stranger doesn't get: the `labs` flag"
    how: "As the body below says."
    tradeoff: "Not recorded: DECISIONS.md kept what was chosen and why, not the options weighed."
choice:
  option: "a"
  reason: "Given in the body below, as DECISIONS.md had it."
rejected: []
supersedes: []
evidence:
  - title: "#385"
    url: "https://github.com/arugula-salad/arugula/issues/385"
  - title: "#342"
    url: "https://github.com/arugula-salad/arugula/issues/342"
  - title: "#347"
    url: "https://github.com/arugula-salad/arugula/issues/347"
  - title: "#387"
    url: "https://github.com/arugula-salad/arugula/issues/387"
  - title: "#464"
    url: "https://github.com/arugula-salad/arugula/issues/464"
decided_by: "jhgaylor"
decided_on: "2026-10-08"
reviews: []
constrains:
  - "path:crates/proto/src/flags.rs"
  - "path:crates/proto/src/op.rs"
  - "path:crates/daemon/src/ops/flags.rs"
  - "path:crates/daemon/src/labs/mod.rs"
  - "path:crates/daemon/src/main.rs"
  - "path:crates/daemon/src/args.rs"
  - "path:crates/daemon/src/mcp/mod.rs"
  - "path:crates/cli/src/main.rs"
  - "path:web/src/shell/ui/developer.tsx"
---

# One switch for what a stranger doesn't get: the `labs` flag

The `labs` flag turns on chat (and huddles, until they were removed),
Fountain (until #942's F5; its daemon side was removed later),
studio apps and chant workspaces, VMs and sandboxes, guest ssh, Forgejo and GitLab and the swarm's
extra views. It is the first named flag in the daemon's config, and the
mechanism #347's per-feature flags extend:
- **The file** is `flags.json` in the state dir, `{"flags": {"labs": true}}`.
  The top-level object has room for the rest of #347's config; a write keeps
  every key and flag it doesn't know, so an older or newer daemon's aren't
  lost. It is written by a temporary file and a rename, private to the owner.
  A file that doesn't parse is logged once and read as every flag at its
  default; only an explicit set replaces it (keeping the old one as
  `flags.json.bad`).
- **The registry** is `arugula_proto::flags::FLAGS`: a name, a sentence and a
  default. `get`, `set` and `all` are the only way in; an unknown name is an
  error to set and off to get. #347 adds a row per feature and gates that
  feature's routes, blocks and MCP tools on `get`; the file, the CLI, the
  route and Settings need no change.
- **Reads are never cached.** `get` reads the file on every call (a request,
  a tool list, a forge open; none is in a loop), so a flag flips with no
  restart, whoever flipped it. That keeps what #385 had, without a cache to
  invalidate between the daemon and `arugulad flags` in another process.
  Two writers at once can lose one change, since each reads the file, changes
  a flag and renames; they are a person at a switch and one at a terminal,
  and the loser sees the flag as it now is, so there is no lock.
- **Three ways to set it:** `arugulad flags [NAME [on|off]]` works on the
  state dir, so it works with the daemon stopped; `flags.list` and `flag.set`
  (`GET /api/flags`, `PUT /api/flags/{name}`) are owner operations (#572) with
  no MCP tool, since an agent has no business with the owner's settings; the
  web client's *Labs…* in the session menu uses them and reloads the page,
  because `HostFeatures` and the Labs chunk are read at load.
- **The old `labs` file** (#385: empty, present means on) was read once
  more and retired: daemons 0.29 to 0.44 moved it into `flags.json` (unless
  that already said `labs`) on start and removed it, and `get` counted it as
  `labs` on until then. Both went in 0.45 (2026-10-18): a machine that skipped
  every release from 0.24 to 0.44 starts with Labs off and turns it back on.
- **The `labs` cargo feature (#452) is unaffected.** `labs::enabled` is still
  `BUILT && flags::get(…)`; a build without Labs can set the flag and
  nothing follows, and *Labs…* says so.
It was a file, not an environment variable, because `shellenv::clean()` drops
every `ARUGULA_*` a shell prints and the desktop app starts the daemon from
a fixed plist; that holds for `flags.json` too. The state dir survives
`arugulad uninstall`, install.sh and the desktop app's updates. Labs is per
machine, visibility rather than enforcement (a machine without it still
answered the routes, until #347: see "Each flag gates its feature" below),
and `HostFeatures.labs` tells the page.
Flags per account and per team on control (#347's last item, #464's option
2) were decided on 2026-10-12: **control pushes, and the machine can
override.** See "Control pushes Labs flags; the machine overrides" below.
Where: `crates/proto/src/flags.rs`, `crates/proto/src/op.rs` (`FlagsList`, `FlagSet`), `crates/daemon/src/ops/flags.rs`, `crates/daemon/src/labs/mod.rs` (`enabled`), `crates/daemon/src/main.rs` (`flags_cli`, the startup migration), `crates/daemon/src/args.rs` (`labs_command`), `crates/daemon/src/mcp/mod.rs`, `crates/cli/src/main.rs`. The dialog is now `web/src/shell/ui/developer.tsx` (next entry).
From: #385, #342, #347, #387, #464.
