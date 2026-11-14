---
section: Fixed
title: Agents can see the files you post in chat
line: Mention an agent with a screenshot and it looks at it before it answers: it saves the file on its machine and reads it, if its machine may read the room.
---
`read_thread` lists each file's id, and its `file` saves that file next to the
agent's pane for the agent to read. It goes through the same `threads.read`
grant as the room's messages, so an agent whose machine can't read the room is
refused. A mention that reaches a session names the files it carries and says
to look before answering. That part needs an Arugula cloud that sends the
files with the mention.
