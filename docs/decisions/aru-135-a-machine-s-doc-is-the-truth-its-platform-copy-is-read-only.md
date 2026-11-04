---
schema: 1
id: "aru-135"
title: "A machine's Doc is the truth; its platform copy is read-only"
state: "decided"
area: "Federation and control"
source:
  via: "cli"
question: "When a machine's Doc has a copy on the platform (#418) and someone edits the copy on the web, which one is the Doc?"
options:
  - id: "a"
    label: "The machine's library Doc is the truth, and the copy is read-only"
    how: "A copy keeps the machine it came from, and the platform refuses a write to it over the Doc routes, by a person or an agent session, naming the machine. Its view has no writer, and the page says it's a copy, from which machine, and when it last updated. The machine's own puts and a delete are as they were; the copy stays in the owner's personal project, the move route having gone (amended for native #673)."
    tradeoff: "A teammate who wants to change a copy has to ask for the change on the machine, or make a Doc of the project's own."
  - id: "b"
    label: "Two-way: a web edit goes back to the machine's Doc"
    how: "The platform keeps web edits per section and the machine's next refresh pulls them into its library, merging by section rev."
    tradeoff: "A merge rule for a Doc two agents and a person fill at once, and a machine that's off for a week comes back to conflicts; the library's Doc stops being what its agent wrote."
  - id: "c"
    label: "Last writer wins, as it is"
    how: "Leave the routes as they are: the copy is a project Doc like any other, and the machine's next put replaces it whole."
    tradeoff: "An edit on the web is dropped without a word the next time the machine puts, and nothing on the page says the Doc is a copy."
choice:
  option: "a"
  reason: "A copy's edits were lost silently (Arugula #678): `may_write` didn't know a copy from a Doc made here, and the machine's next put replaced the title and sections and dropped the ones it didn't have. The Doc is its machine's, and the copy exists so the project reads it with the machine asleep (Jake, 2026-11-03)."
rejected:
  - option: "b"
    why: "A merge between a library and its copies is the larger Docs work, not a fix before launch; a Doc that really moved to a project may get rules of its own there."
  - option: "c"
    why: "It loses people's work and tells no one."
supersedes: []
evidence:
  - title: "Arugula native #678 (this record)"
    url: "https://app.arugula.io/issues/iss_f3002c1cb4437a6b3b66179afbc8b0c8"
decided_by: "jhgaylor"
decided_on: "2026-11-03"
reviews: []
constrains:
  - "aru-118"
  - "aru-134"
  - "path:crates/control-wire/src/project_docs.rs"
  - "path:crates/platform/src/project_docs.rs"
  - "path:crates/platform/src/project_docs_machine.rs"
  - "path:web/src/platform/pages/docs.tsx"
---

# A machine's Doc is the truth; its platform copy is read-only

A machine copies each Doc in its library up to its owner's personal project
(#418), keyed by the machine and the Doc's id, and puts the whole Doc again
on every change. The copy was a project Doc like any other ("A project has
Docs of its own, on the platform", aru-118): owners and editors could write
it on the web, and the machine's next put replaced the title and sections
and dropped the ones it didn't have, so the edit was lost without a word
(Arugula #678). Decided (Jake, 2026-11-03), and said in one line by "One
data model" (aru-134); this record is how the platform and the page keep
it:

- **The machine's Doc is the truth.** A copy is read-only on the platform.
  A write to it over the Doc routes, by a person or an agent session, is
  refused with a message naming the machine that holds the Doc
  (`crates/platform/src/project_docs.rs`, `copy_refused`), and a copy's
  view has no writer.
- **The page says what it is:** a copy, from which machine (by name, with
  `machine_name` beside `machine` on the wire, defaulted on the page for
  an older platform), and when it last updated; it has no form.
- **What doesn't change:** the machine's own puts
  (`crates/platform/src/project_docs_machine.rs`) and deleting a copy from
  the platform.
- **A copy stays in its owner's personal project** (amended 2026-11-04 for
  native #673): the route that moved a Doc into another project went, since
  nothing but tests called it, and with it the one way a machine's puts
  reached a team's project. A Doc for a team is made in the team's project.
  A copy a move put in a team's project before then keeps working as it
  did: the machine's puts land where the copy is, checked against its
  owner's right to write there, and none is expected in production. A Doc
  that should really move to a project may get rules of its own; that
  belongs to the larger Docs work.
- **Edits made to copies before this** can't be recovered: the machine's
  put replaced them.

Where: `crates/control-wire/src/project_docs.rs` (the wire),
`crates/platform/src/project_docs.rs` (the refusal and the writer flag),
`web/src/platform/pages/docs.tsx` (the page).
