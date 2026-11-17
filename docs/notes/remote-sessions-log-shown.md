---
section: Added
title: See who reached your machine
line: "A machine's Settings page lists who reached it through Arugula cloud, with their role and when, and `arugulad sharing log` prints the same."
try: {"route": "#settings=remote-sessions"}
headline: no
---
Each machine has kept its own record of every remote session since the remote-input switch (aru-141), in `remote-sessions.jsonl`, but nothing showed it. Now the machine's Settings page has a Remote sessions section, for its owner only: one row per session, newest first, with who, the role, relayed or direct, when it opened, how long it lasted or that it's still open, and whether remote input is on. `arugulad sharing log`, on the machine, prints the same list, with the daemon running or not.
