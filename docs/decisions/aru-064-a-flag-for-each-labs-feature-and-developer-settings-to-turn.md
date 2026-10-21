---
schema: 1
id: "aru-064"
title: "A flag for each Labs feature, and Developer settings to turn them on"
state: "decided"
area: "Build and release"
source:
  via: "cli"
question: "A flag for each Labs feature, and Developer settings to turn them on"
options:
  - id: "a"
    label: "A flag for each Labs feature, and Developer settings to turn them on"
    how: "As the body below says."
    tradeoff: "Not recorded: DECISIONS.md kept what was chosen and why, not the options weighed."
choice:
  option: "a"
  reason: "Given in the body below, as DECISIONS.md had it."
rejected: []
supersedes: []
evidence:
  - title: "#665"
    url: "https://github.com/arugula-salad/arugula/issues/665"
  - title: "#464"
    url: "https://github.com/arugula-salad/arugula/issues/464"
  - title: "#347"
    url: "https://github.com/arugula-salad/arugula/issues/347"
decided_by: "jhgaylor"
decided_on: "2026-10-08"
reviews: []
constrains:
  - "path:crates/proto/src/flags.rs"
  - "path:crates/daemon/src/labs/mod.rs"
  - "path:crates/daemon/src/hosts.rs"
  - "path:crates/daemon/src/mcp/tools.rs"
  - "path:crates/cli/src/main.rs"
  - "path:crates/daemon/src/args.rs"
  - "path:web/src/shell/client.ts"
  - "path:web/src/shell/ui/developer.tsx"
  - "path:crates/desktop/src/daemon.rs"
---

# A flag for each Labs feature, and Developer settings to turn them on

Chat on a machine shouldn't need Fountain, studio and the rest on with it, so
the one `labs` flag (above) is nine: `chat`, `huddles`, `vms`, `fountain`,
`studio`, `workspaces`, `guest-ssh`, `swarm-themes`, `forges`. Each is a row
in `FLAGS`, off by default. (`agents` came later; `fountain` went with
#942's F5, a retired flag ever since: "A retired flag is a name that isn't
one", below; `huddles` went when huddles were removed: "Huddles are
removed", below.)
- **`labs` is no longer a flag; in the file it means all of them.** `labs`
  in `flags.json` is the answer for every flag
  the file doesn't name itself. A machine that had Labs keeps everything, one
  feature turned off there leaves the rest, and the test suites' one switch
  (`testkit::labs`, `e2e/labs.ts`) still turns it all on. `PUT
  /api/flags/labs` and `arugulad flags labs on` are refused: nothing offers
  it.
- **Each gate reads its own flag.** `labs::on(dir, flags::NAME)` in the
  daemon; `flags::on(dir)` gives the whole set from one read, for what lists
  many (`tools/list`, both `--help`s). As before this is visibility, not
  enforcement, except for Forgejo and GitLab, which are refused. (#347 made
  it enforcement: the next entry.)
- **`HostFeatures` says which.** `labs` is now "any flag on", and `flags`
  names those that are. A page on a daemon from before reads `labs` for every
  flag, which is what it meant there. `Client.flag(name)` is the page's one
  question; `has()` stands as it was (`hasThreads()` went with the
  daemon's threads in M125).
- **Developer settings, unlocked first.** The owner's dialog (session menu,
  palette, the phone's sheet) has a switch per flag and applies it without a
  reload: the Labs chunk loads when a flag first needs it. The menus offer it
  only where `flags.json` exists (`HostFeatures.dev`), which `arugulad flags`
  makes, so a stranger doesn't meet unfinished features. The desktop app's
  *Daemon* menu and `/#developer` open it regardless.
Not done: per-flag enforcement of routes and block constructors (done since,
next entry), and flags per team on control (#347).
Where: `crates/proto/src/flags.rs`, `crates/daemon/src/labs/mod.rs` (`flags`, `on`), `crates/daemon/src/hosts.rs` (`features`), `crates/daemon/src/mcp/tools.rs` (`UNLISTED*`), `crates/cli/src/main.rs` and `crates/daemon/src/args.rs` (`LABS_*`), `web/src/shell/client.ts` (`flag`), `web/src/shell/ui/developer.tsx`, `crates/desktop/src/daemon.rs`.
From: #665, #464, #347.
