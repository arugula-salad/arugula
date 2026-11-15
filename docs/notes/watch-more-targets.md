---
section: Added
title: Watch sessions, threads, runs and deploys
line: An agent's watch can also wait for a hosted session to settle, a reply in a chat thread, or a run or deploy to end.
needs: control
headline: no
---
`watch create` takes `session` (a hosted agent session, `ses_…`, until it settles: done with nothing queued, waiting for input, failed or stopped), `run` (a run, `run_…`, or a deploy, `dep_…`, until it ends) and `room` (a chat room's id, with `parent` for one thread and `from` for one person's reply) beside `pr`. Any of them fires the watch, and the message names ids and states only: never a reply's text. Arugula cloud checks again when the watch fires that you can still read what it watched.
