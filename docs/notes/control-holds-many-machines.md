---
section: Fixed
title: Control keeps up with many more machines
line: Arugula control stays quick as machines join: what every machine and page looks up no longer reads every record, and it keeps relaying while it writes.
headline: no
---
The load test of control (#530) found that every signed request from a
machine, and every page's list of machines, read whole tables: at 5,000
machines a page's calls took seconds. They're found by an index now
(at 5,000 machines, 17 ms). Control also keeps more threads than cores, so
a write waiting on the disk no longer holds up the relay, and it records
when a machine was last seen once a minute instead of every 15 seconds.
