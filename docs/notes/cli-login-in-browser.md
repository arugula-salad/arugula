---
section: Changed
title: The CLI signs in from your browser
line: "`arugula login` prints a link: click Allow in a browser that's signed in, and the CLI reaches your machines with tokens from Arugula cloud, with no device to approve."
needs: control
---
`arugula login` no longer makes the CLI a device that another of your devices approves. It prints a link to Arugula cloud; click Allow there in a browser that's signed in, and the CLI is signed in with a session, as the desktop app is. If the browser is on another computer, paste the address it lands on into the CLI. `--host NAME` then opens each machine with a short-lived machine token (aru-141) and a key made for that connection. A CLI logged in before keeps working: it uses tokens too, and its key only where Arugula cloud gives none or a machine's daemon is older than 0.70.4. `arugula login --device` still makes a device the old way.
