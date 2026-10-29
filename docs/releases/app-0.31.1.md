# Arugula app 0.31.1

- **The camera for People blocks** (#1807). On macOS, *Turn on my camera*
  in a People block said the camera wasn't allowed: the app had only ever
  asked for the microphone. It now asks macOS for the camera too (once,
  the first time), so your avatar or outline, and your video in voice or
  a call, work in the app. Only Arugula's own pages get the camera and
  microphone; any other page is refused both.

- **One window for the hotkey** (#1723). Pressing the hotkey before the
  app finished starting no longer opens a second window, and it hides the
  window reliably.

Needs control 0.65.2 (People blocks). It carries arugulad 0.65.2.
