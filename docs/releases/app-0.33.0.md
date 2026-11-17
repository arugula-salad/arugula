# Arugula app 0.33.0

- **Chat on the Dock badge** (#2433, #498). The Dock icon now counts your
  unread mentions and DMs, added to the panes that need you, and shows a
  dot when chat has unread messages and nothing more. On Linux and Windows
  the badge shows the count. The tray still counts only the panes.

- **Pop a pane out** (#2428, #497; #2568, #1032). Pop out, in a block's pane
  menu or ⇧⌘O, opens the block alone in a pane-sized window of its own, for
  a second monitor. In the app it's always a separate window, never a tab
  of the one it came from, and from app.arugula.io it opens with just that
  pane.

- **"Arugula cloud"** (#2499, #663). The tray menu, the app's notifications
  and its description say Arugula cloud where they said Arugula control.

- **Camera and microphone prompts** (#2324). With People blocks gone, the
  app asks for the camera only to scan a QR code (approving a device,
  checking someone's key) and for the microphone only when an agent asks
  to record and you allow it.

- **The daemon it carries is checked** (#2403). The build checks the
  arugulad and arugula it carries against the daemon release's signed
  `SHA256SUMS`.

It carries the newest arugulad release.

```no-notes
#2291
```
