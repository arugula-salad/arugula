---
schema: 1
id: "aru-095"
title: "Control enrolls hosted machines under a project delegation"
state: "decided"
area: "Access and identity"
source:
  via: "cli"
  issue: "arugula-salad/arugula#1236"
question: "How does an unattended hosted machine join, when no device is there to approve it?"
options:
  - id: "a"
    label: "A project delegation: an owner's device signs once per project that control may enroll hosted machines into it"
    how: "The delegation is stored with the project's roster. Control enrolls each box with a one-time project ticket and a project-scoped certificate. Clients accept such a machine only for that project and label it \"Hosted: the operator can read\". Revoking the delegation stops new enrollments; existing machines are revoked as devices are."
    tradeoff: "Within the project, control can add machines that the project's people will trust, so the operator can read what runs in them."
  - id: "b"
    label: "An owner approves each hosted machine from a device"
    how: "Every create waits for a device approval, as a new device does."
    tradeoff: "Creating or rebuilding a workspace waits on a phone, and unattended rebuilds can't happen."
  - id: "c"
    label: "Control signs hosted machines as itself, trusted everywhere"
    how: "Control's own key vouches for hosted machines across accounts."
    tradeoff: "Control could add a reader to anything, which device chains exist to prevent."
choice:
  option: "a"
  reason: "Hosted machines must start unattended, but control must not gain a reader outside the projects whose owners chose it; the label says what's true of any VM (Jake, 2026-10-21, #1235)."
rejected:
  - option: "b"
    why: "Every create and rebuild would wait on a device."
  - option: "c"
    why: "Breaks \"Trust is a chain of device approvals\" everywhere, not only in the project."
supersedes: []
evidence:
  - title: "#1235"
    url: "https://github.com/arugula-salad/arugula/issues/1235"
  - title: "#1236"
    url: "https://github.com/arugula-salad/arugula/issues/1236"
  - title: "#1239"
    url: "https://github.com/arugula-salad/arugula/issues/1239"
  - title: "#1240"
    url: "https://github.com/arugula-salad/arugula/issues/1240"
  - title: "#1241"
    url: "https://github.com/arugula-salad/arugula/issues/1241"
decided_by: "jhgaylor"
decided_on: "2026-10-21"
reviews: []
constrains:
  - "path:crates/e2e/src/cert.rs"
  - "path:crates/e2e/src/team.rs"
  - "path:crates/e2e/src/hosted.rs"
  - "path:crates/control/src/hosted.rs"
  - "path:docs/control-e2e.md"
---

# Control enrolls hosted machines under a project delegation

An unattended hosted machine has no device present to approve its join
("Trust is a chain of device approvals"). So an owner's device signs a
**project delegation** once: "control may enroll hosted machines into
project P, until revoked" (Jake, 2026-10-21, #1235).

- **Stored with the project's roster,** signed by an owner's device like the
  roster itself. Amended: a project has no roster of its own. The hub keeps
  the delegation and its revocation in `project_delegations`, keyed by the
  project's owner (`team:<id>` or `account:<id>`), and checks them against
  the owning team's roster, or the account, as a courtesy; clients check
  again with the roster they hold (`crates/control/src/db/hosted.rs`).
- **Enrollment.** At create, the platform records a one-time enrollment
  for the box, bound to the project and the workspace: a **ticket** the hub
  makes, for the box's env (boxes started fresh, loam, testnet), or, for a
  box started from a running E2B snapshot (which can't read the env it was
  made with), the box's **sandbox id**, good for about two minutes (Jake,
  2026-10-21). The box presents it with its new key, once, and control
  enrolls it under the delegation with a **project-scoped certificate**
  signed with control's hosted key. **Fail closed:** a second try is
  refused, and the platform deletes a box that finds its enrollment taken;
  an enrollment whose delegation was revoked since is refused too. The
  sandbox id is a bearer secret for its window: never logged, never in a
  URL.
