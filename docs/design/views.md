# Design: Views

Status: first slice built (native #503). The motion prototype is at
https://claude.ai/artifact/9DQ7axQi4kHZwwer8BVV8e, and its **Copy values**
button gives the object in `web/src/shell/views/motion.ts`.

## What a View is

A layout you own that points at panes which live somewhere else. A leaf is
`<machine>.<pane>`, any pane or block on any machine you can reach. A View only
shows them:

- Closing a View closes nothing, and the tabs don't change.
- A pane can be in a tab and in any number of Views at once.
- The daemon doesn't know Views exist. No daemon or protocol change.

Building one is direct manipulation, never a form or a picker: there's no New
view dialog and no naming step.

## Building one

It starts in the swarm, where the blocks are. The element that lands in a leaf
is the one you picked up, streaming the whole way.

- **Hold a block** (130 ms, 280 ms by touch) and it lifts; the stage wakes.
  Moving before that pans, as a drag on a tile always has.
- **Over a leaf**, a zone shows the half it would take, or its middle.
- **Release** and it springs in, the other leaves making room.
- **Flick it at the stage** and it keeps its momentum, then the biggest leaf
  pulls it in.
- **Shift-drag**, or hold empty space and sweep, selects several. Holding one
  carries the flock.
- **Drag a leaf's header** to move it. Past the stage's edge it shrinks to a
  tile, and on release it flies home.

Other ways in: `V` in the swarm (the block under the pointer, or what's swept),
**Add to view** in a pane's menu and the palette, and `Delete` on a focused
header to send a leaf back. With reduced motion, blocks land at once.

Only the swarm's blocks theme has the gestures so far. Its right-click **Add to
view** works in every theme.

## Data model

One account pref per View (`view:<id>`, control's `/api/me/prefs`), or this
browser's localStorage with no control, through `shared/seen.ts`.

```
{ n?: name someone typed, t: tree, at: when it was made (ms) }
tree: "<machine>.<pane>" | ["h" | "v", [shares...], ...children]
```

A leaf of a kind this client doesn't know (say one naming something in control,
written by a newer client) is kept as it is, draws as a placeholder, and is saved
back unchanged, so an older client can edit the View without losing it. Only its
own Remove takes it out.

`<machine>` is control's device id when there is one, so renaming a machine
doesn't break a View; without control, the host's name. Shares are fractions, so
a View fits any window. A pref's value is at most 512 bytes: 9 leaves (the limit
a View has) with a 40-character name and dragged dividers fit, which
`scripts/views.test.mjs` checks. Writes are per key, so two devices changing
different Views don't collide; the same View is last write wins. Prefs load
again when the page comes back.

## How leaves reach their panes

`blocks/pane-links.ts`, split out of remote panes (#17): one full connection per
machine, drawing only the panes something shows (`client.only`), retrying while
it's away. Non-terminal blocks work the same way. Under control it connects with
the same end-to-end target the shell's main client does.

Every leaf goes through a link, the machine shown too: the main client is closed
when you switch machines, and has a selected tab and active pane a View mustn't
disturb. That costs one more connection per machine with a pane in an open View.
A pane nobody shows stays drawn for 4 seconds, so going from the stage to its
View doesn't load it again.

## Tab-scoped things

Until #477, a terminal's size is its tab's, and one client holds it (#473). A
leaf on the View's page follows the same rule as any other screen
(`sizeAsk` in `views/model.ts`), at the size of its place in the layout, so a
block being carried or on its way home never resizes a terminal:

| Who holds the tab's size | The leaf |
|---|---|
| Nobody, or a client that's gone | Takes it, sized to the leaf |
| This page's main client | Takes it. Showing the tab again takes it back the same way |
| Another window or device | Leaves it. Scaled and letterboxed, with **Use this size** |
| Two leaves of one tab | Neither takes it: a tab has one size. Both scaled |

**Use this size** takes the whole tab's size until #477, as the tab's own button
does. The stage in the swarm is a preview: its leaves never take a size.

- **Zoom:** a leaf taking a multi-pane tab's size zooms the tab to its pane, as a
  phone does. Another screen's claim brings the layout back.
- **Focus mode:** a View never sets it, and asks with `claim` only when the tab
  is free, because a refused claim ends the holder's focus (#439).
- **Active pane:** a View has its own active leaf. The main client's stays as it
  was, and reports no pane as looked at while a View is open.
- **Keys:** Ctrl/Cmd+Shift+N go through the leaves in reading order. The tab's
  layout keys are off on a View's page.
- **The leaf's menu** offers Remove from view and Close pane. Close pane really
  closes it; × only takes the leaf out.

## Gone panes

A leaf you can't see says why (machine unreachable, pane closed, not shared with
you, machine not in your list) and offers Remove. Unlike a remote pane, it isn't
removed on its own: you put it there. A pane not yet seen gets 5 seconds before
it counts as closed.

A View holds only addresses, and every daemon checks every connection and
keystroke, so it reaches nothing you couldn't already open. A viewer's leaf is
read-only and never takes a size.

## Phone

One leaf at a time, full screen, with previous and next. A wide terminal another
screen holds stays at a size to read, and pans. Views are in the phone sheet's
Open list until Home (#499). Building happens through the swarm's hold and sweep,
and a pane's menu. The key bar is off on a View's page for now: it belongs to the
main client's pane, so its keys would go to a pane you can't see.

## Not yet

Gestures in the city, hive and timeline themes; dragging a pane's grip from a
tab onto a View; the key bar and swiping between leaves on a phone; Views on
phone Home (#499); pop-out as a one-leaf View (#497); per-pane size in leaves
(#477); sharing a View; stacked leaves.
