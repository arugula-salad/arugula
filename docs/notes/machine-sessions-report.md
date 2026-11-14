---
section: Added
title: Control hears each session's project and card
line: A joined machine tells Arugula cloud which project and card each of its sessions and agent blocks is on, so a project can list its sessions across machines.
headline: no
---
A machine now sends control the list of its sessions (`PUT /api/daemon/machine-sessions`): each session's id, name, project and card, and each pane's id, kind and card when it isn't its session's. An agent block's state report says its project and card too. Ids and names only: no terminal text, titles, file names or transcripts, and a GitHub issue isn't sent as a card. The list goes when the layout changes (or every 30 seconds if it changed otherwise), only when it differs from the last one, and whole every five minutes.

Against a control from before this, nothing is lost: a control without the route (a 404) isn't sent the list again until the daemon restarts, and a block report it refuses for its project and card (any 4xx) is sent again at once without them, and the next ones go without until every block is sent again five minutes later.
