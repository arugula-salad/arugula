---
schema: 1
id: "aru-127"
title: "Hints come from facts, once per account"
state: "decided"
area: "Clients"
source:
  via: "cli"
question: "How does the app mention a feature when it becomes useful: what a hint is, where it shows, how often, how it's turned off and counted, and where \"seen\" is kept?"
options:
  - id: "a"
    label: "One registry of hints, each from a fact, said once in one of four places, seen kept as an account pref on the hub (else in the browser)"
    how: "A hint is an id, a fact (a predicate over the client's state, or an answer the client asks for), one line of copy, an action (a command id or a route) and its place: a pane-header chip, a toast, an empty state, or a pane modal over the fact's pane (a big tip, with a short body, a few buttons and \"Don't show tips like this again\"). One shows at a time, none new while someone types; it's seen once shown, or, for a pane modal (and any hint that says `seenOn: \"answer\"`), once answered. Seen, Tips, Big tips (`tips.modal`) and What's new's last version are prefs in one small store (web/src/shared/seen.ts): through control, `GET`/`POST /api/me/prefs` (a new `account_prefs` table on the hub); otherwise localStorage."
    tradeoff: "A new hub table and route for what is a handful of keys an account; a daemon's own page, even for a joined machine, still remembers per browser, since it has no session with control."
  - id: "b"
    label: "Keep seen in the machine's state dir"
    how: "The daemon stores which hints its owner saw, beside flags.json."
    tradeoff: "Per machine, not per person: every machine says it again, and a guest's page would write to its owner's machine."
  - id: "c"
    label: "Only per browser (localStorage)"
    how: "No server at all."
    tradeoff: "Every browser, phone and desktop window says each hint again, which is what the issue rules out."
  - id: "d"
    label: "A guided tour or timed nudges"
    how: "Coach-marks over the UI on first run, or tips on a schedule."
    tradeoff: "Says things that aren't relevant yet, and interrupts: the swarm's rule is that every hint comes from a real fact."
choice:
  option: "a"
  reason: "Discovery (Arugula #238 and #241): a mention only when a fact makes it relevant, said once per person wherever they sign in. The hub already keeps per-account prefs (push_prefs), so a generic key-value table next to it is the smallest store that follows the account; the platform's notify prefs are typed fields of another process."
rejected:
  - option: "b"
    why: "Seen is the person's, not the machine's."
  - option: "c"
    why: "Kept, but only as the fallback off control."
  - option: "d"
    why: "No coach-marks, no timers, no modal over the window. The one modal is a pane modal, from a fact, over that pane alone, closed for good with \"Don't show tips like this again\" (#1737)."
supersedes: []
evidence:
  - title: "Arugula #241 (Discovery 0)"
    url: "https://app.arugula.io/issues/iss_cedaab33ef116a6f060daece807933fa"
  - title: "#1737 A pane-modal tip place, with \"Don't show tips like this again\" (epic #1736, ruling 7)"
    url: "https://github.com/arugula-salad/arugula/issues/1737"
  - title: "#1735 nvim: tips, a key-tip chip on each nvim start, and the \"nvim in Arugula\" help card (epic #1736, ruling 6)"
    url: "https://github.com/arugula-salad/arugula/issues/1735"
decided_by: "jhgaylor"
decided_on: "2026-10-27"
reviews: []
constrains:
  - "path:web/src/shell/hints/index.ts"
  - "path:web/src/shell/hints/list.ts"
  - "path:web/src/shell/hints/ports.ts"
  - "path:web/src/shell/hints/nvim.ts"
  - "path:web/src/shell/hints/nvim-facts.ts"
  - "path:web/src/shell/hints/editor.ts"
  - "path:crates/mux/src/osc.rs"
  - "path:crates/mux/src/classify.rs"
  - "path:web/src/shell/ui/hints.tsx"
  - "path:web/src/shell/ui/panes/pane-slot.tsx"
  - "path:web/src/shared/ui/modal.tsx"
  - "path:web/src/shell/ui/commands.tsx"
  - "path:web/src/shared/seen.ts"
  - "path:web/src/shared/telemetry.ts"
  - "path:crates/control/src/prefs.rs"
  - "path:crates/control-wire/src/telemetry.rs"
---

# Hints come from facts, once per account

