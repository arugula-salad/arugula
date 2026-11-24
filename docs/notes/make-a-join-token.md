---
section: Added
title: Make a join token from settings or the CLI
line: "A project's settings make a join token under Machines, and `arugula join-token` prints one, so `arugulad join --token \"$(arugula join-token)\"` adds a machine with no browser."
---
A project's token is for its team, or for you on a personal project; `arugula join-token --team ID` makes a team's. Each is good once, until it runs out, and shown only once. A team's owners and editors make them.
