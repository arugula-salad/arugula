---
section: Fixed
title: An agent's ALL-2547 finds its Plane project
line: An agent that names a Plane item as IDENT-N with no project now finds it in the project that holds that connection, instead of looking only in its default one.
headline: no
---
`forge_read`, `link`, `tracker_write` and `board_write` (making a board with
a plan) take an `IDENT-N` such as `ALL-2547` with no `project`. When the
default project (the one the agent was started on, its clone's linked
project, or the personal one) has no Plane connection with that identifier,
they look among the projects the machine's grants reach: one match is used,
several are named so the agent can pass `project`, and none says which
project it looked in and lists the identifiers it could have used.
