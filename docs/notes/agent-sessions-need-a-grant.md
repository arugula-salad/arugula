---
section: Changed
title: Agent sessions need Allow runs
line: Agent sessions run under the project's standing grant. Until an owner allows runs (Control, Devices and machines, Runs on cloud workspaces), asking Arugula in a thread starts nothing and says how.
---
Agent sessions now run under the same standing grant as the project's runs.
Until a project owner allows them, asking Arugula in a thread, Agent on this, Try 3 and
`@codex review` start nothing, and the thread says how to allow them.

**To allow them**, a project owner opens Control's page and goes to *Devices
and machines*, then *Runs on cloud workspaces*, and clicks *Allow runs* next to
the project. It's confirmed with one of the owner's passkeys, on a computer or
a phone. A project's *Settings*, *Runs*, links there, and `arugula runs grant
show --project <name>` says whether a project has one. A grant lasts 90 days
unless the owner picks another length: renew it before it expires to keep
sessions starting.

**Sessions already going when you update** aren't ended. A turn under way
finishes. Until the project has a grant, the session takes no follow-ups,
anything queued for it is dropped, and its thread says so once. Once an owner
allows runs, replies go on as before.

**Revoking the grant** ends the project's agent sessions too: at once with
*also stop the runs and agent sessions going*, otherwise when a turn under way
finishes. Each cloud workspace checks the grant before a session starts and
before each turn, and an agent run needs the grant's permission to instruct as
well as to start.
