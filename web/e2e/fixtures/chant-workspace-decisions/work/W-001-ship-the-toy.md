---
schema: 1
id: "W-001"
title: "Ship the toy"
state: "in-progress"
implements:
  - "toy-001"
needs: []
constrains:
  - "member:delivery"
evidence: []
acceptance:
  - id: "AC-1"
    text: "ship stops at approve-ship until someone approves it"
    verification: "manual"
owner: "lex00"
opened_on: "2026-10-07"
source:
  kind: "workspace"
  member: "delivery"
supersedes: []
---

# Ship the toy
