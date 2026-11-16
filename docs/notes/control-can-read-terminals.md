---
section: Changed
title: Arugula cloud can read what it relays
line: Arugula cloud now decides who reaches your machines, so it can read the terminal traffic it relays (it's built to keep none of it); the pages and docs say so.
---
Arugula cloud used to promise that it relayed your terminals without being able to read them. Since machines take Arugula cloud's machine tokens (aru-141), that's no longer true, and the sign-in page, Getting started, the join and approval screens, the connection tooltips, the README and the docs now say what is.

Arugula cloud can read the terminal traffic it relays, and it can let anyone, itself included, open a shell on any machine your account reaches. So can whoever breaks into it or steals a signed-in session. It's built to keep none of the terminal bytes it relays.

What limits it: a machine token is for one machine and one role, lasts five minutes and opens one channel; a viewer's can't type. Each machine logs every remote session in `remote-sessions.jsonl` in its state directory. And `arugulad sharing remote-input off`, run on the machine, makes everyone who comes through Arugula cloud a viewer. Passkeys still approve deploys, standing run grants and owner changes, and an agent never does. A daemon's own page on your tailnet, and `arugula attach`, don't go through Arugula cloud at all. The whole of it is in `docs/control-e2e.md`, "What control can do to your machines".
