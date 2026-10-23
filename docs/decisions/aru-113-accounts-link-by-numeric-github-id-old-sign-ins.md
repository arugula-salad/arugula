---
schema: 1
id: "aru-113"
title: "Accounts link by numeric GitHub id; old sign-ins run 90 days beside the new"
state: "decided"
area: "Access and identity"
source:
  via: "cli"
  issue: "arugula-salad/arugula#1337"
question: "How does a person's row in each product become one platform account?"
options:
  - id: "a"
    label: "Link by numeric GitHub id into a platform_account column each product adds; both paths run for 90 days; nothing is deleted until then"
    how: "A product sends its own user id and a signed-in hub assertion to a link API, and stores the platform_account it gets back in a column it adds beside its own id. Matching is by the numeric GitHub id, never the login. Old sign-in paths run beside the new for 90 days. A product's old path and tables are deleted only after that window and the last sign-in through it, with backups kept and Jake asked first. Passkeys can't move: a studio user links once with GitHub, or registers a passkey on the hub. The platform page lists linked products, with an unlink."
    tradeoff: "Every product carries two sign-in paths for at least 90 days, and someone who never used GitHub links by hand."
  - id: "b"
    label: "Link by GitHub login"
    how: "Match rows on the login."
    tradeoff: "Logins are renamed and taken by someone else, so a row could link to a stranger's account."
  - id: "c"
    label: "Migrate and cut over on one day"
    how: "Move every product's users into the platform's accounts and delete the old tables."
    tradeoff: "Anyone whose row didn't match loses their account, with no way back."
  - id: "d"
    label: "Link by email"
    how: "Match rows on email address."
    tradeoff: "Products verify email differently, and a person has several."
choice:
  option: "a"
  reason: "The numeric id never changes hands, and running both paths means nobody is locked out on the day (Jake, 2026-10-22, platform step 8, kept from the roadmap, including studio's part on Alex's behalf, pending Alex's review)."
rejected:
  - option: "b"
    why: "Never match accounts by login."
  - option: "c"
    why: "Nothing is deleted before 90 days and the last old sign-in."
  - option: "d"
    why: "The numeric GitHub id is the one key every product can prove."
supersedes: []
evidence:
  - title: "#1337"
    url: "https://github.com/arugula-salad/arugula/issues/1337"
  - title: "Arugula #22"
    url: "https://app.arugula.io/issues/iss_69d1c528120426c6b9a776435c9a10d6"
  - title: "Arugula #27"
    url: "https://app.arugula.io/issues/iss_84dc043d8828c984a990e45b43b032e6"
  - title: "Arugula #38"
    url: "https://app.arugula.io/issues/iss_e3c7b2299b351d2b2bbadced0e1f75b0"
decided_by: "jhgaylor"
decided_on: "2026-10-22"
reviews: []
constrains:
  - "path:crates/control/src/auth.rs"
  - "path:crates/control/src/account.rs"
---

# Accounts link by numeric GitHub id; old sign-ins run 90 days beside the new

A person's rows in each product link to one platform account (Jake,
2026-10-22, platform step 8, #1337, kept from the roadmap).

- **By numeric GitHub id, never by login.** Logins change hands; the id
  doesn't.
- **A link API.** A product sends its own user id and a signed-in hub
  assertion ("The hub is an issuer: one identity for every product"), and
  gets back the `platform_account`.
- **Each product adds a `platform_account` column** beside its own id.
  Nothing is deleted.
- **Old sign-in paths run beside the new for 90 days.** A product's old path
  and its unused tables go only after that window and the last sign-in
  through it, with the backups kept, and each deletion is asked of Jake
  first.
- **Passkeys can't move,** because WebAuthn binds them to one origin. A
  studio user links once with GitHub, or registers a passkey on the hub, and
  studio's own passkeys stay as a fallback for a quarter.
- **The platform page shows "Linked products",** with an unlink.

studio's part is Alex's: Jake decided it on Alex's behalf, pending Alex's
review of this record.
Where: links (Arugula #27, M229, native), `crates/control/src/auth.rs`, `crates/control/src/account.rs`, the deletions (Arugula #38, M240, native).
From: #1337 (platform step 8, tracked natively in Arugula #22).
