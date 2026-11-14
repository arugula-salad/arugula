---
section: Fixed
title: Machines open again when some are offline
line: Pages no longer use up Arugula cloud's connection limit retrying machines that are offline, which had stopped every machine from opening.
---
A page asked Arugula cloud for a new machine token every time it retried a machine, every few seconds, including machines that were offline. With a few offline machines and a few tabs open, that used up the account's 3,000 tokens an hour, and then no machine would open. Now a page:

- doesn't ask for a token for a machine the machine list shows as offline
- asks for one token per machine at a time
- when Arugula cloud says it's limiting connections, stops asking for every machine on the page, for as long as Arugula cloud says (or at least a minute, then longer), and shows "Arugula cloud is limiting connections; trying again in N s"
- when a machine's token is refused, waits a minute or more before asking for that machine again
