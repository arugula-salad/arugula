---
section: Fixed
title: A red main wakes the chief of staff
line: The chief of staff now hears when main itself goes red, and a failing pull request into main no longer wakes it.
headline: no
---
Its "main went red" doorbell watches `github.check.failed` with `match {"heads": "main"}`. That matches a check that ran on main's own head. `branches` held a pull request's base too, so before this a failing PR into main woke the chief of staff. A watch you make yourself can match on `heads` the same way.
