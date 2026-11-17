---
section: Changed
title: No new browser becomes a device
line: Your first browser, the desktop app's window and any other new browser reach your machines with tokens from Arugula cloud; none is made one of your devices any more.
needs: control
---
Since aru-141, a browser that would have waited for another device's approval reached your machines with short-lived machine tokens instead. Now no browser becomes a device at all: not your account's first (which used to make itself the account's root and show recovery codes), not the desktop app's window after a browser allows its sign-in, and not one opened with a machine's join link (it adds the machine in one click). A browser that is already one of your devices keeps its key. Approving a command line (`arugula login`) or the phone app still needs a device: `arugula login --browser` signs the CLI in from any browser. A control that signs no tokens (a dev or test hub), or `localStorage["arugula.machine-tokens"] = "off"`, enrolls a new browser as before.
