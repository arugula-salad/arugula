---
schema: 1
id: "aru-053"
title: "Chat calls the owner by a short name; ledgers keep their login"
state: "decided"
area: "Agents and blocks"
source:
  via: "cli"
question: "Chat calls the owner by a short name; ledgers keep their login"
options:
  - id: "a"
    label: "Chat calls the owner by a short name; ledgers keep their login"
    how: "As the body below says."
    tradeoff: "Not recorded: DECISIONS.md kept what was chosen and why, not the options weighed."
choice:
  option: "a"
  reason: "Given in the body below, as DECISIONS.md had it."
rejected: []
supersedes: []
evidence:
  - title: "#889"
    url: "https://github.com/arugula-salad/arugula/issues/889"
  - title: "#859"
    url: "https://github.com/arugula-salad/arugula/issues/859"
decided_by: "jhgaylor"
decided_on: "2026-10-14"
reviews: []
constrains:
  - "path:crates/mux/src/mux/who_may.rs"
  - "path:crates/mux/src/mux/outside.rs"
---

# Chat calls the owner by a short name; ledgers keep their login

Superseded by M125 for chat: the daemon names nobody in chat any more, and
control calls people by their account. Ledgers still keep the full login.
On a machine's own page the owner's name was their tailnet login, an email,
so chat wrote `@jake@example.com`, while control's page (and the page's own
headers) said "jake" (#889). In threads and conversations the owner is now
their login on control when the machine is one person's and enrolled (as
control's page calls them), else their tailnet login without its domain:
their messages and a conversation's `owner_name` (`chat_name_of`). An `@` of
it still reaches them
(`threads::names` matches the part before the `@`). Ledgers keep the full
login (`name_of`): a gate's actor, which chant records, and a forge write's
sender. Rejected: shortening `name_of` itself, which changed what chant and
the forge ledgers record.
Where (until M125): `crates/mux/src/mux/who_may.rs` (`chat_name_of`, `short_name`), `crates/mux/src/mux/outside.rs` (`People::owner_login`), `crates/mux/src/mux/thread_ops.rs`.
From: #889 (M85's live run, #859).
