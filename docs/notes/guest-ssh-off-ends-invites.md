---
section: Fixed
title: Turning guest SSH off ends its invites
line: Turning Guest SSH access off now also ends every ssh invite and cuts off anyone using one, and any invite can be revoked whatever the setting.
headline: no
---
Whether it's turned off in the machine's *Guest SSH access…* or with `arugulad sharing guest-ssh off`, every ssh invite ends within a second, anyone using one sees "Guest SSH access was turned off", and the guest ssh server stops listening. Turning it on again brings none of them back.

`arugula guests` and `arugula guests revoke ID` work whether the setting is on or off; only making an invite needs it on.
