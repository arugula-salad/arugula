---
section: Fixed
title: No hang talking to a restarted daemon on macOS
line: On macOS, a pane could keep the old daemon's socket open after a restart, and a command run right after the daemon came back could wait forever; panes now let go of it.
headline: no
---
A pane's program and the process that keeps it alive across restarts could
take a copy of a socket the daemon was opening at the same moment (macOS
can't make a socket close-on-exec as it's made). They now close anything
like that as they start.
