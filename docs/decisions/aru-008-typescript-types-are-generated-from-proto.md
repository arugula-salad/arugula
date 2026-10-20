---
schema: 1
id: "aru-008"
title: "TypeScript types are generated from `proto`"
state: "decided"
area: "Protocol and transports"
source:
  via: "cli"
question: "TypeScript types are generated from `proto`"
options:
  - id: "a"
    label: "TypeScript types are generated from `proto`"
    how: "As the body below says."
    tradeoff: "Not recorded: DECISIONS.md kept what was chosen and why, not the options weighed."
choice:
  option: "a"
  reason: "Given in the body below, as DECISIONS.md had it."
rejected: []
supersedes: []
evidence:
  - title: "#387"
    url: "https://github.com/arugula-salad/arugula/issues/387"
  - title: "#396"
    url: "https://github.com/arugula-salad/arugula/issues/396"
decided_by: "jhgaylor"
decided_on: "2026-10-06"
reviews: []
constrains:
  - "path:web/src/proto.gen.ts"
  - "path:web/src/proto.ts"
  - "path:crates/proto/src/ts.rs"
---

# TypeScript types are generated from `proto`

`web/src/proto.gen.ts` comes from the Rust types (`ts-rs`, `just proto-ts`),
and CI fails when it is stale. The hand-written `web/src/proto.ts` is being
retired.
Where: `crates/proto/src/ts.rs`, `justfile` (`proto-ts`), `web/src/proto.gen.ts`.
From: #387 (the arch tracker), #396.
