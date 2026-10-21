---
schema: 1
id: "aru-084"
title: "The platform mirrors linked GitHub repos: metadata, with content opt-in"
state: "decided"
area: "Federation and control"
source:
  via: "cli"
  issue: "arugula-salad/arugula#1205"
question: "What does the platform keep of a linked GitHub repo, so a phone can see its PRs and issues while every machine sleeps, and for how long?"
options:
  - id: "a"
    label: "Mirror metadata for linked repos; fetch bodies and comments on read; content mirroring opt-in per project; purge within 24 hours of an uninstall"
    how: "Webhooks and one leased poller per linked repo keep forge_items, forge_checks and forge_events current: state, title, labels, author, assignees, reviewers and checks, with no bodies. Opening an item fetches its body, comments and review threads with an installation token, kept in a memory cache for a few minutes. A project admin can turn on mirror_content for a repo, which fills forge_content. An uninstall, a suspension or a repo removed from the installation unlinks the repo, and a job purges its rows within 24 hours."
    tradeoff: "Reading a body costs a GitHub call while the cache is cold, and search over PR text needs the opt-in."
  - id: "b"
    label: "Mirror everything, bodies and comments included, for every linked repo"
    how: "Webhook payloads are stored whole; search works over all text from day one."
    tradeoff: "The platform holds every team's code review in plain text by default, including the secrets people paste into comments."
  - id: "c"
    label: "Keep the machines as the only readers, with control relaying pokes"
    how: "As today: each PR or issue block polls GitHub with its person's own gh token, and control's read-only App relays \"something changed\" pokes, deduped in memory."
    tradeoff: "N machines mean N pollers with N tokens, nothing is kept across a deploy, and a phone sees nothing while every machine sleeps."
choice:
  option: "a"
  reason: "Lists and the inbox need only metadata; bodies on read keep the platform's plaintext to what has to exist while machines sleep, and teams that want search over PR text opt in with a notice (Jake, 2026-10-21, #1204)."
rejected:
  - option: "b"
    why: "Content custody for every team by default, for a search most lists don't need; it's a project admin's choice instead."
  - option: "c"
    why: "Nothing exists while machines sleep, so a phone can't see or act on a PR, and the relay loses its state on every deploy."
supersedes: []
evidence:
  - title: "#1204"
    url: "https://github.com/arugula-salad/arugula/issues/1204"
  - title: "#1205"
    url: "https://github.com/arugula-salad/arugula/issues/1205"
  - title: "#1208"
    url: "https://github.com/arugula-salad/arugula/issues/1208"
  - title: "#1209"
    url: "https://github.com/arugula-salad/arugula/issues/1209"
  - title: "#1210"
    url: "https://github.com/arugula-salad/arugula/issues/1210"
  - title: "#1212"
    url: "https://github.com/arugula-salad/arugula/issues/1212"
decided_by: "jhgaylor"
decided_on: "2026-10-21"
reviews: []
constrains:
  - "path:crates/platform/migrations"
  - "path:crates/control/tables.toml"
  - "path:crates/control/src/forge.rs"
---

# The platform mirrors linked GitHub repos: metadata, with content opt-in

The platform mirrors the GitHub repos people link to a project, so a PR or
an issue is a platform object a phone can see and act on while every
machine sleeps (Jake, 2026-10-21, #1204). Until now a PR existed in Arugula
only as a block on a machine, polling GitHub with that person's token.

- **Metadata only.** The mirror keeps state, title, labels, author,
  assignees, reviewers, draft and merge state, and checks: enough for lists
  and the inbox. Webhooks write the rows, and one leased poller per linked
  repo backfills and reconciles them. Rows are keyed on GitHub's numeric
  repo id, so renames survive.
- **Bodies and comments are fetched on read,** after the viewer check
  ("Private items are shown per viewer, by their own GitHub access"), and
  kept in a memory cache for a few minutes, never in the database.
- **Content mirroring is opt-in per project.** A project admin turns it on
  for a linked repo after a notice that says what the platform will then
  hold; it makes PR text searchable. Turning it off purges that repo's
  content.
- **Uninstalling purges.** An uninstall, a suspension, or a repo removed from
  the installation unlinks the repo, and its items, content, drafts and
  inbox rows are gone within 24 hours.

This is platform-plane data ("Two planes, two trust rules"): it has to exist
while machines sleep. Each mirrored item belongs to the project its repo is
linked to ("The project is the unit of platform sharing"), and the tables
are listed in `tables.toml` like every other. The GitHub App's webhook and
key move from the hub to the platform to do it ("Control is two processes:
a hub and a platform", amended), and the webhook keeps its address on
`control.arugula.io` ("Platform paths stay on control.arugula.io forever,
behind the hub's gateway").

This reverses the site's forge page, which says control passes on only
"something changed", never the event itself. The privacy page and the forge
page change when the mirror goes live (M173), not before.
Where: `crates/platform/migrations/`, `crates/control/tables.toml`, `crates/control/src/forge.rs` (the App's webhook, until M161 moves it).
From: #1204 (platform roadmap, step 4), M159 (#1205), M162 (#1208), M163 (#1209), M164 (#1210), M166 (#1212).
