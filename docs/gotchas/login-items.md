---
title: "The desktop app says \"allow Arugula in … Login Items\""
description: "macOS holds back the app's daemon until you approve it as a login item."
area: "Web and desktop"
---

# The desktop app says "allow Arugula in … Login Items"

## You see

The desktop app on a Mac doesn't start its daemon, and says:

```
macOS wants you to allow Arugula in System Settings > General > Login Items before its daemon runs.
```

## Why

The app runs its daemon as a launch agent it registers with macOS
(`crates/desktop/src/service.rs`, `register`). macOS asks you to approve
a new background item before it runs. It may also have been turned off
under *Allow in the Background*.

## Do

Open **System Settings > General > Login Items**, turn Arugula on under
*Allow in the Background*, then start the daemon from the app's *Daemon*
menu.
