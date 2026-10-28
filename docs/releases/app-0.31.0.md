# Arugula app 0.31.0

- **Chat in a window of its own** (#1605, #1661). Chat and Projects now
  open beside your terminals instead of taking over the window, and the
  app keeps one window each for chat, Control and This machine: opening
  one again (Control from chat, say) goes to the window already showing
  it and brings it forward. *Open in a new window* still opens another.

- **Says when the window isn't signed in** (#1603). After control moved,
  or after *Just this machine*, the window showed only this machine with
  nothing to say why. Now a banner says so, with **Sign the app in**, and
  the app's sign-in page says when the app was signed in at another
  address.

- **Link GitHub works in the app** (#1587). Linking GitHub from account
  settings no longer signs the window out: the app links through GitHub's
  device code instead.

- **Notification clicks stay quick on macOS** (#1583). After a day or two
  with notices in Notification Center, the app answered clicks seconds
  late. Each notice now posts and is forgotten, and one handler opens what
  a click was for.

If macOS asks for the keychain password for "Arugula WebCrypto Master Key"
after the update, type it and choose **Always Allow**: it asks once on a
Mac that ran app 0.27.0 or older, which weren't signed with the Developer
ID (#1647, `crates/desktop/README.md`).

Needs control 0.63.8. It carries arugulad 0.64.0.
