# Arugula app 0.32.2

- **Escape never takes the app out of full screen** (#1883, #392). An
  Escape nothing on the page handles, such as one in chat with nothing
  open, no longer drops the window out of full screen. Leave full screen
  with the green button, ⌃⌘F or the View menu, as in Safari.
- **Windows: approve a deploy with Windows Hello in the app** (#1951,
  #386). The app now tells its pages it runs on Windows, not Linux, so
  the deploy page approves with a passkey in the app's window. Linux
  still hands it to your browser.

It carries the newest arugulad release.
