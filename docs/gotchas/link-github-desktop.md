---
title: "\"Link GitHub in a browser or on a phone: the desktop app's window doesn't\""
description: "Linking GitHub to your account is never done in the desktop app's window."
area: "Control and sign-in"
---

# "Link GitHub in a browser or on a phone: the desktop app's window doesn't"

## You see

In the desktop app, *Sign-in and account → GitHub → Link GitHub* shows a
page with only:

```
Link GitHub in a browser or on a phone: the desktop app's window doesn't.
```

Before desktop app 0.31.0, the same button instead dropped the window on
the app's sign-in page, as if you'd been signed out.

## Why

Linking GitHub adds a way to sign in to your account, and control won't do
that from the desktop app's window, whose session may belong to any
program on that computer (`crates/control/src/github_link.rs`, `start`).
Older apps also took the link for a sign-in and signed the window out
(native issue #235). Since daemon 0.63.8 the button opens your browser to
finish, and the window says when it's linked.

## Do

Link GitHub from a browser or a phone, signed in to the same account
(control's *Sign-in and account*). Update the desktop app to 0.31.0 or
later, and the machine's daemon to 0.63.8 or later, to get the button
that does that for you.
