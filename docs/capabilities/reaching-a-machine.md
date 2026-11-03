---
id: "reaching-a-machine"
title: "Reaching a machine"
verified_through: "665cdb10ed705245af5afaf697068c829f8504b6"
entry: ["crates/control/src/relay.rs", "crates/daemon/src/e2e.rs", "web/src/shell/main.tsx", "crates/cli/src/control.rs"]
paths: ["crates/control/src/wake.rs", "crates/e2e/src/channel.rs", "crates/e2e/src/mux.rs", "crates/e2e/src/checkpoint.rs", "crates/e2e/src/frozen.rs", "crates/control/src/compat.rs", "web/src/shell/e2e/channel.ts", "web/src/shell/e2e/noise.ts", "web/src/shell/e2e/relaymux.ts", "web/src/shell/e2e/cert.ts", "web/src/shell/e2e/keys.ts", "web/src/shell/trust/gate.tsx", "web/src/shell/trust/approve.tsx"]
shared: ["crates/daemon/src/control.rs", "crates/control/src/api.rs", "crates/control/src/auth.rs", "crates/e2e/src/cert.rs", "web/src/shell/control.ts", "web/src/shell/client.ts"]
tests: ["web/e2e/control.spec.ts", "web/e2e/fleet-control.spec.ts", "web/e2e/relay-full.spec.ts", "crates/control/src/routing_wire.rs", "crates/control/src/wake_wire.rs", "crates/e2e/src/channel.rs"]
decisions: ["aru-023", "aru-054", "aru-055", "aru-056", "aru-058", "aru-059", "aru-061", "aru-062", "aru-071", "aru-075", "aru-076", "aru-083", "aru-092", "aru-095", "aru-126", "aru-131"]
---
# Reaching a machine

## What a person can do

Someone signed in at control's page, in a browser or on a phone, can use the machines their account may reach without a VPN. It takes a control account, a device approved by one the account already trusts, and a machine joined to that account or shared with it. Terminal traffic through control is end-to-end encrypted, and the relay has limits (below).

## Where it runs

Three processes are involved: the page, the hub (`crates/control`, which serves the page from its binary and runs the relay) and `arugulad`. The platform process carries no terminal bytes; the hub asks it only about hosted machines and paid plans (`PlatformPort`).

State is split. The hub's SQLite holds `devices`, `revocations`, `daemons` (with each machine's direct URLs) and `daemon_access`. A machine holds `<state>/daemon.key` and `control.json`, which has the root it pinned at join (`join_start`, `join_finish`). A browser keeps its keys and enrollment in IndexedDB (`web/src/shell/e2e/keys.ts`) and pins in `localStorage`.

Trust is checked at both ends, not by control. A daemon refreshes certificates from control but decides with `Trust::evaluate` against the pinned root, and only `Kind::connects` kinds (browser, CLI, app) get in. The page runs its own `evaluate` over `/api/devices` and `/api/directory` in `ControlSession.refresh`, and only then uses a daemon's Noise key from its certificate. Removals only add up (`crates/e2e/src/checkpoint.rs`).

A connection, traced from the page:

- `E2ELink` in `client.ts` builds the routes and `E2ESocket.connect` tries them in order. First each direct URL's `/e2e` (1.5 s to connect), then a channel inside the page's one socket to `/api/relay/m` (`RelayMux`), then `/api/relay/c/{id}` if control has no shared socket.
- The handshake is `Noise_IK_25519_AESGCM_SHA256` with the prologue from `channel::prologue`, so being spliced onto the wrong daemon fails it. `/e2e` takes no origin or tailnet identity: the handshake is the authentication (`Class::Token` in `server.rs`).
- On the relay, the hub checks `Session` (cookie and origin, or a CLI's signature) or `Reacher` (also a daemon's signature), then `teams::may_reach`. That only routes. `relay::channel` or `splice_to` opens a stream on the mux of the socket the daemon holds at `/api/relay/dial` (signed, kept by `keep_relay` and `relay_once` in `crates/daemon/src/control.rs`) and splices opaque messages.
- The daemon's `e2e::serve` reads message 1 and asks `Control::device` who the key is. An unknown key gets `Refusal::unknown_device`, which the page doesn't retry on a worse route. A known device gets `Welcome` and becomes a mux client through `Cmd::Connect`.
- Inside, `T` and `B` messages are what `/ws` carries, and `Q` requests go to the daemon's own router (`channel_router`). When a device's standing changes, `no_longer` closes its channel.

There is one channel per viewer. The hub sees who, which machine, when and sizes: `add_relay_bytes`, and a `relay session` analytics event with kind, seconds and bytes. A daemon's own page on the tailnet uses plain `/ws` instead (aru-055, `server.rs`). `arugula --host` tries direct URLs, then `/api/relay/c/{id}` (`crates/cli/src/control.rs`).

`Relay::admit` caps sockets. Defaults in `crates/control/src/lib.rs` are 32 client sockets and 50 machines per account, 5,000 sockets in all, and 2,000 MB a day per account while billing is off, past which traffic slows. With billing on, an unpaid account past twice its allowance is slowed (`plans::relay_standing`).

A hosted machine differs in two ways: `wake::reach` wakes its box when an open asks (summaries don't), and the page accepts it only under a project delegation (`acceptHosted`, aru-095).

## Compatibility

- The "illogical/1" prologue is pinned with the other signed domains (`crates/e2e/src/frozen.rs`), since both ends of every channel must agree.
- Control's page is control's build, so it can be newer than the daemon it talks to. `readWelcome` treats an empty or odd message 2 as no clock, and a control with no shared socket falls back to `/api/relay/c`.
- An older client's certificate reads are protected: `compat.rs` leaves `app` certificates out unless the request names `app-devices` or has a cookie, and aru-131 adds no new device kind for the same reason.
- An older daemon, CLI or approving page falls back to comparing the account's fingerprint when no secret or code is there (aru-058, aru-059).
- Routes an old daemon or app calls don't move, and `routes.toml` says which process serves each (aru-076).
- A machine that's off or asleep: the directory says `online: false`, opening it answers "that daemon isn't connected to the relay", and the fleet keeps its panes greyed from the last summary.
- When control is down, a daemon keeps the last certificates it had, and a page with a saved directory can still try direct URLs (`DIR_KEY`).

## Not known

- What "can't read" promises against a changed page. `docs/control-e2e.md` and `README.md` say the client at control's address is trusted. Whether that wording is the product's promise is for a person to say.
- Whether the relay's default caps are what production runs, and whether they're meant as product limits. Production's settings aren't in the code I read.
- The iPhone app. `native/core` shares the channel, but this checkout has no Swift, and `auth.rs` and `routes.toml` have no `native` client as aru-131 describes. "Phone" here means a browser with a phone's settings (`phoneContext` in `control.spec.ts`).
- The desktop app's window. I traced the page and the CLI, not `crates/desktop`.
