---
schema: 1
id: "aru-061"
title: "A passkey with PRF is a device: one wherever it syncs"
state: "decided"
area: "Federation and control"
source:
  via: "cli"
question: "A passkey with PRF is a device: one wherever it syncs"
options:
  - id: "a"
    label: "A passkey with PRF is a device: one wherever it syncs"
    how: "As the body below says."
    tradeoff: "Not recorded: DECISIONS.md kept what was chosen and why, not the options weighed."
choice:
  option: "a"
  reason: "Given in the body below, as DECISIONS.md had it."
rejected: []
supersedes: []
evidence:
  - title: "#764"
    url: "https://github.com/arugula-salad/arugula/issues/764"
  - title: "#765"
    url: "https://github.com/arugula-salad/arugula/issues/765"
decided_by: "jhgaylor"
decided_on: "2026-10-11"
reviews: []
constrains:
  - "path:web/src/shell/e2e/keys.ts"
  - "path:web/src/shell/control.ts"
  - "path:crates/control/src/passkey.rs"
  - "path:crates/control/src/api.rs"
  - "path:crates/e2e/src/cert.rs"
---

# A passkey with PRF is a device: one wherever it syncs

Where a passkey gives PRF output, the browser's device keys come from it
(two fixed salts, then HKDF), so clearing site data and signing in again,
or a new computer the passkey syncs to, is the same device with no
approval. That device signs the root it pinned once it's approved, and a
browser that derives its keys checks that signature before it takes the
root, so control still can't hand it an account of its own. A browser that
is already a working device keeps its own keys. A removed device, or one
whose signature is missing or wrong, gives way to an ordinary approval.
Why: the approval (#761) is the step people trip on, and a synced passkey
already lets its holder sign in. The cost is written down: whoever holds
the synced passkey is that device, on every machine it reaches.
Where: `web/src/shell/e2e/keys.ts` (`adoptPrfKeys`, `PRF_SALTS`), `web/src/shell/control.ts` (`takePrfKeys`, `rejoin`, `savePin`, `usePasskey`), `crates/control/src/passkey.rs`, `crates/control/src/api.rs` (`pin`), `crates/e2e/src/cert.rs` (`device_pin_body`), [docs/control-e2e.md](docs/control-e2e.md#prf-re-deriving-a-browsers-keys).
From: #764, #765.
