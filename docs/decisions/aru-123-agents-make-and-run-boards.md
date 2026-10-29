---
schema: 1
id: "aru-123"
title: "Agents make and run boards, but don't destroy other hands' work"
state: "decided"
area: "Agents and blocks"
source:
  via: "cli"
question: "What does an agent's boards.write grant cover?"
options:
  - id: "a"
    label: "Rows only (today)"
    how: "An agent adds rows to the zones there are, sets any row's note and status, moves rows and removes the rows its own machine added. A person makes the board, its zones and its goal, and closes it."
    tradeoff: "A lead agent planning an epic can't start the board it plans on: a person has to make it and tell the agent to use it."
  - id: "b"
    label: "Boards, zones, goal and close, but not destroying others' work"
    how: "With boards.write over the project an agent also makes a board (with a goal), sets its title and goal, makes, renames, moves and deletes zones (PROOFS too), closes a board with a done note and opens it again, and repoints a row its own machine added. It still can't delete a zone that holds a row another hand added, remove such a row, or repoint one, and deleting a whole board stays a route nobody has. A board it makes or closes says so (created_by_agent, closed_by_agent), as a row it adds does; created_by and closed_by stay the person it acted for. The person's role is still the ceiling, asked again on every call."
    tradeoff: "An agent can reshape a board a person made, and close an epic; closing and zone changes are reversible, but a zone it deletes takes its own rows with it."
  - id: "c"
    label: "Everything a person can do"
    how: "An agent's grant is a person's on boards: it also removes and repoints any row, and deletes any zone."
    tradeoff: "An agent can destroy what people wrote, and the board stops being the people's view of what the agents are doing."
choice:
  option: "b"
  reason: "Decided by alecraso, 2026-10-29, on Arugula native #335. Boards are agents' management tool and people's view of it, so a person no longer has to make a board for an agent to use it. Everything stays labelled as the agent, the person's role is the ceiling, and what destroys another hand's work stays refused. It amends aru-122, which says agents write under a machine grant: this widens what that grant covers."
rejected:
  - option: "a"
    why: "Backwards for a lead agent planning an epic: a person has to make the board first."
  - option: "c"
    why: "Removing or rewriting people's rows destroys their work, and a board's labels are only worth reading if people's rows stay theirs."
supersedes: []
evidence:
  - title: "Arugula native #335 (Boards: agents can make and run boards)"
    url: "https://app.arugula.io/issues/iss_fd687a1e121fa814cb1852dd6ab2f46f"
decided_by: "alecraso"
decided_on: "2026-10-29"
reviews: []
constrains:
  - "aru-122"
  - "path:crates/platform/src/boards.rs"
  - "path:crates/platform/src/boards/agent.rs"
  - "path:crates/platform/src/db/boards.rs"
  - "path:crates/control-wire/src/boards.rs"
  - "path:crates/arugulad-control-tools/src/boards.rs"
---

# Agents make and run boards, but don't destroy other hands' work

With `boards.write` over a project, an agent makes and runs boards (Arugula
native #335). It amends aru-122, whose agents wrote under a machine grant to
rows only.

- **It may:** make a board, with a goal; set a board's title and goal; make,
  rename, move and delete zones (the PROOFS zone too); close a board with a
  done note and open it again; repoint a row its own machine added.
- **It may not:** delete a zone that holds a row another hand added, remove a
  row another hand added, or repoint one. The platform answers 403 with a
  plain sentence. Deleting a board isn't a route.
- **Labelled:** a board says an agent made or closed it (`created_by_agent`,
  `closed_by_agent`, a machine), beside the person it acted for.
- **Ceiling:** the person's role in the project, and the machine's grant over
  it, are asked on every call; a viewer's machine makes nothing.
