---
schema: 1
id: "aru-101"
title: "A deploy needs an owner device's approval, and agents never approve"
state: "decided"
area: "Access and identity"
source:
  via: "cli"
  issue: "arugula-salad/arugula#1317"
question: "What approves a deploy, when does an approval go stale, and can an agent approve?"
options:
  - id: "a"
    label: "An owner device's S37 work approval over the deploy's bytes, every time, stale after another apply, never an agent"
    how: "A deploy's operation is its environment, plan digest and source. The approval is S37's work approval (arugula work approval v1), naming the request and that operation's hash, signed by a project owner's device; the phone approves with a passkey. A deploy planned before another apply to the same environment can't be approved or applied. No grant, MCP tool, agent's CLI call or thread message counts as an approval, an agent on the owner's own machine included: an agent can plan a deploy and ask for an approval, and that's all. This is enforced in code, and a test proves it."
    tradeoff: "Every deploy waits for a person with a device, routine ones too, and an agent trusted with everything else still has to ask."
  - id: "b"
    label: "Deploys run under the standing project grant"
    how: "runs.start covers a deploy like any other run."
    tradeoff: "Production changes with no person looking at the plan."
  - id: "c"
    label: "Any project member approves"
    how: "Membership is enough to sign a deploy approval."
    tradeoff: "People who can't change a project's settings could change its production."
  - id: "d"
    label: "Agents with a grant may approve"
    how: "A deploy power on a machine lets its agents approve."
    tradeoff: "An agent could plan and approve its own change, and a prompt injection would reach production."
  - id: "e"
    label: "Approvals don't go stale"
    how: "An approval stays good until it expires, whatever else applied."
    tradeoff: "Applying an older plan could undo a change made after it."
choice:
  option: "a"
  reason: "Deploys are where the platform joins an estate's blast radius, so a person with a device approves each one over exactly the bytes that will apply, and no agent ever does (Jake, 2026-10-21, platform step 7, kept from the roadmap)."
rejected:
  - option: "b"
    why: "The standing grant is for runs; a deploy is approved every time."
  - option: "c"
    why: "The approver is a project owner's device."
  - option: "d"
    why: "Agents never approve, whatever grant or identity they act under."
  - option: "e"
    why: "A deploy goes stale if anything else applied to its environment after it was planned."
supersedes: []
evidence:
  - title: "#1317"
    url: "https://github.com/arugula-salad/arugula/issues/1317"
  - title: "#1290"
    url: "https://github.com/arugula-salad/arugula/pull/1290"
  - title: "S37"
    url: "https://github.com/arugula-salad/arugula/blob/main/spikes/s37-team-permissions/README.md"
  - title: "#971"
    url: "https://github.com/arugula-salad/arugula/issues/971"
  - title: "#1002"
    url: "https://github.com/arugula-salad/arugula/discussions/1002"
decided_by: "jhgaylor"
decided_on: "2026-10-21"
reviews: []
constrains:
  - "path:crates/platform/src/grants.rs"
  - "path:crates/platform-api/src/auth.rs"
  - "path:crates/daemon/src/mcp/tools.rs"
---

# A deploy needs an owner device's approval, and agents never approve

Every deploy stops for a project owner's device (Jake, 2026-10-21, platform
step 7, #1317, kept from the roadmap).

- **Bound to the operation's bytes.** A deploy's operation is its
  environment, plan digest and source. The approval is S37's work approval
  (q3), naming the request and that operation's hash, so it can't move to
  another deploy or another plan.
- **The approver is a project owner's device.** The platform stores and
  shows the approval but can't forge it. The phone shows approve and deny
  cards with the plan digest, and approves with a passkey.
- **Stale after another apply.** A deploy planned before anything else
  applied to its environment can't be approved or applied. It's planned
  again.
- **Agents never approve,** whatever grant or identity they act under:
  - no grant (the standing project grant, `threads.post`, `issues.write`,
    `cloud.drive`), MCP tool, CLI call from an agent, or thread message
    counts as an approval;
  - that includes an agent on the owner's own machine ("Agents are in
    conversations", amended);
  - an agent can plan a deploy and ask for an approval, and that's all.

  It's enforced in code, and a test proves it.

The standing grant never covers a deploy ("One standing grant per project
starts runs").
**Amended: the approver is an owner's passkey** (Jake, 2026-10-30,
aru-126, Arugula native #385). The approval is one user-verified passkey
assertion over its bytes, from a passkey registered to a project owner, with
no device-key signature; the device's kind and the devices that certified it
play no part, and an owner's role no longer has to have been given by a
browser. Agents still never approve: they hold no passkey. Against whoever
runs the platform, an approval is tamper-evident (the log, aru-103, and the
hub's published passkey keys), not tamper-proof: the platform holds the
environment's secrets and enrolls the fresh box that applies it. Approvals
already in a deploy's log keep verifying as they were made.

Where: the deploy approval (Arugula #14, M218, native), `crates/platform/src/grants.rs`, `crates/platform-api/src/auth.rs`, `crates/daemon/src/mcp/tools.rs`.
From: #1317 (platform step 7), the plan in #1290, S37 (q3), M89 (#971, parked in #1002).
