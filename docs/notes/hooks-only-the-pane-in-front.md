---
section: Fixed
title: Apps started from a pane keep off it
line: Claude Code in VS Code or the desktop app started from a pane no longer puts its questions, permission prompts and attention on that pane, or becomes the conversation the pane resumes.
headline: no
---
Claude Code's hooks name the pane they run in, and an app started from a pane
inherited that name, so every session in the app used the pane. Now a pane
takes a hook only from the Claude Code in front of it: the one at its prompt,
or in the `tmux` window shown there. Elsewhere, Claude Code asks in its own
dialog. Update `arugula` on the machine for this; an older `arugula` behaves as
before.
