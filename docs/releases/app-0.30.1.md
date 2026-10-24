# Arugula app 0.30.1

- **Passkeys in the window on macOS** (#1460). The app can now sign you in
  with a passkey and approve deploys with Touch ID in its own window,
  instead of sending you to your browser. macOS allows it because the app
  is signed with the associated domain `control.arugula.io` (a Developer
  ID provisioning profile), and control vouches for the app. Use a passkey
  in iCloud Keychain or on your phone: one kept only in Chrome's password
  manager isn't offered to apps. Linux and Windows are unchanged.

- **Notifications from control, natively** (#1461). In the app, **Notify
  this device** now turns on the app's own notifications for everything
  control would push to that device: chat, panes on any machine, inbox,
  deploys, runs, workspaces and account notices, filtered by your Push
  settings. A click opens the right place. The app holds the stream
  itself, so it keeps working with every window closed on macOS. Nothing
  arrives while the app is quit, and a pane on this machine never
  notifies twice. Needs control 0.60.17.

It carries arugulad 0.60.17.
