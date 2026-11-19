---
section: Fixed
title: An agent block no longer waits forever to start
line: An agent that doesn't answer while its block starts is given 30 seconds; then the block starts a new conversation, or stops and shows what the agent last printed.
headline: no
---
When an agent block reopened its conversation after a restart and the agent
never answered, the block said "starting" for good. Each step of starting an
agent now has 30 seconds. If reopening the earlier conversation takes longer,
or fails, the block starts a new one and says so in its transcript ("Couldn't
resume the earlier conversation; started a new one."). If the agent doesn't
start at all, or can't start a new conversation, the block stops with the
reason and the last lines the agent wrote to its error output. Sending it a
prompt starts it again.
