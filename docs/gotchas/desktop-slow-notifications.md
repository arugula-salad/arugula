---
title: "The macOS desktop app gets slower each day it runs"
description: "Before app 0.31.0, every notification left a timer behind: clicks took seconds."
area: "Web and desktop"
---

# The macOS desktop app gets slower each day it runs

## You see

After a day or two running, the desktop app on a Mac answers clicks and
typing seconds late. Activity Monitor shows `arugula-desktop` using a lot
of CPU on over a hundred threads, and `usernoted` busy too. Quitting and
reopening makes it fast again, for a while.

## Why

Up to app 0.30.1, each notification the app showed started a thread that
waited for the notification to be clicked, by asking Notification Center
for its list on a timer on the main thread. A notification left in
Notification Center never ends that wait, so every one added a thread and
a timer, and each timer's call got slower as the list grew (native issue
#234). App 0.31.0 handles clicks with one delegate instead
(`docs/releases/app-0.31.0.md`).

## Do

Update the desktop app to 0.31.0 or later. Until you can, clear Arugula's
notifications in Notification Center, or quit and reopen the app.
