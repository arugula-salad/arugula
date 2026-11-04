---
title: "arugulad update says \"this install updates with: …\""
description: "Homebrew and source builds don't update themselves; the message gives the command that does."
area: "Install and updates"
---

# arugulad update says "this install updates with: …"

## You see

```
$ arugulad update
Error: this install updates with: brew upgrade arugula && arugulad install
```

or ``… with: git pull, build it, and run `arugulad install` again``. In the
web client, the update notice has no button, and asking for one says
`can't update from here:` and one of:

- `there's no release build for this platform`
- `this install updates another way (see the command)`
- ``this daemon isn't the service `arugulad install` set up for you``

## Why

`arugulad update` only replaces a daemon the install script or the desktop
app put there. A Homebrew install belongs to brew, and a build from source
to you (`crates/daemon/src/update.rs`, `other_command` and
`updates_itself`). It also only restarts the service `arugulad install`
set up, not a daemon you started by hand
(`crates/daemon/src/selfupdate.rs`, `refusal`).

## Do

Run the command the message gives. For a daemon you started by hand,
`arugulad install` makes it a service, and later updates work from the
button.
