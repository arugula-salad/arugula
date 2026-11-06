# Design language

Status: decided (#660, from the #685 inventory and its nine picks, Aaron,
2026-11-05). Read this before you build or restyle a page, a pane, a dialog
or a list. Where a piece says *(being built: #N)*, the rule is decided and
the shared component isn't there yet: build to the rule, and don't invent a
third look.

The rule behind all of it: **use the shared piece; if it doesn't fit, change
the piece, not your page.** The code is `web/src/shared/` (`ui/` for the
components, `buttons.css`, `style.css`). Prototypes in a Doc start from the
kit (`shared/ui/runner.html`, `--ar-*` tokens and `ar-*` classes), which
should paint what the app paints.

## Type

*(being built: #687.)* Seven sizes, each with its line-height, and nothing
between them:

| size | job | face | line |
|---|---|---|---|
| 11 | caps label (600, 0.08em), chips, tags, ids, times, kbd | UI; mono for ids | 16 |
| 12 | meta, secondary lines, buttons | UI | 16 |
| 13 | body: row titles (600), messages, form text | UI | 18; prose 1.45 |
| 15 | a title: dialogs, side sheets, empty states, a room on the phone; body on touch | UI | 20 |
| 19 | a section h2; stat numbers (mono) | display 700, caps, 0.01em | 1.05 |
| 32 | a page h1 (`PageHeader`) | display 800, caps, 0.01em | 0.95 |
| 38 | the Doc's own title | display 800, sentence case | 1.02 |

- **Tokens:** `--text-xs` … `--text-2xl`, `--text-doc`, and `--t-body` (13, 15 on
  touch) and `--t-field` (13, 16 on touch). A size that isn't a token is a bug.
- **Faces:** the UI face is Instrument Sans everywhere (its own change, after
  the sizes). Mono is `--f-mono`, never a literal `ui-monospace` stack. The
  display face is for 19 and up; a caps label at 11 is the UI face, not
  display or mono.
- **Fields are 16 on a phone**, or iOS zooms.
- **Off the scale, on purpose:** glyphs and avatar initials sized to their
  box, the terminal, the Doc's paper and body, the start code.
- A name someone wrote (a board, an issue, a Doc) keeps its case:
  `PageHeader named`.

Don't invent a half size (10.5, 13.5, 14), an `em` size, or a letter-spacing
other than 0, 0.01em and 0.08em.

## Colour

Colours are the palette's tokens (`--bg`, `--surface`, `--raised`, `--fg`,
`--subtle`, `--dim`, `--line`, `--line-strong`, `--accent`, `--ok`, `--warn`,
`--danger`, `--info`, `--merged`), never a hex or an `rgb()` in a rule.
Latte is drawn as `legible()` paints it (`shared/appearance.ts`), so test
contrast against that, not the palette as written.

- **Accent means live:** the thing you're in, on or choosing now. The "on"
  tint, the selected row, a count that's for you, the primary button, a
  fresh fill. Links are `--info`, not accent.
- **Status colours mean a state:** ok, warn, danger, merged, info. They go
  on a status chip, a tone bar or a dot, and a state is always said in
  words as well, never by colour alone.
- **Depth is three surfaces:** `--bg` for a page, `--surface` for a sidebar or
  a side sheet, `--raised` for a menu, a dialog, hover. Selection is a tint,
  not a surface.
- **One backdrop**, `--backdrop` (#733's value, 40% black), under every
  dialog, picker, menu sheet and side sheet. Glass keeps its blur on top of
  it; Getting started keeps its heavier look as a modifier.
- **The Doc wears the house pair** (Slate in dark, Latte in light, whatever
  palette is picked), so a shared Doc reads the same for everyone. Its paper
  is `--bg`, its reading ink a stronger text colour, and its named hues map
  to palette colours. *(being built: #729.)*

Don't add a palette, a `var(--x)33` hex suffix (it's invalid CSS and draws
nothing; use `color-mix`), or a `var(--x, #fallback)`.

## Buttons

`shared/buttons.css`. Three heights, whatever the area:

| size | height | class | where |
|---|---|---|---|
| bar | 24 | `.bar-btn`, `.bar-btn.sq` for an icon (24×24) | a pane's or block's bar, a card's ⋯, a toolbar |
| page | 32 | `.primary`, `.secondary` | a page, a dialog, a form |
| touch | 40 | the same, on `(pointer: coarse)` | a phone; segments are 40 |

- **One `.primary` per place.** It is the one thing to do. Everything else is
  `.secondary`; a quiet action inside text is `.link`. `.small` is a compact
  page button for rows and cards, not a fourth height.
- **⋯ is one size:** the bar icon. An overflow menu holds what doesn't fit; copy
  link and share come last before it.
- **Danger** is `.danger` beside a variant, and anything that takes
  something away asks first (`ConfirmRemove`).
- **Add buttons are plain.** `+ Add row` is a `.secondary.small` with a + icon.
  Dashed means "not there yet" or "drop it here", never "click me".

### The "on" state

One look, everywhere: the accent at 18% as the fill (`--on-fill`) with accent
text, 26% under the pointer. It's set by `aria-pressed`, `aria-selected` or
`aria-expanded` (or `.on`), not by a class of your own. Use it on a bg or
surface, never on raised: Latte's text fails there.

Don't invent another "on" (an inverted fill, a solid accent, an accent
border, bold).

### The segmented control

`.seg`: a `.secondary`-edged track, rounded segments with a 2px inset, and
the "on" tint as the thumb. Use it for a choice of one (Open/Closed, the
phone's tabs, Slate/Latte). `role="group"` with `aria-pressed` for a filter,
`role="tablist"` with `aria-selected` for tabs. Sizes: `.small` (24), default
(32), `.touch` (40). A row of actions (the pane presets) is a group of bar
buttons, not a segmented control, because nothing in it is ever on.

## Headers

Two, and nothing else.

- **The bar** is a pane's, a block's or a column's: 34px, 13/500 title, 24px
  ghost buttons, 1px rule under it. `PaneHeader`
  (`shell/ui/shell/pane-header.tsx`). A block's own bar sits under it at the
  same 34 and the same buttons. Chat's room header is a bar. *(being built:
  #686 part 2, a shared `Bar`.)*
- **The page header** is a place you go to: `PageHeader`
  (`shared/ui/page-header.tsx`). A back link, a 32px display title, a state
  chip, up to two labelled actions, a sub line and stats, then a 1px rule.
  Past two labelled actions the rest go into ⋯, and copy link · share · ⋯ are
  always last, in that order. A group inside a page is a section head
  (`SectionHead`, the 19px h2), not a second page header.
- State is a chip; a sentence ("on this machine") is the `sub`.
- **A Doc's own head** (its 38px title) is the document, not the app, and
  stays as it is.

Don't build a header out of a `div`, a 22px title, or a bar that wraps to a
second row.

## Rows and the selected state

*(being built: #756.)* Two shapes, with one selected state.

- **Flat row** (`.list-row`, `shared/ui/list-page.css`): a Docs or Agents row,
  a session, a chat room, an attention item. 13px title at 600, 11px tags.
  Modifiers: `.inset` (rounded, in a sidebar), `.dense` (26px, chat rooms),
  `.touch` (44px, at ≤600px or on a coarse pointer).
- **Card row** (`.sw-card`): a platform or Boards card, 13px. It opens a page
  or a sheet.
- A settings row is a list of things to open: the whole row is the button,
  with hover, and it is never selected.

**Selected** is the same on every shape: the `--selected` tint (accent at
14%, mixed over `transparent` so it works on any surface), a 3px accent bar,
and the title stays in the text colour. A card also gets a 1px accent edge.
A selected row keeps its tint under hover. **Hover** is raised at 45%.
**Focus** is a 2px accent outline, drawn for the keyboard only. **Unread** is
a 700 title and an accent count. Selection owns the bar: a selected row
shows the accent bar whatever its tone, and the tone comes back when you move
off. No glow, no solid accent fill, no different colour per area.

## Chips

`shared/ui/state-chip.tsx`. Five shapes, so a thing's shape says what it is:

| shape | use | is |
|---|---|---|
| status | a state: open, merged, failed, waits | `StateChip` / `Chip`, 11px with a 6px dot, a tone |
| tag | a label: `VM`, `shared`, `on the platform` | `Chip kind="tag"`, a raised box, no edge, 4px corners |
| count | a number | `Count`: neutral; accent when it's for you, warn for a mention |
| object | an issue, a PR, a person, an attachment | `ObjectChip`: mono id, title, a fact in its tone; dashed for proof |
| hint | a key hint | `HintChip` |

Round means a state or a count; square-cornered means a label. Touch: 28px
for status and tag, with a 44px target on anything you can press.

Don't add a `*-chip`, `*-tag` or `*-badge` rule outside the shared chip file.

## Dialogs, side sheets and the backdrop

*(being built: #690.)*

- **The dialog** is `Modal` / `.prompt` (`shared/ui/modal.tsx`): a 15/600
  title, a body, a right-aligned button row, Cancel first and the action last.
  Removing something is `ConfirmRemove` (`shared/ui/confirm.tsx`), including
  on a phone, at touch size. Don't build a dialog or an inline "Delete? Yes /
  No".
- **The side sheet** is one component with a head (a 15/600 title, an optional
  12px subtitle, ×, a rule), a scrolling body and an optional foot. Surface
  fill, a left rule, `--shadow-lg`, 440px wide, one z-index above panes and
  below dialogs. At ≤600px it is a bottom sheet (max 80vh, `--panel-radius` on
  top). Boards' row drawer, Add a pane and Needs you are all it.
- **The backdrop** is under every sheet at every width. Escape, or a click on
  the backdrop, closes it. So on a desktop a click on another card closes
  the sheet first.
- The phone's drop-down sheet hangs from the 44px phone bar, on the same
  backdrop.

## Menus

*(being built: #757.)* One menu: `openMenu` (`shared/ui/menu.tsx`). Chat's ⋯,
Boards' facets and the pane menu all use it.

- An icon column of 16px stroke icons (`rail-icons.tsx`), only in a menu where
  every row has one. ✓ is "several, and the menu stays open"; ● is "one of".
- A menu of 13 rows or more gets a filter row. A work-item picker stays a
  combobox (it takes pasted ids) and draws its list as the menu.
- At ≤600px it is a bottom sheet on `--backdrop`, with 44px rows at 15px. A
  coarse pointer wider than that keeps the dropdown at touch size.
- A menu inherits the page's one UI face; it has no face rule of its own.

## Empty and error states

*(being built: #692.)* One `EmptyState`: a title, a why, and at most one
`.primary`.

- **The title is a statement with a full stop** ("No rows yet."), 15/600. The
  why says what the thing is and where it comes from, in one or two
  sentences, at 13 (15 on touch). The button is the page's own verb and noun.
- **A page** gets a dashed card (`--line-strong`, `--card-radius`); **a pane**
  gets the same card, centred. No picture.
- **No button** when the action is already on screen (chat's composer, the
  phone's quick buttons) or the reader can't take it; the why says who can.
- **No match is a dim line**, not a card, with Clear. So is "None yet." in a
  settings section.
- **An error** is the same card with a solid danger edge and a 15/600 title
  starting "Can't…", and Try again as a `.secondary`. On a page it stands in
  for the content only when the whole list couldn't be read; a form error
  stays a red line by its field.
- **Phone, Here, nothing waiting:** one short dim line under the Needs you
  heading, "Nothing right now."

## Touch

At `(pointer: coarse)` or ≤600px, one rule for every piece:

| piece | size |
|---|---|
| button, segment | 40 |
| row, menu row | 44, text 15 |
| status chip, tag | 28, with a 44 target if pressable |
| body text | 15; fields 16 |
| phone bar | 44 |

Selected, "on" and pressed look the same as on a desktop. The phone gets a
size, not a look.

## Spacing

Use the scale: `--space-1` … `--space-6` (4, 8, 12, 16, 20, 24). Corners come
from the Corners setting (`--btn-radius`, `--tag-radius`, `--card-radius`,
`--panel-radius`), never a raw `6px`. A page header ends in a 1px `--line`
rule; a page's sections are separated by a section head, not a box.

## Open

**Is accent also a tone?** The rule above says accent means live, which is
"the thing you're in or on". Two places use it as a *tone* for a row that is
neither selected nor chosen: the Docs list's "open in a block"
(`shell/ui/docs-page.tsx`) and Boards' Review lane (`shared/boards/text.ts`).
An unselected accent-toned row draws the same bar as a selected one, and only
the tint tells them apart. Cards get past it with an accent edge; flat rows
don't.

- **A. Accent is only live/selected.** "Open" becomes a mark (the `%956` the
  row already shows) and not a bar, and Review gets a status colour. One
  meaning for accent.
- **B. Accent is also a tone.** Rows keep their accent bar, and the selected
  state has to differ from it by something other than the bar (a stronger
  tint, or an edge).

Until this is decided, don't add a new accent-toned row.
