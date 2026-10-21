---
schema: 1
id: "aru-076"
title: "Platform paths stay on control.arugula.io forever, behind the hub's gateway"
state: "decided"
area: "Federation and control"
source:
  via: "cli"
  issue: "arugula-salad/arugula#1151"
question: "Where do the platform's routes live once it's its own process, given daemons from before 0.43 through 0.52 are in production and must keep working?"
options:
  - id: "a"
    label: "On control.arugula.io, forever: the hub is a permanent gateway and the platform has no public address"
    how: "The hub verifies the session cookie or the daemon's signature (and its replay cache), strips any principal header a client sent, and forwards the call over Fly's private network with an HMAC-signed principal header the platform checks. Signed v2 daemon calls cover method, path and query, time, nonce and the body's hash but not the host, so a route can move behind the gateway without an old client noticing."
    tradeoff: "Every platform call takes one more hop through the hub, and the hub's routing table has to name every platform path (routes.toml)."
  - id: "b"
    label: "Give the platform its own public address or domain"
    how: "Clients call the platform directly; the hub only does identity, devices and the relay."
    tradeoff: "Old daemons and apps have control.arugula.io built in and sign paths for it; every one of them would break or need a redirect they don't follow for signed calls."
choice:
  option: "a"
  reason: "The step 1 census (#1128) found daemons from before 0.43 through 0.52 in production, 3 of 13 older than 0.43; a gateway on the same paths is the whole version-skew strategy, and it needs nothing from old clients (Jake, 2026-10-20, #1150)."
rejected:
  - option: "b"
    why: "Old daemons and apps would break: they only know control.arugula.io. Never move a route an old daemon or app calls."
supersedes: []
evidence:
  - title: "#1128 (daemon version census)"
    url: "https://github.com/arugula-salad/arugula/issues/1128"
  - title: "#1150"
    url: "https://github.com/arugula-salad/arugula/issues/1150"
  - title: "#1152"
    url: "https://github.com/arugula-salad/arugula/issues/1152"
  - title: "#1157"
    url: "https://github.com/arugula-salad/arugula/issues/1157"
decided_by: "jhgaylor"
decided_on: "2026-10-20"
reviews: []
constrains:
  - "path:crates/control/src/auth.rs"
  - "path:crates/control/src/main.rs"
  - "path:crates/control/routes.toml"
---

# Platform paths stay on control.arugula.io forever, behind the hub's gateway

Platform paths stay on `control.arugula.io` forever, and the hub is a
permanent gateway in front of the platform (Jake, 2026-10-20, #1150). The
platform has no public address: it listens only on Fly's private network.

For each platform route the hub:
1. verifies the session cookie (with its same-origin check) or the daemon's
   signed `x-arugula-auth` header, including the replay cache;
2. strips any principal header the client sent;
3. forwards the call with a principal header signed with an HMAC key the
   hub and the platform share, which the platform checks and trusts alone.

That's the whole version-skew strategy. A signed v2 daemon call covers the
method, the path and query, the time, a nonce and the body's hash, but not
the host, so a route can move behind the gateway without an old client
noticing. The step 1 census (#1128) found daemons from before 0.43 through
0.52 in production, nine versions apart, and 3 of the 13 older than 0.43.
**Never move a route that an old daemon or app calls.** Every route says
which process serves it (`process` in `crates/control/routes.toml`, M135),
and the gateway is built from that list.

A page origin isn't a platform address. Since step 3 (#1171) the platform
page is on `app.arugula.io` ("The platform page is on app.arugula.io, not on
the keys' origin"), and that host points at the hub, which routes it to the
platform by `Host`. The platform still has no public address, and every
platform path on `control.arugula.io` keeps working for old daemons and apps.
Where: `crates/control/src/auth.rs` (`check_daemon`, `daemon_auth_message_v2`), `crates/control/routes.toml`, `crates/control/src/main.rs`.
From: #1150, #1128 (census), M134 (#1151), M135 (#1152), M140 (#1157).
