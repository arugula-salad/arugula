---
schema: 1
id: "aru-097"
title: "Public previews live on a registrable domain of their own"
state: "decided"
area: "Federation and control"
source:
  via: "cli"
  issue: "arugula-salad/arugula#1236"
question: "Where does a hosted workspace's dev server show, for members and for strangers?"
options:
  - id: "a"
    label: "Members' previews on M50's block origins; public previews on a registrable domain of their own"
    how: "Each listening port gets a members-only b-<key> origin through the relay. A port a member marks public gets <id>.<preview domain> on a separate registrable domain, served through the relay too, per port, showing who opened it and expiring after 7 days unless renewed."
    tradeoff: "One more domain to buy, certify and keep an abuse contact for."
  - id: "b"
    label: "The provider's preview URLs"
    how: "Use E2B's host URLs."
    tradeoff: "Public by default, and they need a header browsers can't send."
  - id: "c"
    label: "Public previews under arugula.run or arugula.io"
    how: "A subdomain of a domain we already have."
    tradeoff: "A stranger's page could read or set cookies for Arugula's own sites."
choice:
  option: "a"
  reason: "Members' previews stay behind sign-in on the block origins we already have; a stranger's public page must never share a registrable domain with Arugula's cookies (Jake, 2026-10-21, #1235)."
rejected:
  - option: "b"
    why: "Never use E2B's public preview URLs."
  - option: "c"
    why: "Cookie scope is the registrable domain."
supersedes: []
evidence:
  - title: "#1235"
    url: "https://github.com/arugula-salad/arugula/issues/1235"
  - title: "#1236"
    url: "https://github.com/arugula-salad/arugula/issues/1236"
  - title: "#1247"
    url: "https://github.com/arugula-salad/arugula/issues/1247"
  - title: "#1248"
    url: "https://github.com/arugula-salad/arugula/issues/1248"
  - title: "#150"
    url: "https://github.com/arugula-salad/arugula/issues/150"
decided_by: "jhgaylor"
decided_on: "2026-10-21"
reviews: []
constrains:
  - "path:crates/control/src/blocks_wire.rs"
  - "path:crates/control/src/blocks.rs"
  - "path:crates/control/src/previews.rs"
  - "path:crates/daemon/src/public_preview.rs"
---

# Public previews live on a registrable domain of their own

A dev server in a hosted workspace shows in two ways (Jake, 2026-10-21,
#1235).

- **Members' previews** use M50's block origins (`b-<key>.arugula.run`), over
  the relay. Only the project's members, signed in, can open them.
- **Public previews** are on a registrable domain of their own (for example
  `arugula.page`; Jake picks the name when he buys it), so a stranger's page
  can't touch `arugula.run` or `arugula.io` cookies. A member makes a port
  public; it shows who did, and expires after 7 days unless renewed. They're
  served through the relay too. An abuse contact and takedown are
  documented.
- **The provider's own preview URLs are never used.** E2B's are public by
  default and need a header browsers can't send.
Where: M50's block origins: `crates/control/src/blocks.rs` relays a block's
channel to a hosted machine, and the daemon's `block_signer`
(`crates/daemon/src/control.rs`) lets in grants from the project's owners'
and editors' devices only (M186). `blocks.rs`'s test
`no_provider_preview_urls` keeps the provider's preview hosts out of the
tree. Public previews (M187): `arugula.site`, Jake's pick (bought
2026-10-22). Control (the hub) terminates HTTPS for `*.<preview domain>`
and brings each request to the box over its relay socket, on a block
channel opened with a grant signed with control's hosted key for one port
(`arugula_e2e::preview`, `crates/control/src/previews.rs`), so **control
sees a public preview's traffic in plain text**, which is acceptable because
it's public; members' previews stay end to end. The box serves it only
while the platform, asked through control at most every 20 seconds, says
that port is public under that id (`crates/daemon/src/public_preview.rs`),
so turning it off takes it down within a minute. The platform keeps who and
until when in `hosted_ports` (`crates/platform/src/hosted/previews.rs`). A
paused box isn't woken for a stranger. Abuse contact and takedown:
docs/control-ops.md, "Public previews".
From: #1235 (platform roadmap, step 5), M175 (#1236), M186 (#1247), M187 (#1248), M50 (#150).
