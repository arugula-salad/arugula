---
section: Changed
title: A new browser works without being approved
line: Signed in on a new browser, you reach your machines at once with a token from Arugula cloud, instead of waiting for another of your devices to approve it.
needs: control
---
A browser that isn't one of your account's devices no longer waits on *Approve this browser*: it lists your machines from Arugula cloud's directory and opens each one with a short-lived machine token (aru-141), fetched fresh for every connection. A browser that is a device keeps using its key. Machines need this release's daemon to take a token; an older one says to update it. Blocks shown through Arugula cloud (ports, editors), approving other devices and machines' joins, and team changes still need a device for now.
