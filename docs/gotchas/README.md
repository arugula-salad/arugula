# Gotchas

<!-- Written by scripts/gotchas-index.mjs (`just gotchas`) from the pages'
front matter. Edit the pages, not this file. -->

Sharp edges people have hit, one page each, found by what you see. Each page
has three parts: **You see** (the symptom, as it shows on screen or in a
terminal), **Why** (the cause, so you can tell whether it's yours) and **Do**.

A new one is a page in this folder with a `title`, a one-line
`description` and an `area` (one of the headings below) in its front
matter, and the three parts. Run `just gotchas` to rewrite this index;
`just check` fails while it's stale.

What the daemon sends off your machine is a page of its own:
[What leaves your machine](../what-leaves-your-machine.md).

## Install and updates

- [arugula says "can't reach arugulad … (is it running?)"](daemon-not-running.md): The CLI can't find the daemon's socket: it's stopped, or it keeps its state somewhere else.
- [arugulad --version says "arugulad-base 0.0.0"](update-says-0-0-0.md): 0.64.0's daemon misreported its version, so updating to it failed; 0.64.1 fixed both.
- [arugulad install keeps flags you meant to drop](install-keeps-old-flags.md): A plain install reuses the daemon arguments the last install wrote.
- [arugulad isn't running after a reboot](gone-after-reboot.md): On Linux lingering is off; on a Mac you only reach over ssh, nobody logged in to start it.
- [arugulad update says "this install updates with: …"](update-another-way.md): Homebrew and source builds don't update themselves; the message gives the command that does.
- [arugulad won't start: "can't listen on 127.0.0.1:7681"](port-in-use.md): Another daemon, or another program, already has the port.

## Web and desktop

- [macOS asks for a keychain password for "Arugula WebCrypto Master Key"](keychain-prompt.md): A Mac that once ran an app before 0.27.4 asks once: Always Allow, and it doesn't come back.
- [Over Tailscale: "This machine's owner is …, and nothing on it is shared with you"](tailnet-not-owner.md): The daemon's owner isn't the Tailscale login you're using, or no owner is set.
- [The desktop app says "allow Arugula in … Login Items"](login-items.md): macOS holds back the app's daemon until you approve it as a login item.
- [The macOS desktop app gets slower each day it runs](desktop-slow-notifications.md): Before app 0.31.0, every notification left a timer behind: clicks took seconds.
- [The page says "This machine's Arugula needs you to sign in from it"](sign-in-from-it.md): Opening 127.0.0.1:7681 in a browser by hand: the daemon wants its local token.
- [The page says unknown host "…" (421)](unknown-host.md): You reached the daemon by a name it wasn't told about: a LAN address, your own DNS name, a proxy.

## Control and sign-in

- ["Link GitHub in a browser or on a phone: the desktop app's window doesn't"](link-github-desktop.md): Linking GitHub to your account is never done in the desktop app's window.
- ["this CLI isn't one of the account's devices any more (revoked?)"](cli-revoked.md): The CLI's device was removed from your account; log out and log in again.

## Agents and MCP

- [An agent block won't start: "agent blocks run it through it"](agent-adapter-missing.md): The agent's ACP adapter, or Node, isn't installed on the machine the block runs on.
- [Claude Code lists Arugula's MCP tools twice](mcp-tools-twice.md): The plugin's server and one added by hand both run; remove the hand-added one.
- [Claude Code's questions stay in its terminal, not as cards](prompts-stay-in-terminal.md): The hooks only act inside an Arugula pane, with the daemon answering.

## Forges

- [A GitHub block says "no gh here" or is read-only](github-block-no-gh.md): Forge blocks use the gh login on the machine the block runs on.

## Labs

- ["… is a Labs feature that's off on this machine"](labs-feature-off.md): A Labs flag is off on the machine that answered, or the flag you set was retired.

## Hosted workspaces

- [In a Cloud workspace, a session's tool says "Permission denied"](session-permission-denied.md): The setup ran as user; sessions run as ses1–ses6, who can't enter /home/user.
