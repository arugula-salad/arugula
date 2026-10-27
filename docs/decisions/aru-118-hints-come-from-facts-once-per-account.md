---
schema: 1
id: "aru-118"
title: "Hints come from facts, once per account"
state: "decided"
area: "Clients"
source:
  via: "cli"
question: "How does the app mention a feature when it becomes useful: what a hint is, where it shows, how often, how it's turned off and counted, and where \"seen\" is kept?"
options:
  - id: "a"
    label: "One registry of hints, each from a fact, shown once in one of three places, seen kept as an account pref on the hub (else in the browser)"
    how: "A hint is an id, a fact (a predicate over the client's state, or an answer the client asks for), one line of copy, an action (a command id or a route) and its place: a pane-header chip, a toast or an empty state. One shows at a time, none new while someone types; once shown it's seen. Seen, Tips and What's new's last version are prefs in one small store (web/src/shared/seen.ts): through control, `GET`/`POST /api/me/prefs` (a new `account_prefs` table on the hub); otherwise localStorage."
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
    why: "No modals, no coach-marks, no timers."
supersedes: []
evidence:
  - title: "Arugula #241 (Discovery 0)"
    url: "https://app.arugula.io/issues/iss_cedaab33ef116a6f060daece807933fa"
decided_by: "jhgaylor"
decided_on: "2026-10-27"
reviews: []
constrains:
  - "path:web/src/shell/hints/index.ts"
  - "path:web/src/shell/hints/list.ts"
  - "path:web/src/shell/hints/ports.ts"
  - "path:web/src/shell/ui/hints.tsx"
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
- `copy(fact)`: one line.
- `action`: a label, and a command id of `COMMANDS` in
  `web/src/shell/ui/commands.tsx` (given the fact's values) or a route
  (`#swarm`). `COMMANDS` is new: the menus had no ids to name.
- `place`: where it renders.

**Three places only:** a chip in a pane's header (`HintChip`), a toast at
the bottom of the workspace (`HintToast`), or a view's empty state
(`HintEmpty`, today the no-sessions view), in `web/src/shell/ui/hints.tsx`.
No modals and no coach-marks. Each has its action and ×. A renderer tells
the hints it's drawn, and a hint is shown (and so seen) only where one is:
a phone has no pane headers, so a chip hint waits for a computer.

**Pacing:** at most one hint on screen. None new while someone types: a key
in the page in the last 1.5 s, or the daemon's `typing` on any pane (its
driver typed in it lately). One on screen stays while its fact holds, until
it's acted on or dismissed; when the fact goes (the server stops, the tab
changes), so does it.

**Once:** a hint is seen when it's shown, not when it's acted on, so a hint
someone ignored doesn't come back either. Dismissing or acting closes it.

**Seen, per account through control, else per browser.** One store,
`web/src/shared/seen.ts`: `seen(id)`, `markSeen(id)`, `pref`/`setPref` for
other keys (Tips, and What's new's last version for Discovery 2), loaded
once. Until it has loaded, everything counts as seen, so nothing shows
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
off: the pref `tips: false`, in the same store, so through control it's the
account's. Each hint has × too.

**Counting:** through `countView` (opt-in usage counts,
docs/telemetry.md): three views, `hint-shown`, `hint-acted` and
`hint-dismissed`, every hint's together. The views are a closed set on the
wire (`View` in `crates/control-wire/src/telemetry.rs`); a hint that needs
counting on its own adds its own variant.

**Facts from the daemon, or the client's alone:**
- *A port listening in a pane* (the example, `port-listening`): no daemon
  event. The client asks `pane.ports` about a pane of the shown tab when it
  gives a reason to: a program (not the shell) runs in it and it started, or
  printed, since the last answer (a dev server prints its URL once it
  listens), at most every 2 s a pane, on this host, for someone who may type
  there and where blocks can show (`web/src/shell/hints/ports.ts`). If that
  ever costs too much, the daemon can push ports in `PaneInfo`.
- *A second person joins*, *an agent waiting while the tab is hidden*:
  the client alone (presence, attention and `document.visibilityState`).
- *A PR URL printed*: needs the daemon, which sees the output; a fact in
  `PaneInfo` (the link, once), as the forge blocks read them.
- *Panes come back after a restart*: needs the daemon to mark the panes it
  restored at startup (a restored pane's policy and `running` are close,
  not the fact).

Where: `web/src/shell/hints/` (the registry, the engine, the port fact),
`web/src/shell/ui/hints.tsx` (the three places), `web/src/shell/ui/commands.tsx`
(`COMMANDS`, *Tips*), `web/src/shared/seen.ts`, `web/src/shell/main.tsx`
(which store), `crates/control/src/prefs.rs` and `db.rs` (`account_prefs`).
From: Discovery 0, Arugula #241 (part of #238); Discovery 3, 4 and 5 add entries.
