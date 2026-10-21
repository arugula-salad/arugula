# Arugula app 0.30.0

- **The platform page stays in the app** (M157): control's `/control.json`
  says where it is (`app.arugula.io` for the hosted one), and the app keeps
  that exact origin in its window, with its window controls working on
  Linux, as control's own page has them. Anywhere else still opens in your
  browser. Older apps open app.arugula.io links in your browser instead of
  the app window; update the app to keep the platform page in the window.

It carries arugulad 0.56.0.
