# Proposal: a better pane right-click menu

Status: proposal, nothing implemented. Code: `paneItems` in
`web/src/shell/ui/commands.tsx`, drawn by `web/src/shared/ui/menu.tsx`, opened
from `web/src/shell/ui/panes/pane-slot.tsx`.

## What's wrong now

1. **It is one flat list of up to ~35 rows.** A terminal pane in a normal
   session shows: Split right/down, Split (local), New VM pane, Split on
   each remote host, Open a web page, Open a port, Start an agent, Start a
   chat, Team chat, Pin a chat room, Claude Code conversations, Open a studio
   app, Open pull request, Open issue, Open in editor, Changes, Open as
   workspace, Attach file, Move to new tab, Go to directory, Copy working
   directory, Share read-only link, Invite over ssh, Share this pane, drive
   items, thread items, restart policy items, Dismiss, Shell integration,
   Forget history, Close pane. On a short window it scrolls.
2. **The first thing you want isn't there.** Right-clicking a terminal has no
   Copy, Paste, Select all or Clear. (Copy/paste work by keyboard and the
   browser menu is suppressed, so on a mouse the pane feels broken.)
3. **Order is by when the feature shipped, not by use.** "Close pane" and
   "Forget history" sit a few pixels from "Shell integration (new shells)", a
   settings toggle. Destructive rows have no confirmation or distance.
4. **Everything that creates a block is a peer of "Split".** "Split right" and
   "Open a port…" are the same idea (put something beside this pane) at the
   same level as a toggle.
5. **Labels are inconsistent**: "Split (local)", "Split right on host",
   "New VM pane on the right", "Open in editor", "Changes" don't say where
   they open or what they act on. Some end in "…", some that prompt don't.
6. **No icons, little grouping.** Only a few separators; the `menu-header`
   and `detail` support exists but the pane menu barely uses it.
7. **Same list in three places** (right-click, header "more", palette), so
   length hurts everywhere. The palette is fine as a flat list; the menu isn't.
8. **Right-click on a command mark** (the stripe) has its own menu
   (`markMenu`), so the pane menu differs by where in the pane you click.

## Proposal

Make the menu short at the top level and put the rest in submenus. Add
submenu support to `MenuItem` (`submenu: MenuItem[]`, opens right on hover or
→, back with ←; the palette flattens it as `Pane › Open beside › …`, which it
already does for headers).

Top level, terminal pane:

```
Copy                     Ctrl+Shift+C     (only with a selection)
Paste                    Ctrl+Shift+V
Select all / Clear screen
──────────
Split right              Split down
Open beside            ▸  Agent… · Web page… · Port… · Pull request… ·
                          Issue… · Editor · Changes · Chat · Studio app ·
                          Remote shell on <host> · VM
Move                   ▸  To new tab · Fill tab (focus) · Go to directory…
──────────
Copy working directory
Attach file…
Share                  ▸  This pane… · Read-only link… · ssh invite · Drive
Thread                 ▸  (existing thread items)
──────────
Pane settings          ▸  Restart policy · Shell integration · Forget history
Close pane
```

~13 rows instead of ~35. Notes:

- **Copy/Paste/Select all/Clear** come first. Copy is disabled with no
  selection; Paste uses `navigator.clipboard.readText` and falls back to the
  browser menu where it's refused (Chrome on a phone, as `editable` already
  does for composers). Applications that track the mouse keep the
  Shift+right-click rule they have now.
- **Close pane** stays last, alone, danger-coloured. **Forget history**
  moves under Pane settings and asks first, since it can't be undone.
- **One verb per row**: "Open beside ▸" always means a new block next to this
  pane, so the individual labels lose "Open"/"beside" repetition.
- **Context-sensitive top**: for a browser, doc, editor or agent pane, the
  block's own rows (Reload, Copy URL, Share doc…) replace Copy/Paste at the
  top. Remote and watch-only panes keep their short lists, as now.
- **Icons** in a left column (reusing `PaneTypeIcon` style), which also
  absorbs the `checked` dot.
- **Keyboard**: arrows, Home, End already work; add → / ← / Enter for
  submenus and type-ahead on the first letter.
- **Command marks**: the stripe's menu (copy command, copy output, re-run)
  merges into the top of the pane menu when the click lands on a mark, so
  there's one menu with extra rows rather than two.

## Cost and order

1. Copy/Paste/Select all/Clear on the pane menu. Small, the biggest win,
   shippable alone.
2. Submenu support in `menu.tsx` (+ palette flattening), tests for keys and
   edge placement.
3. Regroup `paneItems` into the layout above; update e2e specs that click
   menu rows by label.
4. Context-sensitive top and command-mark merge.

Tab, session and `+` menus have the same flat-list shape and can reuse the
submenu work later; they're shorter and not in scope here.

## Questions

- Is "the right click menu" the pane menu (what a terminal shows)? If you
  meant the tab strip, sidebar or the desktop app's native menu, the same
  approach applies, but the lists differ.
- Submenus on touch: tap-to-expand inline, or drill-in? (Proposed: inline.)
