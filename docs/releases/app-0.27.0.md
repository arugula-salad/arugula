# Arugula app 0.27.0

- **Signing in needs no code** (#758). One *Allow* from a browser that's
  already one of your devices signs the app in and makes it a device. The
  "Approve this app as a device" screen only comes up as a fallback.
- **Restarts into an installed update once it's idle** (#561). After an
  update installs, the app restarts itself when no window of it has had
  focus for 5 minutes (and, on macOS, nobody has typed or moved the mouse
  for 5 minutes), no huddle is live and no dialog or file picker is open.
  Windows come back on the pages they showed, without taking focus. Your
  panes live in the daemon, so nothing is lost. macOS and the Linux
  AppImage; on Windows the installer already closes the app.
  *Restart to update* in the tray is still there.
- **An AppImage with its own profile names itself "Arugula app (AppImage)
  on <host>"** (#817), so it's told apart from the .deb in Devices and
  machines.

It carries arugulad 0.37.0.
