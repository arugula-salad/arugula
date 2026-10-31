---
schema: 1
id: "aru-119"
title: "An account can reset its devices from a fresh GitHub sign-in, after a wait"
state: "decided"
area: "Federation and control"
source:
  via: "cli"
  issue: "arugula-salad/arugula#1582"
question: "When none of an account's devices can approve and its recovery codes are gone, how does the person get back in?"
options:
  - id: "a"
    label: "A reset: a fresh GitHub sign-in starts it, a wait lets any device cancel it, then the account starts a new chain"
    how: "From the approval screen, when no device can approve here, a GitHub sign-in from the last ten minutes (REAUTH_MS) asks control to reset the account. Control tells every device and the account's email, and waits. Any device or recovery code still good can cancel. When the wait ends, control revokes every device and recovery code and lets the next browser enroll as the account's new first device, with new recovery codes. Machines keep their pin of the old root and refuse the new chain until the person joins each one again, on the machine. Support can start the same reset for a person who can't, after its own check, and it's logged."
    tradeoff: "Control (or someone holding a fresh GitHub session through the wait) can replace an account's devices, visibly. Teammates see a changed key; anything end to end encrypted to the old keys can't be read."
  - id: "b"
    label: "Only support can reset, by hand"
    how: "The same reset, started only by an operator after checking who's asking."
    tradeoff: "Every stranded person waits on a ticket, and support needs a check strong enough to stand in for the wait."
  - id: "c"
    label: "Keep it as is: recovery codes are the only way back"
    how: "Nothing changes; the approval screen says so."
    tradeoff: "Someone who lost every device and code loses the account. Today that includes everyone whose approvals were on an address control left."
  - id: "d"
    label: "A fresh GitHub sign-in approves this browser straight away"
    how: "Control vouches for the browser on the spot, with no wait and no reset."
    tradeoff: "Control or a stolen GitHub session adds a reader silently, and existing machines would have to accept it."
choice:
  option: "a"
  reason: "An account whose devices are all gone can't be recovered today, and that's not acceptable for a paying customer (lex00, 2026-10-27, #1582): it happened when control moved from control.illogical.widgets.wtf and every approval stayed on the old origin. Jake approved the concept (2026-10-27). The wait and the notices keep a stolen session from doing it unseen, and machines needing a join on the machine keeps aru-056's promise for terminals: control can't add a reader to a machine that's already joined."
rejected:
  - option: "b"
    why: "Kept as the backstop inside (a), not the only way: a person who still has GitHub shouldn't need a ticket."
  - option: "c"
    why: "Loses accounts, and loses them for reasons that aren't the person's fault."
  - option: "d"
    why: "No wait, no notice, and it would make existing machines trust what control says."
supersedes: []
evidence:
  - title: "#1582"
    url: "https://github.com/arugula-salad/arugula/issues/1582"
  - title: "#1581"
    url: "https://github.com/arugula-salad/arugula/issues/1581"
  - title: "#1599"
    url: "https://github.com/arugula-salad/arugula/issues/1599"
  - title: "#1386"
    url: "https://github.com/arugula-salad/arugula/pull/1386"
decided_by: "jhgaylor"
proposed_by: "lex00"
decided_on: "2026-10-27"
reviews: []
constrains:
  - "aru-056"
  - "aru-055"
  - "path:crates/e2e/src/cert.rs"
  - "path:crates/control/src/passkey.rs"
  - "path:web/src/shell/trust"
---

# An account can reset its devices from a fresh GitHub sign-in, after a wait

When none of an account's devices can approve and no recovery code is left,
the person can reset the account's devices (lex00, 2026-10-27, #1582; Jake
approved the concept).

- **Who starts it.** The person, from the approval screen, with a GitHub
  sign-in from the last ten minutes (`REAUTH_MS`, as a first passkey takes
  since #1386). Or support, for someone who can't, after its own check.
- **The wait.** 48 hours. Every device and the account's email hear of it
  when it starts. Any device or recovery code still good cancels it. Support
  can shorten it for a paying account after its check; the log says so.
- **The reset.** Control revokes every device and recovery code, and the
  next browser enrolls as the account's new first device and gets new
  recovery codes.
- **Machines.** A daemon keeps the root it pinned and refuses the new chain:
  the person joins each machine again, on the machine. So control still
  can't add a reader to a machine that's joined (aru-056).
- **Teammates.** Their browsers pinned the old root, and say the key changed
  (`web/src/shell/control.ts`, `verifyPerson`) until they check it again.
- **What's lost.** Anything end to end encrypted to the old keys.
- **Seen.** The new first device shows "reset by GitHub sign-in" (or "by
  support") in *Devices and machines*, and the reset is in the account's
  audit. At most one reset per account a week.

The 48 hours, support's shortening and the weekly limit are the starting
values; change them here when the PRs that build it say otherwise.

**Passkeys too, once they approve** (S46 q5, Jake, 2026-10-30, aru-126).
An account that has ever had a passkey gets a new first one only through
a reset: a first passkey, whether a fresh GitHub sign-in (#1386) or one of
the account's browsers vouches for it, is immediate only for an account
that never had one. Outside a reset, removing a passkey takes an assertion
from another of the account's passkeys; a session alone can't. When this
device reset is done it removes every passkey too, `by reset` in the
passkey log (for an account with GitHub linked; passkeys that are its only
way in stay).

**A passkey reset** (DK10, #393, Jake, 2026-10-31). The same reset for the
passkeys alone, because removing a passkey now takes another: a person
whose passkeys are lost (a password manager gone, a passkey for a site
control no longer serves) while their devices are in use could otherwise
neither remove nor replace them, and this reset refuses to start while a
device was used in the last two days.

- **Who starts it.** A GitHub sign-in from the last ten minutes, as above,
  from *Sign-in and account*, where the "last passkey" and "can't remove"
  refusals point. Devices in use don't stop it.
- **The wait.** 48 hours, announced to every device and the account's
  email. Any device or recovery code the account trusts cancels it, and so
  does any of its passkeys that still works (an assertion over SHA-256 of
  `arugula passkey reset cancel v1`), as does the sign-in that started it.
- **The reset.** Every passkey is removed, `by reset` in the passkey log,
  and the account may add a first one again: through a fresh GitHub sign-in
  (logged `by reset`) or one of its browsers, as for a new account.
  Devices, machines and recovery codes stay as they are.
- **Limits.** As the device reset: at most one done a week, three started a
  day. Seen in the passkey log, the account's notices and *Devices and
  machines* while it waits.

The device reset stays as it is.

Where: `crates/control/src/passkey.rs` (`fresh_github`), `crates/e2e/src/cert.rs`,
`web/src/shell/trust/`.
From: #1582, #1581, epic #1599.
