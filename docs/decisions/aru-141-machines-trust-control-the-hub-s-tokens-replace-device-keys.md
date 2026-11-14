---
schema: 1
id: "aru-141"
title: "Machines trust control: the hub's tokens replace device keys"
state: "decided"
area: "Federation and control"
source:
  via: "cli"
question: "Approving each new browser, phone and machine with a device you already have is the step people trip on, and most machines are now added by a script or by the platform. Do terminals stay end to end, with control unable to read them or add a reader, or does control decide who reaches a machine?"
options:
  - id: "a"
    label: "Control decides who reaches a machine; the hub's tokens replace device keys"
    how: "Signing in is enough. To open a pane, a client gets a short-lived token from the hub naming the person's account, one machine and one role, and presents it on the relay or a direct path; the daemon checks the hub's signature, the machine and the expiry. Machines join with a join token from a logged-in CLI, project settings or the API. Terminal traffic is TLS to the relay, which can read it in memory and stores none of it. Device certificates, approvals, join secrets, recovery codes, PRF, Noise channels and signed rosters are deleted. Passkeys still approve deploys, standing run grants and owner changes."
    tradeoff: "Whoever runs control, breaks into it or steals a session can open a shell on every machine that account reaches, not only read it. The public promise that control can't read terminals goes."
  - id: "b"
    label: "Keep device keys, and control approves new devices itself"
    how: "Control holds a root key that machines pin, and certifies a signed-in browser at once. Noise channels, certificates and the trust code stay; the approval step goes."
    tradeoff: "The same trust in control as (a), with all of the key code kept to say otherwise."
  - id: "c"
    label: "Keep end to end as a per-machine opt-in"
    how: "A machine can be locked to approved devices; every other machine trusts control as in (a)."
    tradeoff: "Two ways to reach a machine, both kept working and tested, for a mode few would turn on."
  - id: "d"
    label: "As (a), plus a passkey tap the first time a new browser opens a shell on a machine"
    how: "The hub refuses a machine token for a browser that hasn't made a user-verified passkey assertion for that machine."
    tradeoff: "A step on every new browser again, though on the device in hand; it doesn't stop control itself."
choice:
  option: "a"
  reason: "The approval step costs more than \"control can't read\" is worth to the people using Arugula: it blocks signing in to work on a project, and machines are mostly added programmatically (Jake, 2026-11-13). Trusting control also removes the one exception in the work-first RFC (a key-holding page that the project app would frame), so the app can be one app on one origin."
rejected:
  - option: "b"
    why: "It keeps the cost of the key code without its benefit."
  - option: "c"
    why: "One behaviour for every machine, and the code really goes."
  - option: "d"
    why: "Jake chose no step-up (2026-11-13). The three guards below limit a stolen token instead."
supersedes:
  - decision: "aru-055"
  - decision: "aru-056"
  - decision: "aru-057"
  - decision: "aru-058"
  - decision: "aru-059"
  - decision: "aru-060"
  - decision: "aru-061"
  - decision: "aru-079"
evidence:
  - title: "#agents, 2026-11-13, the plan (seq 61) and the decisions in its thread (seq 65, 67)"
  - title: "RFC: Work-first Arugula (Doc pdc_1fe6170a1c02bfac4e60295c0e439f08)"
  - title: "Board: The inversion: machines become a component (brd_d08cd00433b2b2a052ba37b7899d3785, #982–#990)"
decided_by: "jhgaylor"
decided_on: "2026-11-13"
reviews: []
constrains:
  - "aru-071"
  - "aru-095"
  - "aru-119"
  - "aru-126"
  - "aru-131"
  - "aru-132"
  - "aru-136"
  - "path:crates/e2e/src/cert.rs"
  - "path:crates/e2e/src/channel.rs"
  - "path:crates/e2e/src/team.rs"
  - "path:crates/daemon/src/e2e.rs"
  - "path:crates/daemon/src/control.rs"
  - "path:web/src/shell/e2e"
  - "path:web/src/shell/trust"
  - "path:web/scripts/check-imports.mjs"
  - "path:docs/control-e2e.md"
  - "path:docs/what-leaves-your-machine.md"
---

# Machines trust control: the hub's tokens replace device keys

Device keys answered "may this client reach my machine?" without trusting
control: each machine pinned its account's first device and let in only keys
that chain back to it, so control could relay terminals but not read them or
add a reader (aru-055, aru-056). The price was an approval from a device you
already have, for every new browser, phone, app window and machine. That is
the step people trip on, and most machines are now added by a script or the
platform. Decided (Jake, 2026-11-13), to land before the launch on Dec 1:

