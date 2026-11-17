---
section: Removed
title: No device approvals, recovery codes or device reset
line: Signing in is all it takes: no screen asks another device to approve this browser, and recovery codes, the device reset and passkeys as devices are gone.
needs: control
---
Since aru-141 a browser reaches your machines with short-lived tokens from Arugula cloud, so the screens that made a browser one of your devices are gone:

- **Approve this browser**, its code and the waiting screen, *New device?* on your other devices, and the screens for a browser that was turned down, lost its key or stopped being trusted. A browser your account stopped trusting works on with tokens.
- **Recovery codes**: none are made, shown or asked for.
- **Reset your devices** and its warning. The passkey reset stays, under Passkeys in *Sign-in and account*.
- **Use on new computers** for a passkey: signing in with a passkey signs you in, and no browser takes device keys from it.
- **Add a phone or browser** and the browsers in *Devices and machines*, which is now *Machines*: your machines, with Remove (on Arugula cloud's word when this browser has no device key), hosted machines and runs on cloud workspaces.

A browser that is already one of your devices keeps its key, and still approves `arugula login` and the phone app. A browser without one says how to sign the command line in instead: `arugula login --browser`, and `arugula login` says so too.
