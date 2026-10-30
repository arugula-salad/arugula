# Arugula app 0.32.0

- **Links leave the app only as web links** (#1826). A link in a page, a
  frame or a popup opens in your browser only when it is `http` or
  `https`; any other kind of link stays put. The daemon's log still opens
  from the tray, as a file.

- **Signing in on this computer** (#1830). The app keeps opening the
  daemon's page signed in; with 0.66.0 the page keeps its own key instead
  of a cookie.

It carries arugulad 0.66.0.
