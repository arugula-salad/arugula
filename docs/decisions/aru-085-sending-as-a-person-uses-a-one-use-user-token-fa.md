---
schema: 1
id: "aru-085"
title: "Sending as a person uses a one-use user token, falling back to their machine"
state: "decided"
area: "Access and identity"
source:
  via: "cli"
  issue: "arugula-salad/arugula#1205"
question: "How does the platform post a person's comment, review or issue to GitHub, as them, when every machine may be asleep?"
options:
  - id: "a"
    label: "A one-use GitHub App user token at send time, dropped after one call; else one of their online machines with gh"
    how: "The platform runs the App's user authorization in the person's browser (silent once they've authorized the App), swaps the code for a user token, makes the one call and drops the token. If that isn't possible, it asks one of the person's online machines to send with gh, as a typed daemon op. With neither, the send fails clearly and the draft is kept. The App gains issues: write and pull_requests: write only; one helper mints every installation token, down-scoped to one repo and the least permission, and writes each mint to github_token_mints."
    tradeoff: "GitHub emails every installer to approve the new permissions, and a send from a phone with no machine awake needs a browser hop."
  - id: "b"
    label: "Store each person's GitHub user token (or refresh token) on the platform"
    how: "Authorize once, keep the token, send whenever."
    tradeoff: "The platform holds a credential that acts as the person on GitHub, for everyone who ever sent."
  - id: "c"
    label: "Send as the App's bot"
    how: "Post with an installation token as arugula[bot]."
    tradeoff: "The comment isn't the person's: reviews don't count as theirs, and GitHub shows a bot."
  - id: "d"
    label: "Send only through the person's machines"
    how: "As today: a block on a running daemon sends with the person's own CLI."
    tradeoff: "Nothing can be sent while every machine sleeps."
choice:
  option: "a"
  reason: "The comment is the person's own and counts as their review, the platform never holds a token that outlives one call, and a phone can send with every machine off (Jake, 2026-10-21, #1204)."
rejected:
  - option: "b"
    why: "A stored token is a standing credential to act as the person; the platform plane never holds a person's own forge tokens."
  - option: "c"
    why: "A bot's comment isn't the person's, and its review doesn't count as theirs."
  - option: "d"
    why: "It's the fallback, not the path: it can't send with every machine asleep."
supersedes: []
evidence:
  - title: "#1204"
    url: "https://github.com/arugula-salad/arugula/issues/1204"
  - title: "#1205"
    url: "https://github.com/arugula-salad/arugula/issues/1205"
  - title: "#1213"
    url: "https://github.com/arugula-salad/arugula/issues/1213"
  - title: "#1214"
    url: "https://github.com/arugula-salad/arugula/issues/1214"
decided_by: "jhgaylor"
decided_on: "2026-10-21"
reviews: []
constrains:
  - "path:crates/control/src/forge.rs"
  - "path:crates/platform"
  - "path:crates/control/tables.toml"
---

# Sending as a person uses a one-use user token, falling back to their machine

When a person sends a draft, a comment, a review or an issue, the platform
posts it to GitHub as them (Jake, 2026-10-21, #1204):
1. **A one-use user token.** The platform runs the GitHub App's user
   authorization in the person's browser, silent once they've authorized the
   App. It swaps the code for a user token, makes the one call, and drops
   the token. The token is never stored, logged or cached.
2. **Their machine.** If no browser hop is possible, the platform asks one of
   the person's online machines to send it with `gh`, as a typed daemon op.
3. **Otherwise the send fails clearly** and the draft is kept.

The comment is the person's own on GitHub, not `arugula[bot]`'s, so a review
counts as theirs. The platform never holds a person's own GitHub token,
which keeps "Two planes, two trust rules": the platform plane holds no
forge tokens of anyone's.

**The App gains write, narrowly.** It gets `issues: write` and
`pull_requests: write`, nothing else, and GitHub emails every installer to
approve. Installation tokens are down-scoped in code: one helper mints all
of them, each for one repo and the least permission it needs (read for
ingest and reads, write only inside a send). Every token minted, user or
installation, is written to an audit log (`github_token_mints`: scope, repo,
caller, purpose, time, and never the token).

**Agents draft, people send** still holds ("Forge blocks use the person's
own CLI, and agents draft while people send", amended). A send is always a
person's action, and no grant makes an agent a sender ("Grants, not
impersonation"). Who the person is comes from the hub ("The hub stays the
identity provider; passkeys are bound to control.arugula.io").
Where: `crates/control/src/forge.rs` (token minting, until M161 moves it to `crates/platform/`), `crates/control/tables.toml` (`github_token_mints`).
From: #1204 (platform roadmap, step 4), M159 (#1205), M167 (#1213), M168 (#1214).
