---
section: Fixed
title: A prompt reaches an agent woken by a background task
line: A prompt sent while a Claude Code agent works on a finished background task's result now goes when that work ends, instead of sitting queued with the agent shown as working.
headline: no
---
Claude Code took a prompt that came in the middle of that work into it and never
answered it, so the agent block waited on it for good, and everything queued
after it waited too. The prompt now goes as a turn of its own once the agent is
done.
