---
schema: 1
id: "aru-120"
title: "A shared Doc's page loads its fonts from its own origin"
state: "decided"
area: "Federation and control"
source:
  via: "cli"
  issue: "arugula-salad/arugula#1064"
question: "The share viewer's CSP (#1064, decision 5) has no font-src, so a shared Doc can't load the faces a Doc wears since #253. Does the viewer's CSP allow fonts, and from where?"
options:
  - id: "a"
    label: "font-src 'self': the build's own woff2 files, nothing else"
    how: "The viewer's CSP (crates/control/src/doc_links.rs) gains font-src 'self'. doc-view.css imports shared/fonts.css, whose Fontsource files Vite copies into the build's assets/, which the viewer already serves. Every other directive stays as decision 5 wrote it."
    tradeoff: "One more kind of request from the page, to its own origin, for files of the same build: no third party, no data: or blob: fonts."
  - id: "b"
    label: "Keep the CSP as it was: system fallbacks on shared links"
    how: "Nothing changes; a shared Doc draws in Arial Narrow and system-ui, unlike the same Doc in the app."
    tradeoff: "A shared link, the place a stranger first sees a Doc, looks less finished than the block."
  - id: "c"
    label: "Inline the fonts as data: URLs in the CSS"
    how: "font-src data:, with the woff2 files base64'd into the page's stylesheet."
    tradeoff: "A stylesheet about 100 KB bigger for every link, and data: fonts allowed where nothing needs them."
choice:
  option: "a"
  reason: "#252 decided a Doc wears its own Arugula look everywhere it's drawn, the shared viewer included (Aaron, 2026-10-27), and #255 asked for this in its comment from #253 (PR #1593). The fonts are the build's own files on the page's own origin, so the page still reaches nothing it didn't before."
rejected:
  - option: "b"
    why: "It leaves the share link without the look #252 decided on."
  - option: "c"
    why: "Bigger for every reader, and a wider source than 'self' for no gain."
supersedes: []
evidence:
  - title: "#1064"
    url: "https://github.com/arugula-salad/arugula/issues/1064"
  - title: "PR #1593 (#253)"
    url: "https://github.com/arugula-salad/arugula/pull/1593"
decided_by: "alecraso"
decided_on: "2026-10-27"
reviews: []
constrains:
  - "path:crates/control/src/doc_links.rs"
  - "path:web/src/shell/doc-view.css"
  - "path:web/doc.html"
---

# A shared Doc's page loads its fonts from its own origin

This amends decision 5 of #1064 (the viewer page and its CSP). A Doc
wears its own Arugula look wherever it's drawn (#252, #253), in Big
Shoulders Display, Instrument Sans and Arugula Mono (`shared/fonts.css`).
Decision 5's CSP had no `font-src`, so `default-src 'none'` refused those
fonts on a shared link, and the Doc fell back to Arial Narrow and
system-ui.

- **The CSP** is decision 5's with `font-src 'self'` added:
  `default-src 'none'; script-src 'self'; style-src 'self'
  'unsafe-inline'; img-src data:; font-src 'self'; connect-src 'self';
  worker-src 'none'; base-uri 'none'; form-action 'none';
  frame-ancestors 'none'`. Nothing else in it changes.
- **The fonts** are Fontsource's woff2 files, from npm, which Vite copies
  into the build's `assets/`. The viewer already serves `assets/` on the
  docs host, and control's origin serves it under `/d/`, so `'self'` covers
  both, and no font file is committed.
- **Still nothing else:** the page reaches no third party, and no data: or
  blob: font. Its icon (the leaf, #255) is a data: URL, which `img-src
  data:` allowed already.
- **The link preview** (#255) changes nothing here. Its tags are in the
  page's head, and its card is `/card.png` on the viewer, which only a
  preview bot fetches. The page itself never loads it.
