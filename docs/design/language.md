# Design language: the kit

**See it:** [`/design`](https://app.arugula.io/design) in the platform app
draws every part below with the real component, one section each, in the
same order as the headings here. Its Look button (`s`) repaints the page in
either theme and along the three sliders, so you can check a part
everywhere it will be drawn. The code is `web/src/shared/kit/`, one file per
part, and one block per part in `kit.css`.

Status: the design lift's kit (plan §4.1, Lift 2, #1478), drawn to the v6
prototype. Screens move onto it lift by lift; until a screen's lift has
merged, work on it keeps today's pieces ([today.md](today.md)). AGENTS.md
("Building UI") says which screens have moved.

**The rule behind all of it: build from the kit; if a part doesn't fit,
change the part, not your screen.** A fifteenth part needs a design-session
sign-off in its PR, and deletes one.

**The material:** the canvas; tactile containers (a section, then cards on
it); flat content (rows, words); the well only for terminals, agent panes and
something to copy.

**Never colour alone.** A state is a mark and a word; a need is the ◆, the
word "asks you" and a thin edge; picked is a ring or a bar as well as a fill.
**Every action has a key**, shown as a `Kbd`. **The focus ring** (2px of
`--focus`) shows for the keyboard on every part.

## Tokens

`shared/kit/tokens.css`, painted by `shared/kit/look.ts` from the theme and
the sliders (light↔dark, cool↔warm, flat↔deep). New CSS names a role, never
a value or one of today's names (`--bg`, `--fg`, `--surface` … are aliases
until each screen's lift drops them).

| group | roles |
|---|---|
| surfaces | `--canvas` `--rail` `--section` `--card` `--card-raised` `--paper` `--well` |
| ink | `--ink` `--ink-dim` `--ink-faint` `--well-ink` `--well-dim` `--on-accent` `--on-needs` |
| lines | `--line` `--line-strong` `--hover` `--focus` |
| signal | `--accent` (brand, working, primary, send) `--needs` (asks or waits on you) `--needs-tint` `--fail` `--fail-tint` `--review` `--done` `--backdrop` |
| depth | `--shadow-card` `--shadow-raised` `--highlight` `--sheen` `--glow` |
| type | `--font-sans` `--font-title` `--font-mono` `--font-id` `--font-meta`; `--text-label` `--text-meta` `--text-body` `--text-title` `--text-hero` `--text-doc` |
| shape | `--radius` `--radius-control` `--gap` `--row` `--rail-w` `--panel-w` |

- **Where mono goes is a token, not a class change.** Put `.m-code`, `.m-id`
  or `.m-meta` on the element; `--font-id` and `--font-meta` decide whether
  ids and meta are mono.
- **Amber is a highlight, never a fill**: the mark, the kind word, "asks
  you", a 2px edge. Red likewise.
- `look.ts`'s `fix()` keeps every word at AA (4.5:1) whatever the sliders.

## Surface

`Surface` (`surface.tsx`): `section`, `card`, `paper` or `overlay`. A section
holds cards; a card is the thing you act on; paper is a Doc's page; an
overlay floats (a `Sheet` is one). `current` rings the open one. Nothing else
draws a box: no `-card` class outside the kit.

## Row

`Row` (`row.tsx`): one flat line, mark · kind · title · who and meta · age,
on a section or a card, never a card of its own. `plain`, `need` (the amber
tick), `selected` (a fill and a bar; `aria-current`) or `done` (dim). The
whole line is one button; a row that expands says so (`aria-expanded`) and
holds its body under the line.

## NeedRow

`NeedRow` (`need-row.tsx`): the A2P row, one for the Overview, the Inbox, a
board, a card and Machines. It takes the wire's item (`kind`, `from`, `for`,
`subject`, `lines`, `answer`, `escalated`, a to-do's `how`) and answers in
place:

- **ask:** the question and its options. **permission:** the command in a
  well, Allow / Deny. **trust:** the folder, Trust / Not now. **approval:**
  Start / Not now. **to-do:** why, the how in a well with Copy, a note marked
  "no secrets", Done / Can't.
- The keys are `y` and `x` (`D` for Done), shown on the buttons; the
  screen's key table binds them.
- It says who: "asks you", "waits on you", or "asks Jake" (neutral: no amber,
  no answer buttons).
- Answered, it stays as a dim receipt: "✓ Truncate middle by you now · note".
- **It works at a third of the screen and on a phone (360–480px).** Put rows
  in a `NeedList`: under 480px each is two lines, the mark and the title,
  then the kind, who and age.

## Well

`Well` (`well.tsx`): inset, flat, mono, its label set into the frame; the
frame brightens while focus is inside. Only for a terminal, an agent pane, or
something to copy.

## StateMark

`StateMark` (`state-mark.tsx`): ◉ working (breathes) · ◆ asks · ◇ waits ·
▲ stuck · ✕ failing · ✓ done · ○ idle, always with its word. `Mark` is the
mark alone, for a row that says the state in words elsewhere.

## Crumb

`Crumb` (`crumb.tsx`): where you are, as a path you can climb. The first step
is the project; the last is here and isn't a link. An id step is in the id
face.

## Kbd

`Kbd` (`kbd.tsx`): a key as a small raised chip; `"g o"` is two. A button
whose action has a key shows it inside (`Button keys`).

## Avatar

`Avatar` (`avatar.tsx`): a person is their initials on a disc; an agent is
the ✻ mark on a well. Its name is its label.

## Panel

`Panel` (`panel.tsx`): the agent panel on the right: its tabs, a head (who,
where, what it's doing), the transcript or a well, and the composer.

## Composer

`Composer` (`composer.tsx`): where you write to an agent or a room. It glows
while you're in it; ↵ sends, ⇧↵ is a new line; the line under it says who it
goes to.

## Tabs

`Tabs` (`tabs.tsx`): an icon, a title, a count and a close. ← → Home End
move between them; Delete closes the focused one. Panes beside each other
come with the board (Lift 5b).

## Sheet

`Sheet` (`sheet.tsx`): the one overlay, over `--backdrop`: Go to…, the keys,
Look, a menu, a confirm. Focus goes in and stays in; Esc or a click on the
backdrop closes it, and focus goes back.

## Button

`Button` (`button.tsx`): `primary` (one to a place), `plain` or `danger`.
`keys` shows the key that does the same and sets `aria-keyshortcuts`.

## Field

`TextField`, `Toggle` and `Choice` (`field.tsx`). A text box carries its label
and may end in a hint ("no secrets"); a toggle takes effect now
(`role="switch"`); a choice is one of two to four, as segments the arrow
keys move through (`role="radiogroup"`).

## Guards

- `check-cards`, `check-chips` and `check-fields` let the kit draw a card, a
  chip or a field.
- `check-type` and `check-colour` hold the kit to the size and colour roles
  like everything else.
- `kit.spec.ts` checks every part on `/design`: it renders, shows the ring,
  never says a state in colour alone, and works by keyboard.
