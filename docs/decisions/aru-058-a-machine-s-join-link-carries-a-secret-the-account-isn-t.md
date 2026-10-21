---
schema: 1
id: "aru-058"
title: "A machine's join link carries a secret; the account isn't compared"
state: "decided"
area: "Federation and control"
source:
  via: "cli"
question: "A machine's join link carries a secret; the account isn't compared"
options:
  - id: "a"
    label: "A machine's join link carries a secret; the account isn't compared"
    how: "As the body below says."
    tradeoff: "Not recorded: DECISIONS.md kept what was chosen and why, not the options weighed."
choice:
  option: "a"
  reason: "Given in the body below, as DECISIONS.md had it."
rejected: []
supersedes: []
evidence:
  - title: "#760"
    url: "https://github.com/arugula-salad/arugula/issues/760"
  - title: "#765"
    url: "https://github.com/arugula-salad/arugula/issues/765"
decided_by: "jhgaylor"
decided_on: "2026-10-11"
reviews: []
constrains:
  - "path:crates/e2e/src/cert.rs"
  - "path:crates/daemon/src/control.rs"
  - "path:crates/cli/src/control.rs"
  - "path:web/src/shell/trust/approve.tsx"
  - "path:web/src/shell/control.ts"
---

# A machine's join link carries a secret; the account isn't compared

`arugulad join`, Getting started and `arugula login` put a secret of their own
after the code in the approval link (`#join=CODE.SECRET`), which control never
sees. The approving device sends a MAC under it over the account and root it
pinned and the machine's keys, and the machine pins that root when it checks
out, asking nothing. A wrong MAC ends the join and drops the key (one guess
per code); a missing one refuses unless `--account` names the account. The
secret is 16 characters (80 bits), not the 8 first proposed: control sees the
MAC, so it could guess a shorter secret offline. A shorter typed code would
take a PAKE. Older daemons and CLIs, and a control that doesn't say
`join_secret`, keep comparing the account's fingerprint.
Why: a compare only works if the person actually compares, and over ssh it
meant going back to the terminal to type `y`.
Where: `crates/e2e/src/cert.rs` (`join_secret`, `join_mac`), `crates/daemon/src/control.rs` (`join_start`, `join_finish`, `AccountCheck`), `crates/cli/src/control.rs` (`login`), `web/src/shell/trust/approve.tsx` (`JoinPrompt`), `web/src/shell/control.ts` (`approveJoin`, `signInNext`), [docs/control-e2e.md](docs/control-e2e.md#joining-a-machine-the-secret-in-its-link).
From: #760, #765.
