---
section: Fixed
title: A pane just started keeps its first card
line: A question or permission prompt from Claude Code that a pane runs right away shows on the pane, instead of only in Claude Code's own dialog.
headline: no
---
A pane learns its program's process id a moment after starting it. A Claude
Code hook that arrived in that moment was taken as coming from outside the
pane (an editor or the desktop app started from it), so its card never showed.
Now the pane finishes starting before it decides.
