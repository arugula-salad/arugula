---
section: Added
title: Agent blocks say what they've spent
line: Your agent can see what each agent block it started has spent, in tokens and dollars where the agent reports them, and the total, in list, wait and read_output.
needs: control
---
`list` shows `spent $1.23 · 45.6k tokens` on each agent block, and ends with the total for the blocks the caller started, with money kept apart by currency. `wait` and `read_output` on an agent block say the same. Codex reports tokens only, so its blocks read `45.6k tokens · cost not reported`.
