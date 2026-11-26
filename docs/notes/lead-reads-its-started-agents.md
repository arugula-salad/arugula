---
section: Fixed
title: A board lead reads and closes the agents it started
line: A board's lead can read and close the issue agents it started on its own machine, in whatever tab they opened.
headline: no
---
A lead that started an issue agent with `start_agent` (no `to`) was refused `read_output` and `close` on it, with "in another tab". An agent block's token now reaches the blocks it started, in any tab, as well as its own tab. What it didn't start in another tab stays out of reach.
