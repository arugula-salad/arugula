---
schema: 1
id: "toy-000"
title: "Ship on every merge"
state: "decided"
area: "delivery"
source:
  kind: "workspace"
  member: "delivery"
question: "When does the toy ship?"
options:
  - id: "a"
    label: "on every merge"
    how: "ship runs straight through."
    tradeoff: "Fast, and nobody looks first."
choice:
  option: "a"
  reason: "Nothing here could break anyone."
rejected: []
supersedes: []
evidence:
  - title: "The toy's README"
    url: "https://example.com/toy"
    as_of: "2026-10-01T00:00:00Z"
decided_by: "lex00"
decided_on: "2026-10-01"
reviews: []
constrains:
  - "member:delivery"
---

# Ship on every merge
