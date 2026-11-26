---
section: Added
title: Session watches can match the session's owner
line: A watch on session.needs_input or session.idle can match by, the login of the session's owner, and me is your person, so a chief of staff wakes only for its own person's sessions.
headline: no
---
A watch on `session.needs_input` or `session.idle` takes a `by` match: the
login of the session's owner, with `me` for your person. The chief of staff
sets `{"by": "me"}` on its `session.needs_input` watch.
