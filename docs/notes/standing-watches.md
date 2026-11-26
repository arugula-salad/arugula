---
section: Added
title: Standing watches for agents
line: An agent's event watch can stand: it stays live after it fires, rings at most every 5 minutes with every event since, and can skip what its own session caused.
needs: control
---
`watch create` takes `standing` (an event watch alone), `every` (1m to 24h, 5m by default) and `exclude_self`. A standing watch lives until it expires (still at most 7 days) or is cancelled. The first matching event fires it at once. Events that match in the gap after that come together in its next fire, which lists every one by number. `exclude_self` skips the watching session's own agent block asking for input or going idle. A chief of staff now sets its doorbells as standing watches once, instead of making them again after every wake, and its own permission prompts no longer ring its `session.needs_input` doorbell.
