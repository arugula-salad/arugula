---
schema: 1
id: "toy-002"
title: "Releases go out on Fridays"
state: "proposed"
area: "delivery"
source:
  kind: "workspace"
  member: "delivery"
question: "When do releases go out?"
options:
  - id: "a"
    label: "Fridays"
    how: "release runs on Fridays."
    tradeoff: "A weekend to notice problems."
choice: null
rejected: []
supersedes: []
evidence: []
decided_by: null
proposed_by: "lex00"
decided_on: null
reviews: []
constrains:
  - "path:delivery/ship.op.ts"
---

# Releases go out on Fridays
