---
section: Changed
title: Agents work in their session's project
line: An agent's tools now pick the project its session is in before the one its repo is linked to, and every tool picks it the same way.
needs: control
---
When a tool isn't told a project, it takes the first of these that has one: the project of the issue the agent was started on, the project its session is in, the project its clone's GitHub repo is linked to, then your default project. Issues, boards, links, Plane writes, agent sessions, `start_agent` and new Docs all follow this order, and the answer says which step chose. The session's step needs an Arugula control that knows it.
