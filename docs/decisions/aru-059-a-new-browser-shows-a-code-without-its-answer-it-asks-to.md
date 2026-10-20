---
schema: 1
id: "aru-059"
title: "A new browser shows a code; without its answer it asks to check the account"
state: "decided"
area: "Federation and control"
source:
  via: "cli"
question: "A new browser shows a code; without its answer it asks to check the account"
options:
  - id: "a"
    label: "A new browser shows a code; without its answer it asks to check the account"
    how: "As the body below says."
    tradeoff: "Not recorded: DECISIONS.md kept what was chosen and why, not the options weighed."
choice:
  option: "a"
  reason: "Given in the body below, as DECISIONS.md had it."
rejected: []
supersedes: []
evidence:
  - title: "#761"
    url: "https://github.com/arugula-salad/arugula/issues/761"
  - title: "#765"
    url: "https://github.com/arugula-salad/arugula/issues/765"
decided_by: "jhgaylor"
decided_on: "2026-10-11"
reviews: []
constrains:
  - "path:web/src/control.ts"
  - "path:web/src/ui/control.tsx"
  - "path:web/src/ui/scan.tsx"
  - "path:crates/control/src/api.rs"
---

# A new browser shows a code; without its answer it asks to check the account

A browser, phone or app window waiting to be approved shows a code
(`LOOKUP.SECRET`, as text and as a QR code of a `#device=` link) instead of
its fingerprint. The approving device scans or types it and sends a MAC under
the secret over the account, the root it pinned and the new device's keys;
the new device pins the root only when the MAC checks out. A wrong MAC is
refused and the secret dropped (one guess per code). An approval with no MAC
(a page from before codes, or `just dev-control`'s headless device) isn't
refused outright, nor taken on trust: the new browser shows the account's
fingerprint and asks the person to check it (*It matches*). The scanner uses
the browser's `BarcodeDetector`, else jsQR, loaded only then.
Why: the compare was easy to skip, and the new browser took the root on
control's word. Refusing a MAC-less approval outright would strand anyone
approving from a tab opened before the deploy.
Where: `web/src/control.ts` (`enroll`, `vouched`, `approveDevice`, `DEVICE_HASH`), `web/src/ui/control.tsx` (`Waiting`, `DevicePrompt`, `Unvouched`), `web/src/ui/scan.tsx`, `crates/control/src/api.rs` (`enroll`, `approve`, `device`), [docs/control-e2e.md](docs/control-e2e.md#approving-a-new-browser-or-phone-the-code-it-shows).
From: #761, #765.
