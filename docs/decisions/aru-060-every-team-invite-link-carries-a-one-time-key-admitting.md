---
schema: 1
id: "aru-060"
title: "Every team invite link carries a one-time key; admitting someone compares nothing"
state: "decided"
area: "Federation and control"
source:
  via: "cli"
question: "Every team invite link carries a one-time key; admitting someone compares nothing"
options:
  - id: "a"
    label: "Every team invite link carries a one-time key; admitting someone compares nothing"
    how: "As the body below says."
    tradeoff: "Not recorded: DECISIONS.md kept what was chosen and why, not the options weighed."
choice:
  option: "a"
  reason: "Given in the body below, as DECISIONS.md had it."
rejected: []
supersedes: []
evidence:
  - title: "#762"
    url: "https://github.com/arugula-salad/arugula/issues/762"
  - title: "#765"
    url: "https://github.com/arugula-salad/arugula/issues/765"
decided_by: "jhgaylor"
decided_on: "2026-10-11"
reviews: []
constrains:
  - "path:crates/e2e/src/team.rs"
  - "path:crates/control/src/teams.rs"
  - "path:web/src/e2e/team.ts"
  - "path:web/src/control.ts"
  - "path:web/src/ui/control.tsx"
---

# Every team invite link carries a one-time key; admitting someone compares nothing

An *Ask me first* link carries a one-time key in its fragment, as a presigned
link does, and the owner's device signs it (`arugula team ask invite v1`,
with the role and when it was made). Asking to join sends the key's signature
over the asker's account and root. The owner's page shows only requests whose
proof checks out against an invite one of the team's owners signed, and the
prompt says "<name> used your invite link (made <when>, for <role>)", with no
fingerprint. Admitting signs the same v1 roster as before, so daemons need no
feature for it. Links and requests from before were dropped at the update
rather than kept on a compare path.
Why: the fingerprint was 16 hex digits of someone else's device, which
nobody checked; the link already reaches the right person, so make it prove
that.
Where: `crates/e2e/src/team.rs` (`AskInvite`), `crates/control/src/teams.rs` (`invite`, `accept_invite`), `web/src/e2e/team.ts` (`askOk`), `web/src/control.ts` (`makeInvite`, `acceptInvite`, `loadTeams`), `web/src/ui/control.tsx` (`AdmitPrompt`), [docs/control-e2e.md](docs/control-e2e.md).
From: #762, #765.
