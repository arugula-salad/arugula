---
section: Fixed
title: An agent block no longer waits forever to start
line: An agent that goes silent while its block starts is given up on; the block starts a new conversation, or stops and shows what the agent last printed.
headline: no
---
When an agent block reopened its conversation after a restart and the agent
never answered, the block said "starting" for good. Now an agent that says
nothing for 30 seconds while its conversation opens (two minutes while the
agent itself starts) is given up on; one that's busy, replaying a long
conversation say, is waited for. If the earlier conversation can't be
reopened, the block starts a new one and says so in its transcript
("Couldn't resume the earlier conversation; started a new one."). If the
agent doesn't start at all, or can't start a new conversation, the block
stops with the reason and the last lines the agent wrote to its error
output. Sending it a prompt starts it again.
