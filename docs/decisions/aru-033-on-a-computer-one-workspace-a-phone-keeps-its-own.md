---
schema: 1
id: "aru-033"
title: "On a computer, one workspace; a phone keeps its own"
state: "decided"
area: "Clients"
source:
  via: "cli"
question: "On a computer, one workspace; a phone keeps its own"
options:
  - id: "a"
    label: "On a computer, one workspace; a phone keeps its own"
    how: "As the body below says."
    tradeoff: "Not recorded: DECISIONS.md kept what was chosen and why, not the options weighed."
choice:
  option: "a"
  reason: "Given in the body below, as DECISIONS.md had it."
rejected: []
supersedes: []
evidence:
  - title: "#1022"
    url: "https://github.com/arugula-salad/arugula/issues/1022"
  - title: "#1028"
    url: "https://github.com/arugula-salad/arugula/issues/1028"
decided_by: "jhgaylor"
decided_on: "2026-10-18"
reviews: []
constrains:
  - "path:web/src/ui/app.tsx"
  - "path:web/src/ui/shell"
  - "path:web/src/ui/panes"
  - "path:web/src/appearance.ts"
---

# On a computer, one workspace; a phone keeps its own

The desktop web client and the desktop app draw one shell: a rail, the
session sidebar, a context header, the tab strip, a pane toolbar, pane
headers and a footer (#1018). It replaced the one bar in 0.46 (M113),
after M107–M112 built it behind a switch; the switch and the bar are gone,
and a stored choice of the bar is read and dropped. A phone (`usePhone`,
700px and under) keeps `PhoneHeader` and its sheet: the rail doesn't fit,
and the phone's layout is its own decision. Chrome is laid out outside
`.main`, and pane headers take the one cell row the layout already keeps
above each pane, so the daemon's cell geometry and who sizes a tab are
untouched by it. Slate is the dark palette when none is picked; a picked
one is kept. Every palette's UI text is drawn at WCAG AA against its
backgrounds: `legible()` moves a color that falls short toward white or
black as little as it takes, so palettes keep their hues and the terminal
colors aren't touched. Where: `web/src/ui/app.tsx`, `web/src/ui/shell/`,
`web/src/ui/panes/`, `legible()` in `web/src/appearance.ts`. From: S39,
M107–M113 (#1022–#1028).
