---
title: "The page says \"This machine's Arugula needs you to sign in from it\""
description: "Opening 127.0.0.1:7681 in a browser by hand: the daemon wants its local token."
area: "Web and desktop"
---

# The page says "This machine's Arugula needs you to sign in from it"

## You see

You opened `http://127.0.0.1:7681` (or `localhost:7681`) in a browser on
the machine, and got a page that says:

```
This machine's Arugula needs you to sign in from it.

Run this in a terminal here, which opens the page signed in:

arugula web
```

## Why

On the machine itself the daemon doesn't take "it came from localhost" as
proof it's you: any program, or any page in your browser, can reach
localhost. It wants its local token, kept in `local-token` in its state
directory, or the key the page it opened was given
(`crates/daemon/src/access.rs`, `check_local`). A browser tab you opened
by hand has neither.

## Do

- `arugula web` opens the page signed in. `arugula web --print` prints the
  address instead, to open where you like.
- Or use the desktop app, which signs in for you.
- A script sends the token in `local-token` as `Authorization: Bearer`.
