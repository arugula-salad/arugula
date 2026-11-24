---
section: Changed
title: Machines reach each other with control's tokens
line: "Delegating to another machine's agents, and driving a cloud workspace from your machine, work with a machine joined by a join token."
headline: no
---
A daemon now asks control for a machine token to reach another machine (Arugula #1002, aru-141): `agents` lets it call that machine's agents (`/api/a2a/` only), and `drives` gives it full use of a cloud workspace of its project. Before, only a device certificate opened that channel, so a machine joined with a join token couldn't be reached or reach others. Where control doesn't make these tokens yet, or the other machine runs a daemon from before them, the daemon uses its device key as before. A daemon older than this one refuses another machine's token.
