---
section: Added
title: An issue's thread has a lead
line: An agent started on an issue leads its thread, so a comment there reaches that agent wherever it runs, and the issue shows who leads it.
needs: control
headline: no
---
`start_agent` with `issue` makes the new agent the lead of that issue's thread; another agent started on it takes the lead over, and `post_thread` with `lead: true` (or `remove: true`) takes or lets go of it, as in a staff room. A post there is told to the lead, never the lead's own. `read_thread` names the lead, and the issue's page shows it above the comments.
