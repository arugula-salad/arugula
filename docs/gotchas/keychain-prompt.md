---
title: "macOS asks for a keychain password for \"Arugula WebCrypto Master Key\""
description: "A Mac that once ran an app before 0.27.4 asks once: Always Allow, and it doesn't come back."
area: "Web and desktop"
---

# macOS asks for a keychain password for "Arugula WebCrypto Master Key"

## You see

After updating the desktop app, a macOS dialog:

> Arugula wants to use your confidential information stored in "Arugula
> WebCrypto Master Key" in your keychain.

## Why

The item isn't one Arugula writes. WebKit makes it in your login keychain
to protect the keys a page keeps, here the device key. Its access list
names the build that made it. Desktop apps up to 0.27.0 weren't signed
with a Developer ID, so on a Mac that ran one, the item names that build,
and the first signed build to read it asks. Every release since 0.27.4 is
signed with the same Developer ID, so once you allow it, later updates
don't ask (`crates/desktop/README.md`, "A keychain prompt after an
update").

## Do

- Type your login keychain's password (usually your Mac's login password;
  after a password change outside System Settings, maybe the old one) and
  click **Always Allow**. It may ask a second time: **Always Allow**
  again. It doesn't come back.
- **Deny** leaves the app unable to read its keys, and it asks again at
  the next launch.
- If you don't know the password, delete the item in Keychain Access,
  then sign in to control again and approve this device again.
