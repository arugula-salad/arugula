---
section: Changed
title: Every browser reaches machines with tokens
line: Your browser opens your machines with a short-lived token from Arugula cloud, even on a computer you approved before.
headline: no
---
Since 0.70.4 a machine takes a machine token from Arugula cloud (aru-141), and a browser that isn't one of your devices already used them. Now every browser does. A browser that has a device key still uses it when a machine is too old to take a token, or when the control it signed in to gives none (a dev or test control). `localStorage["arugula.machine-tokens"] = "no-key"` brings back the old default for one browser, and `"off"` turns tokens off.
