---
schema: 1
id: "aru-129"
title: "A Doc's prototype runs in a frame on control's proto origin"
state: "decided"
area: "Federation and control"
source:
  via: "cli"
  issue: "arugula-salad/arugula#506"
question: "A ```prototype fence in a Doc runs its own inline scripts in a sandboxed frame. A srcdoc frame inherits its parent page's CSP, so under control's platform pages and the shared Doc viewer (script-src 'self') its scripts never run. Where does the frame load from there, and who may run one?"
options:
  - id: "a"
    label: "A dedicated origin on control, proto.<block domain>, serving one static runner page under its own CSP"
    how: "Host::Proto (crates/control/src/blocks.rs) serves web/src/shared/ui/runner.html at / and 404s everything else, with sandbox allow-scripts; default-src 'none'; script-src 'unsafe-inline'; style-src 'unsafe-inline'; img-src data: blob:; font-src data:; base-uri 'none'; form-action 'none'; frame-ancestors <control's sites, the platform page, the docs viewer>. The parents gain only frame-src <proto origin>. The same page is the frame's srcdoc where the page has no script CSP (the daemon's own app). On a shared link nothing is framed until the reader clicks Run prototype."
    tradeoff: "Needs a block domain; with none the section shows a one-line note on control's pages. One more host for control to route."
  - id: "b"
    label: "Loosen the parents: script-src 'unsafe-inline' for the frame"
    how: "Allow inline scripts in the platform page's and the viewer's CSP so srcdoc frames run."
    tradeoff: "It lets inline script run in the page itself, which is the one thing those CSPs exist to stop."
  - id: "c"
    label: "blob: or data: frames"
    how: "Load the page from a blob: or data: URL."
    tradeoff: "The parents' frame-src would have to allow blob: or data:, and the frame still inherits script-src in some engines."
choice:
  option: "a"
  reason: "Tested in Chromium and WebKit (#506): a frame from a different origin, named in the parent's frame-src and carrying its own CSP header, runs its inline scripts; srcdoc, blob: and data: frames don't under script-src 'self'. The parents change by one frame-src, and nothing about their scripts. The owner settled the fence name (prototype, so html fences stay code), the proto origin and click to run on shared links."
rejected:
  - option: "b"
    why: "It weakens the page that holds the account's session to run a stranger's prototype."
  - option: "c"
    why: "Blocked by the parents' frame-src today, and no better isolated."
supersedes: []
evidence:
  - title: "#506"
    url: "https://github.com/arugula-salad/arugula/issues/506"
decided_by: "alecraso"
decided_on: "2026-11-01"
reviews: []
constrains:
  - "path:crates/control/src/blocks.rs"
  - "path:crates/control/src/doc_links.rs"
  - "path:crates/platform-api/src/page.rs"
  - "path:web/src/shared/ui/runner.html"
  - "path:web/src/shared/ui/prototype-frame.tsx"
---

# A Doc's prototype runs in a frame on control's proto origin

A ```prototype fence in a Doc section is a page with its own inline
scripts and styles, drawn in `<iframe sandbox="allow-scripts">`. An
```html fence stays a code block. There is never `allow-same-origin`,
`allow-top-navigation*`, `allow-popups`, `allow-forms` or `allow-modals`,
so the frame's origin is opaque: no cookies or storage, no reaching the
page above, the machine or the account, and no moving the window. The
iframe also has `referrerpolicy="no-referrer"`, `allow=""` and
`loading="lazy"`, and a "Prototype · sandboxed" label on its edge.

- **One runner page, two ways to load it.** `runner.html` is static. It
  posts `arugula.ready` to its parent, waits for one `arugula.html`
  message from it, then replaces itself with the prototype by
  `document.open/write/close`, the frame CSP (`default-src 'none';
  script-src 'unsafe-inline'; style-src 'unsafe-inline'; img-src data:
  blob:; font-src data:`) and the Doc's `color-scheme` written first. It
  takes the message only from `parent`, once; the parent sends it only to
  the frame it made, once, and only after `ready`.
- **Where the page has no script CSP** (the daemon's own app, the desktop
  app), the frame is that page by `srcdoc`, and it runs at once.
- **Where it has** (control's platform pages, `script-src 'self'`, and the
  shared viewer on `docs.<block domain>`), a srcdoc frame inherits the
  parent's CSP and its scripts don't run. The frame loads the same page
  from `proto.<block domain>`: `Host::Proto`, which serves it at `/` and
  nothing else (a 404 for any other path), sets no cookie, and carries the
  CSP in the first option. Its `sandbox` directive keeps it opaque even
  opened on its own.
- **A shared link is click to run.** On the viewer nothing is framed, and
  the proto origin is not asked for anything, until the reader clicks
  "Run prototype" on a card with the title. In the Doc block and on
  project Docs it runs at once.
- **A prototype that navigates itself** (the frame loads a third time) is
  replaced by "This prototype tried to leave the page. Reload it." and a
  Reload button.
- **The parents change by one directive.** The platform page's
  `frame-src 'none'` becomes `frame-src <proto origin>`, and the viewer's
  CSP gains the same `frame-src`. Nothing else in either moves. Control
  says where the proto origin is the way it says where the hub is: the
  platform host's answers carry `X-Arugula-Proto`, and the viewer derives
  it from its own host (`docs.` becomes `proto.`).
- **No block domain, no proto origin.** The section shows "Prototypes need
  a block domain on this page." on those pages.
- **Fonts** are `data:` only. A font host is #508's to decide.
