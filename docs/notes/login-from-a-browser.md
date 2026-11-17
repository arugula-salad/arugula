---
section: Changed
title: arugula login signs in from a browser
line: Plain `arugula login` prints a link to click Allow on in a browser that's signed in; `--device` joins the terminal as a device, as before.
---
Since browsers stopped holding device keys, a new account had nothing to approve the code `arugula login` showed. Now `arugula login` signs in the way `--browser` did, and Arugula cloud gives the CLI a session of its own kind: it reaches your machines and does what the CLI does, but can't approve sign-ins, devices, joins or deploys, add passkeys or change a team's members, since an agent on that machine can read its file. The Allow page says whether the CLI or the desktop app is asking. `arugula login --device` keeps the old way, and an Arugula cloud too old for a CLI's session gets it too, with a message saying so. `--browser` still works.
