---
schema: 1
id: "toy-001"
title: "A person approves each ship"
state: "decided"
area: "delivery"
source:
  kind: "workspace"
  member: "delivery"
question: "Who lets a ship go out?"
options:
  - id: "a"
    label: "a gate a person approves"
    how: "ship stops at approve-ship until someone approves it."
    tradeoff: "Slower, and someone has to be around."
  - id: "b"
    label: "no gate"
    how: "ship runs straight through."
    tradeoff: "Fast, and nobody looks first."
choice:
  option: "a"
  reason: "A ship reaches people, so a person should say yes to each one."
rejected:
  - option: "b"
    why: "Nobody would look before it went out."
supersedes:
  - decision: "toy-000"
evidence:
  - title: "The ship op, as decided"
    path: "delivery/ship.op.ts"
    sha256: "c03f661ea9635a1d4544289b6b513b0087d5dd5668eb54baad0eaed0fb52ad3b"
    as_of: "2026-10-02T00:00:00Z"
decided_by: "lex00"
decided_on: "2026-10-02"
reviews: []
constrains:
  - "member:delivery"
---

# A person approves each ship
