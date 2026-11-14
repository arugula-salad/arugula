---
section: Fixed
title: Follow-ups survive a daemon restart
line: A follow-up or mention waiting for Claude Code in a terminal is kept on disk, so a daemon restart or update no longer loses it, and one meant for an earlier session in the pane says so.
---
Claude Code's hooks now confirm each follow-up they print. One the hook never confirmed is offered again, so a hook that died mid-answer doesn't lose it either.
