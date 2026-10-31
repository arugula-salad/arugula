---
title: "arugulad isn't running after a reboot"
description: "On Linux lingering is off; on a Mac you only reach over ssh, nobody logged in to start it."
area: "Install and updates"
---

# arugulad isn't running after a reboot

## You see

The machine is back, but its daemon isn't: the machine shows as offline in
control and on your other machines, and `arugula status` on it says the
service isn't running. When it was installed, you may have seen one of
these:

```
note: lingering is off, so it starts at login, not boot: `loginctl enable-linger $USER`
```

```
arugula: arugulad on box stops when you log out: lingering couldn't be turned on without sudo (`sudo loginctl enable-linger $USER` there keeps it)
```

```
note: ada has no GUI login on this Mac (only ssh), so arugulad runs as a background agent: it keeps running after you log out, but after a reboot it won't start until ada logs in to the desktop or runs `arugulad install` again. `arugulad install --system` starts it at boot instead (a LaunchDaemon; needs sudo).
```

## Why

- **Linux:** `arugulad install` sets up a systemd *user* service. systemd
  starts a user's services when they log in, unless lingering is on for
  them (`crates/daemon/src/install.rs`, which checks `loginctl show-user`
  and prints the note).
- **macOS reached only over ssh:** with no desktop login, the daemon runs
  as a background agent, which launchd starts again only when you log in
  (`crates/daemon/src/install.rs`, `background_note`).
- **Linux without systemd:** `arugula --ssh` starts the daemon detached,
  which nothing restarts after a reboot (`crates/cli/src/ssh.rs`).

## Do

- **Linux:** `loginctl enable-linger $USER` (with `sudo` if it asks).
  `arugulad install --system` doesn't apply on Linux: lingering is what
  starts it at boot.
- **macOS:** `arugulad install --system`, which runs it as you from boot
  in a LaunchDaemon (it runs `sudo`). Run it as yourself, not as root.
- **No systemd:** start it from whatever starts things at boot there, or
  run `arugula --ssh` to it again after a reboot.
