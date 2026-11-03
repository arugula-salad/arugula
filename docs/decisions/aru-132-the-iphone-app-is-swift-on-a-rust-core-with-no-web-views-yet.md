---
schema: 1
id: "aru-132"
title: "The iPhone app is Swift on a Rust core, with no web views yet"
state: "decided"
area: "Clients"
source:
  via: "cli"
question: "What does the iPhone app build natively, what does it share, and what stays web?"
options:
  - id: "a"
    label: "Rust for signing, channels and the terminal, Swift for screens, web views later"
    how: "`native/core` holds what must match the daemons byte for byte. The first-scope screens are Swift. A block that's a web page by nature waits, or opens in Safari, until it needs a web view."
    tradeoff: "The terminal's selection, scrollback, search, links and accessibility have to be built for iOS, where xterm.js gives the web client them for free."
  - id: "b"
    label: "Swift only"
    how: "CryptoKit signs requests, which is all projects, issues, boards and chat need. No Rust on the phone."
    tradeoff: "Reaching a machine would need a third copy of the Noise channel and pane stream, beside Rust's and the web client's TypeScript, and a terminal engine that isn't the daemon's."
  - id: "c"
    label: "A web view for the terminal"
    how: "The web client's xterm.js in a web view, with selection and the rest already built."
    tradeoff: "The encrypted channel ends where the keys are, so a page would hold the device's keys, against aru-131, or the core would feed it decrypted output."
choice:
  option: "a"
  reason: "A mistake in the wire code breaks security or connections, so it stays one implementation; the screens are where native feel matters and they're cheap (Jake, 2026-11-03, Arugula native #601)."
rejected:
  - option: "b"
    why: "Fine until the app reaches a machine, which #603 does. It's the fallback if the core had failed on iOS, and it didn't."
  - option: "c"
    why: "It puts the app's keys, or its decrypted terminals, back in a page."
supersedes: []
evidence:
  - title: "S26: how native can the desktop app get"
    url: "https://github.com/arugula-salad/arugula/blob/main/spikes/s26-native/README.md"
  - title: "S31: the iPhone can only be a client"
    url: "https://github.com/arugula-salad/arugula/blob/main/spikes/s31-mobile/README.md"
  - title: "Arugula native #601 (this record)"
    url: "https://app.arugula.io/issues/iss_07004f8da0cbd25d94697db043271e7b"
  - title: "Arugula native #633 (web views, later)"
    url: "https://app.arugula.io/issues/iss_6b0dfdc3ab1542ed9c02be2f2cd3eaee"
decided_by: "jhgaylor"
decided_on: "2026-11-03"
reviews: []
constrains:
  - "aru-131"
  - "path:native/core"
---

# The iPhone app is Swift on a Rust core, with no web views yet

The first native app shares Rust under native screens (Jake, 2026-11-03,
Arugula native #601).

- **Rust, in `native/core`.** Signing a request, the encrypted channel to a
  daemon, a pane's stream and the terminal, on the daemon's own libghostty pin
  (a test there checks it matches `crates/vt`).
- **Swift.** The first scope's screens: chat, projects, issues, boards,
  sessions and approvals.
- **Web views, later.** Diff, editor, browser and app blocks are web pages by
  nature. They need an embed mode and a web session from a signed device, and
  neither exists yet (#633), so the first builds have none.
- **What this rests on.** S26 was a desktop spike, so its speed numbers come
  from a Mac and a Linux machine, not a phone. This is a choice for the iPhone,
  not a measured result there. What was measured on iOS: the core builds, and
  its smoke test runs on a simulator (`just native-ios`).
- **Where the crate lives.** Not under `crates/`: the root `Cargo.toml` and
  every `crates/*/Cargo.toml` are hub inputs (`scripts/ci-hub-changed`), and a
  hub deploy drops every relayed terminal. `native/core` is a workspace of its
  own, so it needs no change to either.

Where: `native/core`, `justfile` (`native-check`, `native-ios`).
From: Arugula native #601, the Native iPhone app board.