- **Only for that project.** Clients accept such a machine only for project
  P. Personal devices never trust it for anything else.
- **Labelled.** The shell and the platform page show it as "Hosted: the
  operator can read". That's true of any VM: whoever runs it can read its
  memory.
- **Revoking** the delegation stops new enrollments. Existing hosted
  machines are revoked as devices are today. Amended: they aren't. A
  device's removal is a signed statement clients keep; a hosted machine
  is removed on control's word alone: control stops listing it, refuses
  its signatures and drops it from the relay, and the workspace's delete
  or rebuild deletes its sandbox (`DELETE /api/hosted/machines/{id}`,
  `HubPort::hosted_gone`). A revocation reaches only certificates made
  after it: one made before stays accepted until the machine is removed.
  And a hosted machine is reached only through control's relay: control
  keeps no direct URL for it, and clients try none.
- **A revoked delegation stays revoked** (Jake, 2026-11-04). Amended:
  - **One judgment.** Control judges a revoke once, as it arrives: signed by a
    device of someone who's an owner now, sent from their own session, its
    time near control's clock. The first ends the delegation for good. Every
    owner's device's revoke is kept, up to 32 for a delegation, and the route
    says what became of each (`first`, `kept`).
  - **Control's word.** Control then says so to whoever checks: `arugula
    project delegation closed v1`, signed with the key the delegation names.
    It's the delegate giving up what it was given, so it needs nobody's
    authority and can only make an answer a refusal, which control could
    already give ("Control can refuse service but can't read", aru-055). Its
    time is control's clock, or the earliest revoke's own time if that's
    before it.
  - **What a client goes by.** A delegation ended at the earliest of control's
    word and any revoke by an owner of the moment; a certificate made at or
    after that is refused, and one made before stays. That goes by the
    certificate's time, not by when a machine was first seen: a machine
    enrolled after a revoke, while control still took enrollments under that
    delegation, is refused from when clients hear this word.
  - **Clients keep it.** A client that has heard control's word keeps it, by
    delegation, the earliest of each: a box and a daemon in their state
    directories, the CLI beside its login, a browser in local storage. Each
    keeps at most 512 (the earliest-ended go first), and only of a delegation
    an owner signed.
  - **One live delegation.** A project and owner have one at a time: another is
    refused until the first ends. One that names a key control no longer has,
    or whose signer isn't an owner's now, has ended by itself, so changing
    either needs no overlap. A revoke also ends any other of its project and
    owner, for projects that held two before this rule.
  - **Why a past owner's signature doesn't count.** No check can tell a revoke
    signed while its signer was an owner from one signed since and dated
    earlier, because `at` is the signer's own. So someone who's out has no new
    power: control refuses them, and their signature alone counts nowhere. A
    revoke they made while they could still stands; another owner signs a new
    delegation to go on, and machines enrolled before aren't touched.
  - **Limits.** Control can leave its word and the revokes out, and a client
    that never heard them can't tell. Control writes a certificate's time, so
    it can backdate one past the end. Control writes its word's time too, so a
    word dated forward admits later certificates unless an owner's earlier
    revoke is supplied. A client's memory goes with its local state: a browser
    whose storage is cleared, or a CLI logged in again, has to hear it again.
    Clients from before this go by revokes alone; control enrolls nothing
    under an ended delegation whichever client asks.

`docs/control-e2e.md` says what control can and can't do with a
delegation.
Where: `crates/e2e/src/hosted.rs` (the delegation, its revocation, the
certificate, and what clients accept), `crates/control/src/hosted.rs`
(delegating, enrolling, routing), `web/src/shell/e2e/hosted.ts`,
`docs/control-e2e.md` ("Hosted machines: a project's delegation") (M178).
From: #1235 (platform roadmap, step 5), M175 (#1236), M178 (#1239), M179 (#1240), M180 (#1241).
