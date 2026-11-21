---
section: Added
title: A chief of staff for each project
line: An agent that watches your project's event log from your staff room. It wakes when main goes red, a board closes or someone needs you, and it briefs you in numbers and links.
needs: control
headline: no
---
An agent starts it with `session_start` and `recipe: chief-of-staff`. It runs as an agent session on the project's Cloud workspace, in your staff room, which it leads. It reads the event log from where it left off, and it sets its watches again after each wake. The workspace pauses between wakes. It never starts, stops or closes sessions: it suggests, and you do it.

Agent sessions can also read and post in your staff room and in the project's rooms, as your agent, with `read_session_thread` and `post_session_thread`. See [the chief of staff](../chief-of-staff.md). Needs the next server release.