The app already knows the facts that make a feature relevant: a port starts
listening in a pane, a PR URL is printed, an agent waits while the tab is
hidden, panes come back after a restart, a second person joins. One
mechanism turns such a fact into a one-time mention. Never a timer, never
a tour.

**A hint** is an entry in `HINTS` (`web/src/shell/hints/list.ts`):
- `id`: its name for good, what "seen" remembers (`seen:<id>`); never reused.
- `fact(client)`: what holds now, from the client's state or from an
  answer it asked for, or null. It says where (a chip's pane) and the
  values the copy and the action take. It's asked only while the hint is
  unseen and Tips are on, so a fact that costs a request stops costing
  once it's been said.
- `copy(fact)`: one line (a pane modal's title).
- `action`: a label (which may name the fact's values: *Split right on
  HOST*), and a command id of `COMMANDS` in
  `web/src/shell/ui/commands.tsx` (given the fact's values) or a route
  (`#swarm`). `COMMANDS` is new: the menus had no ids to name. What's
  new's *Try it* names them too (Discovery 2, `shell/ui/whats-new.tsx`).
- `place`: where it renders.
- `seenOn`: `"show"` (a chip's, a toast's, an empty state's) or
  `"answer"` (a pane modal's), below.
- `repeat: "per-fact"`: said again for each new fact, below.
- A pane modal's alone: `body(fact)`, paragraphs, lists and headings of
  text, key caps (`{ kbd }`) and links (`{ link, href }`), and `second`, a
  second button (a command, a route, or only closing it); or `buttons`
  instead of both, a short list, each a command, a route or a close, with an
  optional `when(client, fact)` that shows it only while it holds (nvim's
  card shows *Add arugula.nvim* only when the pane's nvim lacks the plugin
  and Arugula could add it). A heading with an `id` is a section: a fact
  whose `args.section` names it opens the modal scrolled to it.

**Four places only:** a chip in a pane's header (`HintChip`), a toast at
the bottom of the workspace (`HintToast`), a view's empty state
(`HintEmpty`, today the no-sessions view), or a pane modal over the fact's
pane (`HintPaneModal`), in `web/src/shell/ui/hints.tsx`. No coach-marks,
and no modal over the window. Each has its action and ×. A renderer tells
the hints it's drawn, and a hint is shown only where one is: a phone has
no pane headers, so a chip hint waits for a computer, and draws no pane
modals.

**A pane modal** (a big tip, #1737) covers its pane between the pane's
header and its edges with a panel over 90% of it (5% margin each side, the
dimmed pane showing around it) and a thin border, drawn by the pane's slot
(`web/src/shell/ui/panes/pane-slot.tsx`), for any kind of pane: a
terminal, an agent, chat or Docs block, a framed block (browser, editor,
port), here or on another machine. The pane keeps running and drawing
under it, dimmed; the rest of the workspace stays usable. It's `useModal`
with `covers` (`web/src/shared/ui/modal.tsx`): the pane's view is `inert`
while it's up, so no click, key or focus reaches it, a frame's neither;
the focus moves in when it was on that pane (a terminal, a composer, a
frame) or nowhere, and back to it on close; Escape and Tab are the
modal's while the focus is in it, so Escape closes it before the terminal
hears it. Its title, short body, buttons and the checkbox **Don't show tips
like this again** are centred in the panel, at a reading width, and scroll
inside it when the pane is small; a × is in its corner. The buttons are
`buttons` that show now, or the `second` and the `action`; one is the
primary (the action, or the first shown `buttons` entry that isn't a close). Closing it with ×, Escape or a button answers it. It never opens
under a dialog over the window, over a pane someone else drives (pair
mode: its `driver` isn't this person), or with Big tips off.

**Pacing:** at most one hint on screen. None new while someone types: a key
in the page in the last 1.5 s, or the daemon's `typing` on a pane someone
else drives (its driver typed in it lately). This person's own keys are the
page's alone: the daemon's `typing` lasts 5 s after the last key, so a dev
server started with Enter would wait that long for its chip. One on screen stays while its fact holds, until
it's acted on or dismissed; when the fact goes (the server stops, the tab
changes), so does it.

**Once:** a hint is seen when it's shown, not when it's acted on, so a hint
someone ignored doesn't come back either. Dismissing or acting closes it.
A hint with `seenOn: "answer"` is seen only when it's answered (acted on,
or closed): one hidden before that, by a tab switch or its fact going,
shows again the next time its fact holds. Every pane modal is, since it
asks for an answer and a tab switch shouldn't use it up; chips may opt in,
as nvim's do (#1735).

**Repeating hints** (`repeat: "per-fact"`, #1735): the one exception to
once. The fact carries a `key` naming which fact it is, and the hint is
seen per key: `seen:<id>` holds the keys of the last 16 facts it was said
for (a list, so it stays within a pref's 512 bytes), and a fact with a new
key says it again. nvim's key tip is one: its key is the pane and its
nvim's server pid, so each new nvim process gets a tip and a refocus, a
tab switch or a reload doesn't. It paces like any other: it waits behind a
higher hint and a pane modal, never shows while someone types, and Tips
off stops it. A hint may take something when it comes on screen (`said`):
the key tip takes its tip there, and the account's place in the tip list
(`nvim.tips.next`, a pref beside seen) moves on, so each tip shows once
before any repeats.

**Shown from a menu** (`showHint`): a menu may show a hint on purpose, seen
or not and with Tips or Big tips off, in place of what's on screen: nvim's
help card from the pane menu's "nvim help" and from Help in the account
menu. It stays while its pane is there and drawn, until it's answered;
where nothing draws it (a phone), the menu opens the docs page instead.
Tips off, and "Don't show tips like this again", stop a hint showing on
its own, never a menu showing it.

**Seen, per account through control, else per browser.** One store,
`web/src/shared/seen.ts`: `seen(id)`, `markSeen(id)`, `pref`/`setPref` for
other keys (Tips, and `whats-new`, the last version What's new showed,
Discovery 2), loaded once. Until it has loaded, everything counts as seen, so nothing shows
before we know.
- On control's page with an account signed in: the account's prefs on the
  hub, `GET /api/me/prefs` (all, as one object) and `POST` (some; `null`
  takes one back; a key left out stays, so two tabs don't undo each
  other). Table `account_prefs (account, k, v)`, deleted with the account;
  control knows nothing of the keys. Bounded: 256 keys an account, names of
  lowercase letters, digits and `.:_-` up to 80, values up to 512 bytes of
  JSON (`crates/control/src/prefs.rs`).
- Anywhere else (a daemon's own page, a read-only link): localStorage
  (`arugula.seen`), re-read before each write. A daemon's page has no
  session with control even when its machine is joined, so it can't write
  the account's prefs; it remembers per browser.
- A control that can't answer the prefs falls back to the browser's.

**Turning hints off:** *Tips* in Settings (the account menu) turns them all
off, pane modals too: the pref `tips: false`, in the same store, so
through control it's the account's. Each hint has × too. A pane modal's
**Don't show tips like this again**, ticked when it's answered, sets
`tips.modal: "off"`: every pane modal stops on that account (its devices,
on control's page), and chips and toasts carry on. *Big tips* in Settings,
under Tips, shows that and turns them back on.

**Counting:** through `countView` (opt-in usage counts,
docs/telemetry.md): three views, `hint-shown`, `hint-acted` and
`hint-dismissed`, every chip's, toast's and empty state's together; and
four, `pane-tip-shown`, `pane-tip-acted`, `pane-tip-closed` and
`pane-tip-opted-out`, every pane modal's together. The views are a closed
set on the wire (`View` in `crates/control-wire/src/telemetry.rs`); a hint
counted on its own, a chip as well as a pane modal, adds its own variants
and names them in its entry (`views`: shown, acted, closed, and a pane
modal's opted out), counted beside the shared ones. nvim's help card and
chips do (`nvim-help-*`, `nvim-connect-*`, `nvim-open-here-*`,
`nvim-diff-*`, `nvim-key-tip-*`).

**Facts from the daemon, or the client's alone:**
- *A port listening in a pane* (the example, `port-listening`): no daemon
  event. The client asks `pane.ports` about a pane of the shown tab when it
  gives a reason to: a program (not the shell) runs in it and it started, or
  printed, since the last answer (a dev server prints its URL once it
  listens), at most every 2 s a pane, on this host, for someone who may type
  there and where blocks can show (`web/src/shell/hints/ports.ts`). If that
  ever costs too much, the daemon can push ports in `PaneInfo`.
- *A second person joins*, *an agent waiting while the tab is hidden*,
  *a path pasted or files dragged over a pane*, *`cd` in a fresh pane*
  (the shell integration's command line), *a Doc block*, *a team made*
  (what control tells the page, `ControlSession.teams`): the client alone.
- *A PR URL printed*: the daemon sees the output; the page never scans
  it. The pane's output scanner (`crates/mux/src/osc.rs`, beside OSC 133)
  picks out links shaped like a PR's or an issue's, the web client's
  `forgePr`/`forgeIssue` shapes, printed or in OSC 8, and `PaneInfo.link`
  is the last one. The client opens it as its click does (`forgeLink`).
- *Panes come back after a restart*: the daemon marks the terminals it
  restored at startup whose process didn't survive (`PaneInfo.restored`,
  for as long as it runs; a restored pane's policy and `running` are close,
  not the fact).
- *A Claude Code conversation on this machine, none open*: the daemon's
  index already knows (`conversations.list`, what `arugula claude ls`
  reads); the client asks once a page, while the hint is unseen.
- *Push*: only where this device's push is `off`: never on plain http, a
  browser without it, notifications blocked, or where it's on already.
- Batch 2 (Discovery 11). *A command's mark* (TerminalView's OSC 133
  marks), *the Share dialog warned a pane may show secrets* (the daemon's
  guess, once the dialog closes), *push on through control*, *a shared
  session with agents in it* (a guest's or a teammate's `notifyPref`),
  *`code` as the last command line*, *another machine in the host menu*
  and *a GitHub-only account on a device that can make a passkey*: the
  client alone. *vim or nvim in a pane* is the daemon's foreground command
  (`PaneInfo.command`). *A commit or a checkout* is the daemon's:
  `PaneInfo.branch` (M109) and `head`, the short commit its repository's
  HEAD is at, read from the files with the branch
  (`crates/mux/src/classify.rs`, read afresh when a command ends); the
  client remembers each pane's from when it first saw it, typing or not,
  and the fact holds once either moved. `code` as the last command line
  says the editor hint; vim or nvim in the pane never does (#1735).
- nvim (#1735, `web/src/shell/hints/nvim-facts.ts`, its actions in
  `nvim.ts`): the daemon's. A pane runs nvim when its foreground command
  is nvim or the daemon reads an nvim there (`PaneInfo.editor`, #1730),
  which also says its server's pid and its version, and `offer_plugin`
  when arugula.nvim could be added (#1733). The help card's fact is the
  first nvim pane of the shown tab; `nvim-connect`'s, an nvim offered the
  plugin on this host; `nvim-open-here`'s, an nvim the daemon reads while
  the Editor preference (#1731, `shell/open-in-editor.ts`) isn't nvim;
  `nvim-diff`'s, a pane of the tab with Claude Code's edit waiting
  (`PaneInfo.diff`) beside an nvim the daemon reads; the key tip's, a pane
  whose nvim has a pid. Its button runs `:help <tag>` in that nvim
  (`pane.nvim_help`, over nvim's RPC socket), else opens the tag's page in
  nvim's online docs.

**Under a test runner** (`navigator.webdriver`) hints keep quiet unless the
page sets `window.__arugulaHints`, as telemetry's ask does: a toast would
land on what other specs click. The hints' own specs set it. A spec may
also bring hints of its own, `window.__arugulaTestHints`, read only under
a test runner and offered before the registry's: the pane modal's spec
(`web/e2e/hints-pane-modal.spec.ts`) does, so the place is tested apart
from any feature's tip.

Where: `web/src/shell/hints/` (the registry, the engine, a module per fact),
`web/src/shell/ui/hints.tsx` (the four places), `web/src/shell/ui/panes/pane-slot.tsx`
(where a pane modal is drawn), `web/src/shared/ui/modal.tsx` (`useModal`'s
`covers`), `web/src/shell/ui/commands.tsx` (`COMMANDS`, *Tips*, *Big tips*),
`web/src/shared/seen.ts`, `web/src/shell/main.tsx` (which store),
`crates/control/src/prefs.rs` and `db.rs` (`account_prefs`).
From: Discovery 0, Arugula #241 (part of #238); Discovery 3, 4, 5 and 11 add entries; #1737 the pane modal; #1735 repeating hints, menus that show one, and nvim's.
