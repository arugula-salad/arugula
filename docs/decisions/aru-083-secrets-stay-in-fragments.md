---
schema: 1
id: "aru-083"
title: "Secrets stay in fragments"
state: "decided"
area: "Clients"
source:
  via: "cli"
  issue: "arugula-salad/arugula#1172"
question: "When the platform's hash routes become paths, which fragments move with them?"
options:
  - id: "a"
    label: "Only the platform's hash routes become paths; every secret-bearing fragment stays a fragment in the shell"
    how: "#link=, #join=, #device=, invites and doc keys stay fragments. The shell's old #chat, #cloud and #cloud-ws routes redirect to app.arugula.io forever, and a test says the secret-bearing fragments are untouched."
    tradeoff: "The shell keeps some hash routing, listed in one place with why each stays."
  - id: "b"
    label: "Move every route to paths"
    how: "One routing style everywhere."
    tradeoff: "Paths reach servers, logs and Referer headers; a fragment never leaves the browser."
choice:
  option: "a"
  reason: "Fragments never reach a server, which is the point of putting secrets there (Jake, 2026-10-20, #1171)."
rejected:
  - option: "b"
    why: "Sends secrets to servers and logs."
supersedes: []
evidence:
  - title: "#1171"
    url: "https://github.com/arugula-salad/arugula/issues/1171"
  - title: "#1172"
    url: "https://github.com/arugula-salad/arugula/issues/1172"
  - title: "#1180"
    url: "https://github.com/arugula-salad/arugula/issues/1180"
decided_by: "jhgaylor"
decided_on: "2026-10-20"
reviews: []
constrains:
  - "path:web/src/shell"
---

# Secrets stay in fragments

Secret-bearing fragments stay fragments (Jake, 2026-10-20, #1171):
`#link=`, `#join=`, `#device=`, invites and doc keys never become paths,
because a fragment never reaches a server, a log or a Referer header. Only
the platform's hash routes become paths on `app.arugula.io` ("Platform URLs
are ids with cosmetic slugs"). The shell's old `#chat`, `#cloud` and
`#cloud-ws` routes redirect to the matching platform URL, forever, so old
notifications, bookmarks and messages from old daemons still land (M154).
The shell's remaining hash listeners are listed in one place, each with why
it stays a fragment.
Where: `web/src/shell/`.
From: #1171, M146 (#1172), M154 (#1180).
