---
section: Fixed
title: An agent's read of the event log fits its tool output
line: Reading the event log with watch kind events gives one line per event and stops at 24 KB, with next_after to read on, so a busy hour no longer overflows the agent's tool output.
headline: no
---
An agent reading a busy hour of the event log got about 90 KB back, over Claude Code's tool-output limit, and had to parse a saved file. Each event is now its number, type, time, a summary and the ids and states it can be watched by. Name `types` to get whole events, as before. Either way the answer stops before it passes 24 KB, always with at least one event, and says to pass `next_after` to continue: no event is skipped.
