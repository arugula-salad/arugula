---
section: Fixed
title: One key does one thing
line: A key now runs one action, the one for where you are: on a board `l` and `f` no longer run the agent panel's too, and ⌘K on Chat opens only the room switcher.
headline: no
---
When a page and the agent panel (or the whole app) both had a key, pressing
it ran all of them. Now the most specific one runs: a dialog's, then the
page's, then the agent panel's, then the app's. On a board, `l` puts you in
the lead's box and `f` opens Filter, and the panel stays as it was. On Chat,
⌘K (Ctrl+K) opens Jump to a room, without Go to… opening behind it;
Ctrl+Shift+K still opens Go to… there. Escape closes the top dialog, not
every one under it.

After a `g`, Arugula waits a moment for the place's letter. If you click
into a field instead, it stops waiting, so the next key you type lands in
the field.
