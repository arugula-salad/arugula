---
section: Added
title: Watch-only remote access, and a log of who came
line: "`arugulad sharing remote-input off` lets people who reach this machine through control watch but not type, and each visit is logged on the machine."
headline: no
---
Control is moving to deciding who reaches a machine (aru-141), so each machine gets two guards of its own. `arugulad sharing remote-input off`, run on the machine, makes everyone who comes through control a viewer: a channel that could type closes within two seconds and comes back as a viewer's. Only the machine sets it; nothing through control can turn it back on. And every channel opened to the machine is written to `remote-sessions.jsonl` in its state directory, with who it was, what they could do and how long they stayed.
