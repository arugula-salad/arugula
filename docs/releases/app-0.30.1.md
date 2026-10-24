# Arugula app 0.30.1

- **Passkeys in the window on macOS** (#1460). The app can now sign you in
  with a passkey and approve deploys with Touch ID in its own window,
  instead of sending you to your browser. macOS allows it because the app
  is signed with the associated domain `control.arugula.io` (a Developer
  ID provisioning profile), and control vouches for the app. Use a passkey
  in iCloud Keychain or on your phone: one kept only in Chrome's password
  manager isn't offered to apps. Linux and Windows are unchanged.

It carries arugulad 0.60.16.
