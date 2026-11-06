---
section: Fixed
title: Exports and inbox marks hold up
line: Opening an issue on GitHub is far less likely to make a second copy, and a GitHub hiccup no longer clears what you marked seen or done.
headline: no
---
Opening a native issue on GitHub now records the GitHub link in the same step
that marks the send done, and only for an issue in the draft's repo. Before, a
failure between the two left the issue with no link, and a second export could
open a duplicate GitHub issue. A new export is also refused while another of
the same issue was sent, while a send whose issue couldn't be taken for its own
stands (it may be on GitHub as that, and can't be linked, so the message says
discarding the draft may open a second GitHub issue), or when a send that never
finished may have opened an issue with its title already. A duplicate is still possible if GitHub
opens the issue and the platform stops before it hears back, until the mirror
sees that issue.

An issue already on a board also joins its GitHub copy at once, so a PR that
closes the GitHub issue attaches to the board's row.

The inbox drops an entry only when GitHub clearly says you can't read the
repo. An error, a rate limit or a garbled answer from GitHub used to count as
"no" for ten minutes, which deleted your entries with their seen and done
marks.
