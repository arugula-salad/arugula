---
section: Fixed
title: Board agent chips stay current
line: An agent's chip on a board drops the last turn's diff once a turn leaves none, and a row whose agent's machine left control no longer keeps a lane set by hand that its work has outgrown.
headline: no
---
A machine now says when a turn left no diff, so the chip stops showing the
one before. A self-hosted control older than this release shows such a turn
as "0 files changed, +0 −0" until it is updated.

A report that can't be sent is kept when control fails, not dropped, however
long control is out of reach. A close that control keeps answering with
server errors is given up after about an hour, so it can't slow every other
report. An agent block whose machine left control is read as an earlier
agent as soon as its report goes stale.
