---
section: Changed
title: Browser sign-in for the CLI is opt-in for now
line: "`arugula login` makes the CLI one of your devices again, approved from one you have; `arugula login --browser` signs it in from a browser with no key."
needs: control
---
`arugula login` is back to making the CLI one of your devices, approved with a code from a device you already have. Signing in from a browser, new in 0.70.7, is now `arugula login --browser`. It stays opt-in until Arugula cloud can tell a CLI's session from a browser page's. Either way, `--host NAME` reaches machines with short-lived machine tokens first. A CLI already signed in from a browser keeps working.
