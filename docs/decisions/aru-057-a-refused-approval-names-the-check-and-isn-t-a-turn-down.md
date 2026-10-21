---
schema: 1
id: "aru-057"
title: "A refused approval names the check, and isn't a turn-down"
state: "decided"
area: "Federation and control"
source:
  via: "cli"
question: "A refused approval names the check, and isn't a turn-down"
options:
  - id: "a"
    label: "A refused approval names the check, and isn't a turn-down"
    how: "As the body below says."
    tradeoff: "Not recorded: DECISIONS.md kept what was chosen and why, not the options weighed."
choice:
  option: "a"
  reason: "Given in the body below, as DECISIONS.md had it."
rejected: []
supersedes: []
evidence:
  - title: "#327"
    url: "https://github.com/arugula-salad/arugula/issues/327"
  - title: "#330"
    url: "https://github.com/arugula-salad/arugula/issues/330"
decided_by: "jhgaylor"
decided_on: "2026-10-06"
reviews: []
constrains:
  - "path:crates/e2e/src/cert.rs"
  - "path:crates/control/src/api.rs"
  - "path:web/src/shell/control.ts"
---

# A refused approval names the check, and isn't a turn-down

When control refuses an approval, its 403 carries a `reason` code beside the
sentence (`revoked`, `approver_untrusted`, `bad_signature`, `cant_approve`,
`recovery_for_machine`, `no_chain`), and the page acts on the code. Control,
the daemon and the CLI find the failed check with one function, so they
agree. A removed key keeps #330's words: it is refused at join with 410, and
at approval with 403 `revoked`. Closing the prompt after a refusal leaves the
join waiting; only *Cancel* turns a machine down. A browser checks its own key
against the account's trust before it offers *Approve*.
Why: "doesn't check out" left people with no way forward, and a failed
approval was recorded as a rejection.
Where: `crates/e2e/src/cert.rs` (`Trust::refusal`, `Refusal`), `crates/control/src/api.rs` (`approval_ok`), `web/src/shell/control.ts` (`REFUSED`, the `untrusted` phase).
From: #327, #330.
