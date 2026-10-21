# #1018: the workspace's chrome and panes, redesigned

Planned against `main` at 0d107f23 (v0.45.0). Numbers: S39 and M107–M113 (#1022–#1028).
S38 holds M93–M98 (#979) and telemetry M100–M106 (#989).

## What's there today

The desktop workspace is `App` in `web/src/shell/ui/app.tsx` (957 lines) under one
36px `AppBar` (`ui/app-bar.tsx`). From left to right, the bar holds the host
button, the places (Panes, Swarm, Chat, Agents), a session menu, Share, the
tab strip, `+` and `▾`, Commands, the update chip, people, the account and
the window buttons. That one bar is what the issue wants broken up.

What the redesign can reuse as it is:

- **Every action** already lives in `ui/commands.tsx` (`paneItems`,
  `tabItems`, `newTabItems`, `sessionItems`, `settingsItems`), and the
  palette (`ui/palette.tsx`, Ctrl/Cmd+Shift+P) flattens them, with fuzzy
  ranking and recents. Command search is this palette extended, not a new
  registry.
- **Attention:** `PaneInfo.attention/.reason/.ask` arrive in `State` and in
  fleet summaries. Requests are answered through `client.act()` →
  `POST /api/attention/act`, with `PermissionButtons`
  (`ui/answer-card.tsx`). Viewing a request never answers it. Viewing a
  `Done` pane marks it seen, which is the existing behaviour.
- **Connection state:** `client.connected`/`failures`, `client.path`
  (direct or relayed) and fleet `HostStatus`. `machineState()`
  (`ui/hosts.tsx`) already turns these into words.
- **Repo per pane:** `PaneInfo.project {root, name}` (from `classify.rs`)
  and `cwd` (OSC 7).
- **Guest:** `client.role()` and `ownsMachine()`. The places and actions
  already hide themselves for guests.
- **Cloud runs** (`web/src/shared/cloud.ts`) already show up as fleet panes with
  attention.

What the redesign needs and doesn't have yet:

| Need | State | Plan |
|---|---|---|
| Layout presets | No intent rebuilds a tab's tree | New core intent (M109) |
| Desktop focus mode | `Tab.zoom` exists, but it is set by `View` (size arbitration) and only phones use it | Daemon-owned focus (M109) |
| Branch | Missing. Only the diff block knows HEAD and the default branch | `PaneInfo.branch`, read from `.git/HEAD` in `classify` (M109) |
| Interactive pane headers | `PaneHeader` is one cell row in the divider row, with `pointer-events:none`, and only when there's more than one pane | Make it interactive without changing the geometry (M110) |
| Attention inbox | `GET /api/attention` exists but the web client doesn't use it. Swarm's rail and Chat's Activity list asks | A drawer over the fleet model (M111) |
| Activity per session | `/api/history` has no session filter and no ids, and is owner-only. `/api/events` has no ids | Small daemon additions (M112) |
| Keybinding table | Shortcuts are capture-phase listeners scattered across files | Central table (M107) |
| Session overview route | None | `#overview` from current state (M111) |

## Decided (Jake, 2026-10-18)

- **Toggle, then flip.** The new shell sits behind a client setting
  (Appearance → *New workspace*, in localStorage) from M108 until M113, then
  becomes the only shell and the old `TopBar` is deleted. main stays
  releasable between milestones, and e2e runs against the old shell until
  M113 moves the specs over.
- **Focus is shared and daemon-owned** (decision 2 below).
- **Shortcuts are the Ctrl/Cmd+Shift family:** Ctrl+Shift+1–9 jumps to pane
  N (matched on `event.code`, so layouts don't matter), Ctrl+Shift+Enter
  toggles focus, Ctrl+Shift+E opens `+ Pane`, and Ctrl+Shift+P is search
  (today's palette). Ctrl+Shift+N was the first pick, but Chrome reserves it
  for incognito and a page can't take it. M107 checks each binding in
  Chrome, Firefox, WebKit and the desktop app before it ships.
- **Next:** this plan, then a tracker and an issue per milestone. Stop for
  review before M107.

## Decisions this plan makes (change any)

1. **The chrome, not the colours, is the change.** The layout (rail,
   sidebar, header, tab strip, toolbar, footer) applies to every palette in
   `appearance.ts`. The mission-control colours become a new dark palette,
   and the default for dark. Nothing in the chrome uses a raw colour, only
   tokens, so latte and the others keep working.
2. **Focus is daemon-owned and shared,** like the rest of the layout. A new
   `Tab.focus: Option<PaneId>` is set by `Intent::Focus{tab, pane}` and
   `layout()` honours `focus.or(zoom)`. Phones keep `View.zoom`. Focus never
   touches `root`, so the round trip is free and exact, and it survives a
   reload. Split, MovePane and DockTab clear it, as they clear zoom.
3. **Presets are one atomic intent:** `Intent::Arrange{tab, preset, main}`
   with `Preset::{Columns, Grid, MainStack}`. It rebuilds `tab.root` from
   `root.panes()` (reading order), with new split ids from `node_ids()`, and
   returns no effects, so no pane is spawned or killed. Each preset is
   deterministic for any n:
   - Columns: one Row of n with equal weights.
   - Grid: c = ⌈√n⌉ columns, filled row by row. The last row's panes widen
     to fill it.
   - MainStack: a Row of main (0.6) and a Column of the rest in reading
     order. For n = 1 it is just the pane. For n = 2 it is Columns with
     main first.

   Access is Editor on the tab, like RenameTab. It is refused when
   `min_size` won't fit in the tab's cells, and the button then says why.
4. **Pane headers stay one cell row.** The layout already reserves a row
   above every pane (the divider row, or the top margin). That keeps the
   terminal geometry and the daemon's size maths as they are, which is the
   riskiest acceptance criterion. Hit targets grow by overlapping the
   one-column gutters with a 6px invisible handle, not by taking cells.
5. **Sessions on the selected host.** The sidebar replaces the session
   menu. Rows show name, aggregate attention, tab and pane counts, and the
   repo or branch only when every pane agrees ("Multiple branches"
   otherwise). Groups are Needs you, Working and Idle, from pane attention.
   There is no Archived group and no last-activity time we don't have
   (`activity.last_ms` is real, so "2m ago" is fine).
6. **Phone is untouched:** `usePhone()` (≤700px) keeps `PhoneHeader` and the
   sheet. Between 700px and about 1100px, the sidebar collapses to the rail
   and the header drops secondary metadata before anything shrinks
   terminals.
7. **Keep test hooks.** The new elements keep `.session-button`,
   `[data-tab-id]`, `.tab`, `.new-tab`, `[data-new-tab-more]`,
   `[data-account-menu]`, `[data-host]` and `data-open-*` where the role
   carries over, so most of the roughly 120 specs keep passing.
8. **The desktop app's titlebar:** the context header is the drag region
   (`data-tauri-drag-region`) and hosts `WindowButtons`. The rail leaves room
   for macOS's traffic lights.

## Milestones, in order

```
M107 ─┬─ M108 (shell) ───────┬─ M110 (panes) ─┬─ M111 (attention, overview, search) ─┬─ M113
      └─ M109 (daemon layout) ┘                └─ M112 (activity drawer) ──────────────┘
```

**S39: fixture, tokens and screenshots (half a day, folded into M107's PR).**
- A seeded fixture, `just dev-fixture` (a script over the CLI against
  `just dev`'s daemon): 8 sessions, one with 4 tabs, one tab with 6 real
  panes (a terminal running tests, a shell, an agent, an editor, a diff and
  a browser on a port), attention on two of them, and a remote pane.
- "Before" shots at 1440×900, 1920×1080, 1100×800 and a phone, through
  `just dev-browser`.
- The palette's tokens checked against WCAG contrast.

**M107 (#1022): pull the shell apart, without changing the look.**
- Split `app.tsx` into `ui/shell/` (`Rail`, `SessionSidebar`,
  `ContextHeader`, `TabStrip`, `PaneToolbar`, `StatusFooter`, `PaneHeader`)
  and `ui/panes/` (`TabArea`, `PaneSlot`, `Divider`, `DropOverlay`). At this
  stage they still render today's bar.
- `ui/keys.ts`: one table of bindings with `{id, keys per OS, when, run}`.
  The existing listeners move onto it, and the palette and the footer read
  hints from it.
- Done when: the e2e suite passes unchanged.

**M108 (#1023): the new shell.**
- Rail: Sessions (toggles the sidebar), Panes, Swarm (labelled "all
  machines"), Chat (when `hasThreads`), Agents (owner and Labs), Settings.
- Sidebar: search with keyboard and an empty state, `+` through the existing
  new-session flow, the row overflow menu (`sessionItems`), and its width
  and collapse kept in localStorage per device.
- Header: brand → `#overview`, the host button with its connection chip
  (direct, relayed, reconnecting or offline, from real state), the session
  breadcrumb, a command search button (the palette), the bell (M111 fills
  it, hidden until then), the account and people, and the window buttons.
- Tab strip: today's `TabItem` with its semantics.
- Footer: host, repo and branch only when known, the agents running, and
  needs-input, with hints from `keys.ts`.
- Each route keeps its return target (Panes → last tab and pane per
  session).

**M109 (#1024): the daemon's layout (parallel with M108).**
- `Intent::Arrange` and `Intent::Focus`, `Tab.focus` with
  `#[serde(default)]`, access arms, `arb_intent()`, `tests.rs` cases (every
  n from 1 to 9 for each preset keeps the pane set and validates), and
  `just proto-ts`.
- `PaneInfo.branch`: read `HEAD` in `classify` beside `project`, with no
  `git` process and refreshed when the cwd changes. A detached HEAD shows
  the short sha.

**M110 (#1025): pane ergonomics.**
- An interactive `PaneHeader` on every pane when there is more than one: an
  ordinal (the shortcut index), type, status, host for remote panes,
  cwd/repo and branch, expand (Focus) and `…` (`paneItems` plus split
  right/down, move to a new tab, copy link). Click focuses, drag uses
  `.grip`'s docking.
- Pane toolbar: the pane count opens a switcher. Columns, Grid and
  Main + stack call `Arrange`, disabled with a reason for viewers or when
  the pane count doesn't fit. Focus toggles. `+ Pane` opens a drawer with
  type (shell, agent, editor, changes, port), machine, directory (the
  picker) and placement right or down, showing the destination before
  creating.
- Keyboard resize through the existing `resize_split`.
- The shortcuts above, from `keys.ts`, with hints in menus, tooltips and the
  footer.

**M111 (#1026): attention, overview and search.**
- The bell, the footer's needs-input and the sidebar's Needs you group all
  open one drawer. Its items come from the fleet model swarm already builds,
  plus cloud runs. Selecting an item shows command, machine, directory and
  agent. Allow and Deny use `PermissionButtons` and `act`, and Go to pane
  navigates without answering. Closing the drawer leaves the request
  pending.
- `#overview`: the selected host's sessions, each with its tabs, panes and
  attention, from `State` only.
- The palette gains sessions, tabs and panes on this host, and the
  directory picker stays its own action.

**M112 (#1027): the activity drawer.** Daemon:
- `/api/history` gains `session=` and a stable `id` (host, pane and start
  offset, hashed), so refresh merges instead of duplicating.
- Guests get their visible panes' entries, through the same visibility as
  `State`.

Web: a collapsible drawer below the tab, never a pane, with All, Agents
(Agent and Answer entries) and Commands. System covers exit, opened, closed
and machine events from `/api/events`. Files is left out, because there's
no file log. Live means follow. Refresh resyncs by id. `…` holds clear view
and collapse. Clicking an entry reveals its pane.

**M113 (#1028): polish, states and the release.**
- Keyboard-only paths and visible focus, `aria-label`s on every icon,
  `prefers-reduced-motion`, and contrast.
- States: empty host, offline, reconnecting, guest or viewer, failed pane
  creation, long names, and 20 tabs.
- Two clients on one tab (`size-fight`).
- e2e: extend structure, layout, reattach, attention, phone, blocks and
  presence, plus new `chrome.spec.ts` and `arrange.spec.ts`.
- After shots at the four sizes, `docs/features.md`, and release notes.

## Done when

The issue's acceptance criteria, with the fixture's screenshots in each UI
PR (Before | After), `pnpm typecheck` and `pnpm build`, the full
`just e2e` on the rollup, and core and proto tests for M109.

## Left out (follow-ups, not in this track)

Custom session groups and archive, a file-activity log, per-agent model and
tool controls beyond what the agent block offers, and Cmd+1–9 or Cmd+K
bindings.
