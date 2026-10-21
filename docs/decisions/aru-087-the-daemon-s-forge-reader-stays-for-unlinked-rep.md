---
schema: 1
id: "aru-087"
title: "The daemon's forge reader stays for unlinked repos; linked repos' blocks are pins"
state: "decided"
area: "Agents and blocks"
source:
  via: "cli"
  issue: "arugula-salad/arugula#1205"
question: "Once the platform mirrors linked repos, what happens to the daemon's own forge reader and the PR and issue blocks that use it?"
options:
  - id: "a"
    label: "Keep the daemon's reader for unlinked repos, tailnet-only use, Forgejo and GitLab, through one reader crate shared with the platform; a block on a linked repo becomes a pin onto the platform's object"
    how: "The GitHub reader moves into its own crate (crates/forge-github) behind a TokenSource: the person's gh token on the daemon, an installation token or one-use user token on the platform. A PR or issue block on a linked repo is a pin {kind, global_id} that renders from the platform, and its daemon stops polling that repo. Old daemons keep polling with their own token; the poke relay is retired after a window of at least 30 days."
    tradeoff: "Two paths for one kind of block, linked and unlinked, kept alike by sharing the reader and model types."
  - id: "b"
    label: "Retire the daemon's reader; every forge block reads through the platform"
    how: "Blocks always render platform objects."
    tradeoff: "Tailnet-only users, unlinked repos, and Forgejo and GitLab would lose their blocks."
  - id: "c"
    label: "A separate reader for the platform"
    how: "Write the platform's GitHub client apart from the daemon's."
    tradeoff: "Two readers and two models of a PR that drift apart."
choice:
  option: "a"
  reason: "A linked repo is read once, by the platform, instead of once per machine, while everything the platform doesn't mirror keeps working as it does today (Jake, 2026-10-21, #1204)."
rejected:
  - option: "b"
    why: "Arugula works without control on a tailnet, and the platform mirrors only GitHub repos someone linked."
  - option: "c"
    why: "Two readers drift; one crate keeps the model types the same on both sides."
supersedes: []
evidence:
  - title: "#1204"
    url: "https://github.com/arugula-salad/arugula/issues/1204"
  - title: "#1205"
    url: "https://github.com/arugula-salad/arugula/issues/1205"
  - title: "#1206"
    url: "https://github.com/arugula-salad/arugula/issues/1206"
  - title: "#1216"
    url: "https://github.com/arugula-salad/arugula/issues/1216"
  - title: "#1218"
    url: "https://github.com/arugula-salad/arugula/issues/1218"
  - title: "#1220"
    url: "https://github.com/arugula-salad/arugula/issues/1220"
decided_by: "jhgaylor"
decided_on: "2026-10-21"
reviews: []
constrains:
  - "path:crates/daemon/src/forge"
  - "path:crates/forge-github"
  - "path:web/src/shell/blocks"
---

# The daemon's forge reader stays for unlinked repos; linked repos' blocks are pins

The daemon keeps its own forge reader (Jake, 2026-10-21, #1204). It serves
repos nobody linked, people who use Arugula on a tailnet without control,
and Forgejo and GitLab, as "Forge blocks use the person's own CLI, and
agents draft while people send" describes.

- **One reader, two token sources.** The GitHub reader and its model types
  (PRs, issues, checks, reviews, events) are their own crate,
  `crates/forge-github`, behind a `TokenSource`: the person's `gh` token on
  the daemon, an installation token or a one-use user token on the platform
  ("Sending as a person uses a one-use user token, falling back to their
  machine"). Nothing in the crate knows about blocks, `layout.json` or the
  platform.
- **A block on a linked repo is a pin** `{kind, global_id}` onto the
  platform's object ("The platform mirrors linked GitHub repos: metadata,
  with content opt-in"). It renders from the platform, and its daemon stops
  polling that repo.
- **Drafts on linked repos are platform rows.** An updated daemon uploads its
  waiting drafts for linked repos once; unlinked repos keep drafts in
  `layout.json`.
- **Old daemons keep working.** Their blocks keep polling with their own
  token, which is harmless. The hub's poke relay is retired only after a
  deprecation window of at least 30 days (M174), and then still answers
  `forge.watch` with an empty `forge.watching`, so no old daemon errors
  ("Platform paths stay on control.arugula.io forever, behind the hub's
  gateway").
Where: `crates/daemon/src/forge/`, `crates/forge-github/` (M160), `web/src/shell/blocks/` (`web/src/blocks/` before M147).
From: #1204 (platform roadmap, step 4), M159 (#1205), M160 (#1206), M170 (#1216), M172 (#1218), M174 (#1220).
