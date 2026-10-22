---
schema: 1
id: "aru-103"
title: "The deploy event log is append-only, and anyone can verify its approvals"
state: "decided"
area: "State and durability"
source:
  via: "cli"
  issue: "arugula-salad/arugula#1317"
question: "How is a deploy's history kept, and what may we call it?"
options:
  - id: "a"
    label: "Append-only, hash-chained deploy_events with device-signed approvals; not called WORM"
    how: "deploy_events rows are inserted and never updated or deleted, and each carries the hash of the event before it. run_events are append-only too. Approvals are device signatures over the operation's bytes, so anyone with the owner device's public key can check one. Whoever runs the platform's database could still rewrite it, so the log is tamper-evident, not write-once storage, and nothing calls it WORM."
    tradeoff: "A rewrite by the database's operator is caught against an earlier copy, not prevented."
  - id: "b"
    label: "WORM storage"
    how: "Write the log to object storage under a retention lock."
    tradeoff: "A second store for a log people read on the page, and the deploy state would split across them."
  - id: "c"
    label: "Updatable rows with an audit table"
    how: "Deploy rows change in place, and a trigger copies old values."
    tradeoff: "The history is whatever the code chose to copy."
choice:
  option: "a"
  reason: "Tamper-evident and verifiable is what deploys need now, and the word WORM promises more than a database table gives (Jake, 2026-10-21, platform step 7, kept from the roadmap)."
rejected:
  - option: "b"
    why: "More storage and promises than step 7 needs."
  - option: "c"
    why: "History must not be rewritable by the code that writes it."
supersedes: []
evidence:
  - title: "#1317"
    url: "https://github.com/arugula-salad/arugula/issues/1317"
  - title: "#1290"
    url: "https://github.com/arugula-salad/arugula/pull/1290"
  - title: "S37"
    url: "https://github.com/arugula-salad/arugula/blob/main/spikes/s37-team-permissions/README.md"
decided_by: "jhgaylor"
decided_on: "2026-10-21"
reviews: []
constrains:
  - "path:crates/platform/migrations"
  - "path:crates/platform/src/db"
---

# The deploy event log is append-only, and anyone can verify its approvals

A deploy's history is a log anyone can check, and we don't call it WORM
(Jake, 2026-10-21, platform step 7, #1317, kept from the roadmap).

- **Append-only.** `deploy_events` rows are inserted and never updated or
  deleted. So are `run_events` ("A run is a platform object on a hosted
  workspace").
- **Hash-chained.** Each event carries the hash of the one before it, so a
  copy of the log shows any change made since.
- **Approvals can be verified by anyone.** Each is a device signature over
  the deploy's operation ("A deploy needs an owner device's approval, and
  agents never approve"), checked against the owner device's public key.
- **Not WORM.** Whoever runs the platform's database could still rewrite it.
  The log makes that visible, not impossible, and nothing calls it WORM.
Where: environments and deploys (Arugula #13, M217, native), `crates/platform/migrations/`, `crates/platform/src/db/`.
From: #1317 (platform step 7), the plan in #1290, S37 (q3).
