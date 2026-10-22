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
  - "path:docs/control-e2e.md"
---

# Control enrolls hosted machines under a project delegation

An unattended hosted machine has no device present to approve its join
("Trust is a chain of device approvals"). So an owner's device signs a
**project delegation** once: "control may enroll hosted machines into
project P, until revoked" (Jake, 2026-10-21, #1235).

- **Stored with the project's roster,** signed by an owner's device like the
  roster itself.
- **Enrollment.** At create, the platform hands the box a one-time project
  ticket. The box presents it, and control enrolls it under the delegation
  with a **project-scoped certificate**.
- **Only for that project.** Clients accept such a machine only for project
  P. Personal devices never trust it for anything else.
- **Labelled.** The shell and the platform page show it as "Hosted: the
  operator can read". That's true of any VM: whoever runs it can read its
  memory.
- **Revoking** the delegation stops new enrollments. Existing hosted
  machines are revoked as devices are today.

`docs/control-e2e.md` says what control can and can't do with a
delegation.
Where: `crates/e2e/src/cert.rs`, `crates/e2e/src/team.rs`, `docs/control-e2e.md` (M178).
From: #1235 (platform roadmap, step 5), M175 (#1236), M178 (#1239), M179 (#1240), M180 (#1241).
