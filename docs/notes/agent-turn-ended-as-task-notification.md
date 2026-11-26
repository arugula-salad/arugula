---
section: Fixed
title: An agent block no longer stays working after background tasks
line: A Claude Code agent block whose turn ended after it ran shell commands in the background now goes idle and takes the next prompt, instead of showing working for good.
headline: no
---
After background shell tasks, Claude Code could report the turn's end as a
background task's and never answer the prompt, so the block showed working and
every prompt sent to it waited. A turn that has gone quiet after that now ends
by itself, a minute later, and what was queued goes. `prompt_agent` to a block
that is still mid-turn says the prompt is queued and when the turn last showed
anything.
