---
section: Changed
title: An agent session pushes only for someone who can push to the repo
line: The platform mints an agent session's push token only when the person who asked for the session can push to that repository on GitHub right now.
headline: no
---
An agent session on a hosted box pushes its branch with a token the
platform mints for that one push. The platform now mints it only when
the session's asker could push to the repository themselves: it looks up
the asker's GitHub account and asks GitHub for their role on the repo at
each push, and a role below write, no linked GitHub account, an asker
whose account is gone, or GitHub not answering means no token, with the
reason in the box's log (for example, that they can't push to
`OWNER/NAME` on GitHub). The token still writes the repository's contents
and workflows, as before, so a session begun before main changed CI can
still push; the asker's own GitHub access now bounds that. A clone of a
linked repo at a session's start is unchanged.