- **Control decides who reaches a machine.** A signed-in person asks the hub
  for a **machine token** and presents it when opening a pane, on the relay or
  on a direct path. The daemon checks it and nothing else about the client.
  No device keys, no approvals, no recovery codes, no PRF.
- **Machines join with a join token.** A logged-in CLI joins its machine
  without asking. Project settings and the API make tokens for scripts and
  images. The platform makes one for a hosted box, so the project delegation
  (aru-095) is no longer needed. A machine keeps its own key to sign its
  calls to control (aru-076's signed requests); that key no longer takes part
  in deciding who reaches it.
- **Terminal traffic is TLS to the relay.** The relay can read terminal bytes
  in memory and stores none of them. Each viewer is still its own stream, so
  revoking a person closes theirs.
- **Passkeys still approve actions** (aru-126's passkey half): deploys,
  standing run grants and owner changes. An agent holds no passkey and never
  approves.
- **Deleted, with no opt-in kept:** device certificates and the trust chain,
  approvals and their codes, join secrets, recovery codes, the device reset
  (the passkey reset stays, aru-119), PRF-derived keys, Noise channels, and
  signed team rosters, which become the platform's roles. With no keys in a
  page, aru-079's origin split has no reason: the shell and the platform page
  can be one app on one origin, and the import fence and the key-bundle check
  go with it.

## The machine token

A token the hub signs with its issuer keys (aru-106), short-lived and bound to
one machine and one role:

| Claim | Meaning |
|---|---|
| `sub` | The person's account id |
| `aud` | The machine's id; the daemon refuses a token for another machine |
| `role` | `owner`, `editor` or `viewer`, from the machine's owner and the platform's team roles at the time it's made |
| `iat`, `exp` | Made, and expiry: five minutes |
| `jti` | Unique, so the daemon can refuse a replay |

A daemon takes the hub's public keys from control when it joins and refreshes
them as they rotate (aru-106's overlapping keys). It checks the signature,
`aud`, `exp` and `jti` itself, so a relay bug can't hand one machine's token to
another. A client asks again before the token expires; a channel already open
stays open until the person is removed, when the relay closes it.

**Read-only links keep their own keys** (amended 2026-11-13, building step 2).
A machine token lives five minutes and is taken once, so it can't be a link
that lasts a day and is opened many times. A link stays what it is: a one-off
key in its fragment that the daemon lists as a viewer of one session until the
link expires (aru-055's links, `acl.link_by_key`). It never used the account's
device keys, so nothing about it changes.

**A deploy's box refuses tokens** (#2510). It lets nobody in (M219), so the
daemon refuses a machine token there before reading it, whatever control says.

## The guards

Since control, or a stolen session, can now type into a person's own machine,
three cheap limits (proposed by the project-inversion agent and accepted by
Jake, 2026-11-13):

- **Tokens are bound to one machine and one role,** as above. A viewer's
  token can't send input on any machine.
- **The daemon logs each remote session it accepts:** who, which role, over
  which path, from when to when. The machine's own page and the CLI show the
  log.
- **A per-machine "remote input off" switch.** It's set on the machine, by
  the CLI or the machine's own page, never from control. While it's off, the
  daemon treats every remote token as `viewer`.

## What it costs

- Whoever runs control, breaks into it or steals a session can open a shell
  on every machine that account reaches, not only read it. Short tokens, the
  log and the switch limit this; they don't prevent it.
- What we say publicly changes: `docs/what-leaves-your-machine.md`,
  `docs/control-e2e.md` and the site say control relays terminals it can't
  read. Those change before the launch, in the PRs that build this.
- Anything encrypted end to end to device keys today is lost when the keys
  are deleted.

## Order

1. This record.
2. The daemon accepts machine tokens alongside device certificates, so
   today's clients keep working.
3. Machines join with join tokens, and the approval screens come off.
4. The privacy copy is rewritten.
5. The project app draws panes itself (the inversion board's #990).
6. The key code is deleted once the fleet has updated.

Where: `crates/e2e/src/cert.rs`, `crates/e2e/src/channel.rs` and
`crates/e2e/src/team.rs` (to be deleted), `crates/daemon/src/e2e.rs` and
`crates/daemon/src/control.rs` (the daemon's checks and what it pins),
`web/src/shell/e2e/`, `web/src/shell/trust/`, `web/scripts/check-imports.mjs`;
the hub's issuer, relay and join routes are in arugula-salad/cloud.
