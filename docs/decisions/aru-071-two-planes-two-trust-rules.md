---
schema: 1
id: "aru-071"
title: "Two planes, two trust rules"
state: "decided"
area: "Federation and control"
source:
  via: "cli"
  issue: "arugula-salad/arugula#1123"
question: "What may control hold in plaintext, now that Cloud workspaces, agents driving them and chat all live there?"
options:
  - id: "a"
    label: "Two planes: the machine plane stays end to end; the platform plane holds plaintext for what must exist while every machine is asleep, listed table by table in a file CI checks"
    how: "aru-055 is scoped to the machine plane. crates/control/tables.toml lists every table control holds with its plane, what it holds and why it must exist asleep, and docs/control-e2e.md's \"What control stores\" is generated from it."
    tradeoff: "Says out loud that control reads some things; what it reads is a list anyone can diff."
  - id: "b"
    label: "Keep aru-055 as the rule and record each platform feature as an exception"
    how: "As today: aru-046, aru-047 and aru-051 each carve out an exception."
    tradeoff: "Every new platform feature is another exception, and nothing says where the exceptions stop."
  - id: "c"
    label: "Encrypt the platform objects too"
    how: "Chat, Cloud workspaces and secrets end to end, keys on members' devices."
    tradeoff: "Nothing works while every machine is asleep, which is the reason these live in control."
choice:
  option: "a"
  reason: "Platform features have to work with every machine asleep, so control must hold them; saying so as a checked list keeps the machine plane's promise intact and makes narrowing the platform plane later a diff, not a retraction (Jake, 2026-10-20, #1122)."
rejected:
  - option: "b"
    why: "Exceptions with no boundary are what made control feel like a hub that reads; a list has an edge."
  - option: "c"
    why: "Defeats the point of the platform plane: it exists for what has to work while machines sleep."
supersedes: []
evidence:
  - title: "#1122"
    url: "https://github.com/arugula-salad/arugula/issues/1122"
  - title: "#1123"
    url: "https://github.com/arugula-salad/arugula/issues/1123"
  - title: "#1124"
    url: "https://github.com/arugula-salad/arugula/issues/1124"
decided_by: "jhgaylor"
decided_on: "2026-10-20"
reviews: []
constrains:
  - "path:crates/control/tables.toml"
  - "path:docs/control-e2e.md"
  - "path:crates/control/src/db.rs"
  - "path:crates/control/src/db"
---

# Two planes, two trust rules

Control is two planes with two trust rules (Jake, 2026-10-20, #1122):
- **The machine plane** is terminal bytes, pane content, machine keys and
  the relay between a client and the daemon that owns a terminal. Control
  can't read any of it: "Control can refuse service but can't read" holds
  here, unchanged.
- **The platform plane** holds plaintext for what has to exist while every
  machine is asleep, and nothing else: team chat ("Chat lives in control, in
  plain text"), Cloud workspaces and their transcripts ("Cloud execution is
  owned by Arugula control"), cloud secrets control can decrypt, Shared
  Docs, billing and usage, and later issues, runs and deploys. It never holds
  terminal bytes, machine keys, or a person's own credentials (passkeys,
  forge tokens).

Every table and view control holds is listed in `crates/control/tables.toml`
with its plane, whether it holds metadata, ciphertext or plaintext, why it
has to exist while machines are asleep, whether it goes with the account,
and which third parties see it. A test builds a fresh database and fails
when a table is missing from the list or the list names one that isn't
there, and "What control stores" in `docs/control-e2e.md` is generated from
it. What we say publicly about what control holds is that list, so
narrowing the platform plane later is a diff, not a retraction. The privacy
page follows the list with the launch (#213).

A third plane, the substrate (compute providers behind one interface), comes
in later steps and holds nothing of its own here.

Planes are trust rules, not processes. Since step 2 (#1150) control runs as
two processes, a hub and a platform ("Control is two processes: a hub and a
platform"), and they don't line up with the planes exactly. The hub holds
the machine plane (the relay, devices, rosters) and identity, and also some
platform-plane data: Shared Docs and doc links stay with the hub's doc
origins. The platform process holds the rest of the platform plane: chat,
Cloud workspaces and their transcripts, cloud secrets, billing, projects
and grants. `tables.toml` says both for each table: `plane` for the trust
rule and `process` for who holds it.
Where: `crates/control/tables.toml`, `crates/control/src/db.rs`, `crates/control/src/db/`, `docs/control-e2e.md`.
From: #1122 (platform roadmap, step 1), M127 (#1123), M128 (#1124).
