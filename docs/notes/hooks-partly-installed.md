---
section: Changed
title: Says when Claude Code's hooks are only partly installed
line: When some of Arugula's hooks are missing or out of date (an older install, say), `arugula status` and Getting started say which, and that `arugula hooks install` adds the rest.
headline: no
---
An `arugula hooks install` from an older version can leave Claude Code's
settings with most of Arugula's hooks but not all: no SessionStart, say, so a
mention that arrives mid-turn reaches the agent unexplained. `arugula hooks
status`, `arugula status`, Getting started's Agents step and a once-only line
under the top bar now say which hooks (or read-only permission rules) are
missing, and that `arugula hooks install` adds them without changing anything
else. Hooks that aren't there at all are still an offer, the acting rules
(`--allow-acting`) stay a choice, and when a Claude Code policy blocks the
hooks, that's said instead.
