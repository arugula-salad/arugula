# Cuts (the design lift)

What the design lift (plan §7.1) took out, one line each, so it can come back
if people miss it. Newest at the bottom.

- **Palettes:** Mocha, Platinum, Solarized light, Nord, Gruvbox, Tokyo Night and Dracula. Look's theme and sliders replace them (Arugula #1477).
- **Finishes:** Aqua, Brushed metal and Glass, and `platform/finish.css` (Arugula #1477).
- **Corners:** square and round; every corner is today's soft one (Arugula #1477).
- **Chat density:** compact rows (time, name and text on one line); rows are cozy (Arugula #1477).
- **"Use the Slate look":** Reset does the same (Arugula #1477).
- **The footer's machine, and the focused pane's repository and branch:** the crumb says where you are (Lift 3, Arugula #1479).
- **The footer's "N need you":** the rail's Inbox is the one count (Lift 3, Arugula #1479).
- **Slate and Latte's look:** the cyan accent on navy and the purple on cream; Arugula's green on near-black and olive on grey paper replace them, and Moss, Sea glass, Stone & ink and Graphite & acid are the other presets. Docs, terminals and editors follow since Arugula #1511.
- **Bundled fonts:** Instrument Sans, Big Shoulders Display and JetBrains Mono ("Arugula Mono"); the app uses Helvetica Neue and SF Mono from the system (Arugula #1482).
- **A stored slider setting from before the brand:** it starts again at Arugula's; the theme is kept (Arugula #1482).
- **The palette's other menu rows:** Go to… lists 25 commands (no clipboard ones, no single settings); the rest stay in their menus (Lift 3, Arugula #1479).
- **Ctrl+Shift+P and Ctrl+Shift+G:** ⌘K / Ctrl+K (Ctrl+Shift+K from a terminal) opens Go to…; Go to directory… is a command in it (Lift 3, Arugula #1479).
- **The switcher's Show archived:** archived projects are listed last in Go to…, marked archived (Lift 3, Arugula #1479).
- **The Machines rail count:** it counted unread run threads; Runs' own view still does (Lift 3, Arugula #1479).
- **Your picture on the rail:** the kit's Avatar draws your initials, on the rail and a phone's bar (Lift 3, Arugula #1479).
- **The team as the crumb's first step:** the crumb starts at the project; a team's pages are under Settings and Chat (Lift 3, Arugula #1479).
- **A board's Steps view:** the board is its lanes, on a phone too; an agent still orders a group's rows for `board_read`'s Steps block (Lift 5a, Arugula #1481).
- **The board's sidebar:** View, Find, Docs, Add and Workers go; Filter and Add row are in the board's title block, the Docs are its tabs, and Link a Doc, Add a group, New issue, Copy link and Workers are in its ⋯ (Lift 5a, Arugula #1481).
- **A row's drawer in the right column:** it's a sheet over the board; the right third is Needs you and the lead (Lift 5a, Arugula #1481).
- **A board's made and edited times, and Open in browser:** the title block has the lead, the machine and the progress (Lift 5a, Arugula #1481).
- **Every project's Boards (`/boards`):** Go to… finds a board; `/boards` opens the current project's Boards, and a board's old address its page (Lift 5a, Arugula #1481).
- **A board beside the Boards list:** a board opens on its own page (Lift 5a, Arugula #1481).
- **A board tile's chips:** its labels, assignees' initials, attachments, sessions, Docs, earlier agents, note and marks as chips; a tile is its mark, number, state word, title and one meta line, and the drawer has the rest (Lift 5a, Arugula #1481).
- **"no proof" on a Done tile, and a tile's fill by lane:** a proven one says "proven"; tiles are one card colour, with an amber edge when one waits on you (Lift 5a, Arugula #1481).
- **The Issues page, and its stats line:** the project's issues are All issues, a row of Boards (`/p/:project/issues` selects it), with Open/Closed, search, Label, Assignee and Export as before (Lift 5a, Arugula #1481).
- **Not on a board when it's empty:** its row shows only while some open issue is on no board (Lift 5a, Arugula #1481).
- **An issue's label and assignee Edit buttons:** the labels and the people are the buttons (click to edit), in the card's properties (Lift 5a, Arugula #1481).
- **The New issue page:** New issue and Add row are a sheet (title, description, Start an agent on it); labels and assignees are set on the issue after; `/p/:project/issues/new` opens All issues with the sheet (Lift 5a, Arugula #1481).
- **A board's New issue in ⋯:** Add row opens the issue and puts it on the board; Add an existing item… adds an issue or PR that's there already (Lift 5a, Arugula #1481).
- **The Overview's zoom:** the Project / Board / Work pill, the board and work levels and their transition; a board's card opens its board, and the old zoom addresses land there (Lift 4, Arugula #1480).
- **The Overview's divider:** the right column is a third, fixed (Lift 4, Arugula #1480).
- **The briefing line:** "since 1:35 AM…" and its counts from the event log; the chief of staff's room says what moved (Lift 4, Arugula #1480).
- **A board's cards in flight on the Overview, with their previews:** the board shows them (Lift 4, Arugula #1480).
- **Not on a card:** its sessions are on Machines; an ask from one is a Needs-you row saying "not on a card" (Lift 4, Arugula #1480).
- **Your work on the personal project's Overview:** your Claude Code and Codex conversations are on your machine's page (Lift 4, Arugula #1480).
- **A board's lead chip and thread button on the Overview:** the board has both (Lift 4, Arugula #1480).
- **Alerts in Needs you** (checks or a run failed): the board's and Running's (Lift 4, Arugula #1480).
- **The proposals' cards in Needs you:** an approval is a Needs-you row, answered there; its card stays in the staff room (Lift 4, Arugula #1480).
- **Answering a form, or several questions, from Needs you:** Open takes you to the pane, which answers it (Lift 4, Arugula #1480).
- **`web/screenshots/dive.mjs`:** the README's dive.gif toured the zoom, which is gone (Lift 4, Arugula #1480).
- **The talk panel's who-line** ("Talking to … : a message goes straight to this agent"): the panel's head says who, where and how it stands, and the composer's line says where a message goes (Lift 4, Arugula #1480).
- **An agent's rung line, who started it and its parent's last word** (#1441's head): the head says its rung (lead, worker, chief of staff) and machine; the threads it's told things in are a tab of their own (Lift 4, Arugula #1480).
- **An issue agent's tab with its issue's thread over its peek** (#1441): the issue's thread is a tab of its own in the panel (Lift 4, Arugula #1480).
- **Open in Panes ↗ and Board thread on a peek:** the Terminal (`v`) shows the pane live in the panel and the floor (`f`) makes it full screen; Machines still opens it (Lift 4, Arugula #1480).
- **A peek's card link** (`#12` in its head): the card's board shows it, and Needs you's ↵ opens it (Lift 4, Arugula #1480).
- **The talk panel's saved tabs from before** (`arugula.ov-talk:*` in session storage): the panel keeps its own (`arugula.ov-panel:*`); a browser tab open over the upgrade starts with the chief of staff (Lift 4, Arugula #1480).
- **The panel's Needs-you column when collapsed** (`\`): the right column folds to a strip, Needs you with it; `\` or the strip brings both back (Lift 4, Arugula #1480).
- **A thread's own head in the agent panel** (its "Thread" line, its ×, its Follow button): the panel's head says where you are, its one Back (and Esc) closes the thread, and Follow is in the ⋯ beside Back (Lift 4, Arugula #1480).
- **The panel's tabs and head on the floor** (`f`): only the transcript, room or terminal goes over the page, with a ⤢ to leave (Lift 4, Arugula #1480).
- **Chat's own composer look in the panel's rooms** (its letterbox field and square send): the kit Composer's glow, round send and target line, with chat's mentions, files and drafts (Lift 4, Arugula #1480).
- **A Claude Code terminal's raw lines in the panel's transcript:** its tool calls fold to a line each; its banner, prompt box, spinners and timings are left out (the Terminal, `v`, still shows them) (Lift 4, Arugula #1480).
- **`AskCard`, `QuestionCard` and `FormCard`** (`shared/ask.tsx`) and their CSS: an agent's question, form or sign-in link is the kit's NeedRow, with the same answers (Lift 4, Arugula #1480).
- **Swarm's rail cards** (their kind colours, pop-in, uppercase titles and tinted frames): the rail is a list of NeedRows, as Needs you's (Lift 4, Arugula #1480).
- **A board block's ask card** (its amber fill, "Asks" chip, folded Why and "Waiting for Sam, asked by …" line): the tile's ask is the kit's NeedRow, as on the desk and the card page, whose buttons are the asked person's (part of Arugula #1523).
- **All issues' issue cards** (their state-coloured bars, the bold title, the "Closed" chip and the 20px avatars): each issue is a kit Row, ○ open or ✓ closed, #N, title, labels, who and when (Arugula #1521).
- **Not on a board's own row look** (the dimmed select box until hovered, the accent-tinted selection, labels hidden on a phone): the kit Row, with its select box before it (Arugula #1521).
- **The terminals' and editors' Slate and Latte colors:** a terminal, an editor block and nvim are drawn from the look, on the well; a Doc from Arugula's own pair, whatever the look (Arugula #1511).
- **Settings' list and detail** (a list of sections beside the chosen one, its way back on a phone, the section index on a daemon's page): a scope is one list, each section a row that opens in place (Lift 7, Arugula #1487).
- **Settings › You › Appearance:** Look (`s`, the top bar's Look) is the one place for theme, preset and sliders; Tips, the editor and lending this device are Preferences (Lift 7, Arugula #1487).
- **The daemon's `#settings` Appearance section:** Look, as in the app (Lift 7, Arugula #1487).
- **The scopes' "whose" lines** under Settings' header ("Yours, wherever you sign in…"): the tab says the scope (Lift 7, Arugula #1487).
- **Machines as a scope in Settings' switch:** a machine's settings open from Machines (its Settings…), at the same address (Lift 7, Arugula #1487).
- **Settings › Project › Machines** (the join token): Machines' Add a machine (Lift 7, Arugula #1487).
- **Settings › Project › CI alerts, Runs and workspaces, Claude spend** as rows of their own: parts of one Agents row (Lift 7, Arugula #1487).
- **The Team page** (`/t/:team/chat`, `/members`, its tabs): its members are Settings › Team, its chat the team's project's chat (Lift 7, Arugula #1487).
- **The avatar menu's phone-only Your account…:** the menu has each of Settings' scopes on every screen (Lift 7, Arugula #1487).

## #2947 (Lift 6b, the Docs list)

- **The shell's Docs page (`#docs`):** a machine's Docs are in Go to… by title, and the app's Docs are the rail's; an old `#docs` lands on one of them (Lift 6b, Arugula #1484).
- **My Docs and Team Docs tabs:** one list, with a Mine filter in a team (Lift 6b, Arugula #1484).
- **The Docs list's sort (Updated or Created), search, time groups and preview pane:** the latest changed first; Go to… finds a Doc by title; a Doc opens on its page (Lift 6b, Arugula #1484).
- **A Docs row's ⋯ and the list's ticks (Move several to a team):** a Doc's ⋯ on its page has the same, one Doc at a time (Lift 6b, Arugula #1484).
- **The Docs page's empty state with Start an agent:** the list's empty state says where Docs come from (Lift 6b, Arugula #1484).

## #2950 (Lift 7, Deploys)

- **The Deploys place** (`/p/:project/deploys`, a project's deploys newest first): each environment's history is in Settings › Project › Secrets and environments, where the address lands; a deploy waiting on you is a Needs-you row (Lift 7, Arugula #1487).
- **"Plan a deploy" (`/p/:project/deploys/new`) and each environment's Deploy…:** an agent's `plan_deploy` or `arugula deploys plan` plans one; the address lands on the environments (Lift 7, Arugula #1487).

## #2953 (Lift 6d: the Inbox)

- **The Inbox's old list** (its swarm-coloured cards, "N need you · N items" stats, the Decide / Names you / Chat headings): every project's items under their project, each a kit NeedRow answered in place (Lift 6d, Arugula #1486).
- **The Inbox's All projects toggle** (`[data-all-projects-toggle]` on `/inbox`): the Inbox is always every project's; `/inbox?all=1` still opens it (Lift 6d, Arugula #1486).
- **Deploys waiting for you as cards on the Inbox:** a deploy's row opens its page, where it's approved (Lift 6d, Arugula #1486).
- **The Inbox badge's @ for a waiting deploy and its "N deploys wait" tip:** the badge is the server's count, as it is (Lift 6d, Arugula #1486).

## #2951 (Lift 7, the legacy token names)

- **The legacy token names** (`--bg`, `--fg`, `--dim`, `--surface`, `--raised`, `--ok`, `--warn`, `--danger`, `--info`, `--text-xs…2xl`, the radii …): every rule names the role it aliased, `tokens.css` holds only the roles, and check-colour refuses the old names (Lift 7, Arugula #1487).
- **Five colors no role covered:** `--subtle` is `--ink-dim`; a merged change (`--merged`, purple) is `--done`; `--teal` and `--pink` are `--review`; `--peach` is `--needs` (Lift 7, Arugula #1487).

## #2952 (Lift 7, Getting started)

- **Getting started's Welcome step** (right-click, drag to split, Jump anywhere, Edit in nvim, the CLI line): the hints say each at its moment; Edit in nvim is in + Pane and Settings (Lift 7, Arugula #1487).
- **Getting started's Agents step** (each adapter's state, Start an agent, What your agent can do, the Claude Code plugin's two lines, the hooks check): adapters are the Start-an-agent dialog's and the agent block's, what an agent can do is that dialog's, and Claude Code's one click is under Add a machine where it isn't set up (Lift 7, Arugula #1487).
- **Getting started's Things to try** and its done marks (`seen:getting-started.try.*`), **the step rail, Back/Next, and the "it's in Go to…" toast**: Getting started is one screen, Add a machine (Lift 7, Arugula #1487).
- **Getting started's Make a team / Share a session** after joining: Settings › Team and a session's Share (Lift 7, Arugula #1487).

## #2954 (Lift 6d: no bell)

- **The bell on a machine's page** (`[data-open-attention]`, its count of what needs you on every machine): the rail's Inbox badge is the one count; the attention drawer still opens from Go to… and the sidebar until Lift 6a (Lift 6d, Arugula #1486).

## #2961 (Lift 5b: the lead arranges the board)

- **5a's fixed board tabs** (each linked Doc and each PR opened from the fold, `[data-board-tab-doc]`/`[data-board-tab-pr]`): the board's tab is its own, and Docs and PRs open as panes beside it, from `+ pane` or the fold (Lift 5b, Arugula #1497).
- **The panes' keys** (v7's `5` `6` for tabs, `[` `]` pane focus, `w` close a pane, `W` close a tab, `f` maximise, `g 0` Reset): no new keys until Aaron's list (#1534). The panes' buttons, the tab's ×, Reset in the header and ⋯ do each; `1`–`4` and Esc stay (Lift 5b, Arugula #1497).
- **A preview drawn inline**: a preview pane is a link to it, until previews have a source to frame (Lift 5b, Arugula #1497).
- **Dragging a pane onto the tab strip**: the pane's move button puts it in a tab of its own (v7's weak spot; Lift 5b, Arugula #1497).
- **Which pane is maximised, across pages**: maximise marks the pane yours on the platform, but which one fills the board is this page's (Lift 5b, Arugula #1497).

## #2968 (#1520: the board's right third on the agent panel)

- **The lead head's Terminal button** (`[data-board-lead-open]`, which went to the lead's pane on its machine's page): the Lead 🔒 tab has Transcript and Terminal (`v`) in the panel, and the panel's ⤢ (#1520).
- **The board's thread as the right third's only content**: it's the board's channel, behind the board tab's ⇄ (#1520). A page without the agent panel keeps it as it was.

## #2965 (Lift 6c: Chat on the kit)

- **Chat's rooms head ("Arugula", with its ⋯):** the bar's crumb names the project and the room; ⋯ (Mark all as read, sections, unread only) sits beside search (Lift 6c, Arugula #1485).

## #2970 (#1520: cards open their page, workers in the panel)

- **A tile's body opening the drawer on the board's page**: it opens the card's page; ⋯ → Details opens the drawer (#1520). A PR's, a link's or a note's tile, and the board block's, still open the drawer.

## #2966 (Lift 6c: Chat's Agents section)

- **About's "What a lead reads":** the prototype's `board_read` excerpt beside #agents; it comes with the server's board_read and capabilities wording (Arugula #1522).
- **A lead's active or idle state:** a board's dot says only whether it has a lead, until the server says which leads are active (Arugula #1522).

## #2967 (Lift 6c: tags in the agents' rooms)

- **Counts in the @ list** ("every agent here · 8", "the active ones · 4"): nothing on the wire says who's in a channel or active yet (Arugula #1536).
- **Green tag pills:** tags are Chat's mention pills; a pill of their own would be a new chip (Arugula #1522).

## #2973 (Lift 6c: rooms as panes on a board)

- **#agents as a pane on the Overview:** it opens beside a board (+ pane, `g a`); the Overview's panes come with Lift 9 (Arugula #1498).
- **Chat's link back to its rooms list in a pinned pane:** the pane is the room alone; Chat is a rail click away.

## #2983 (Lift 6a: the Machines list)

- **A session's card listing every tab and pane, "+N more tabs"**: a session is one row; only the panes that need you and the one you had open last show under it, and the session opens at its last tab (Lift 6a, Arugula #1483).
- **Agents only** (`[data-machines-agents]`, `?agents`): every session shows; the old address lands on the list (Lift 6a, Arugula #1483).

## #2987 (Lift 6a: no Swarm)

- **Swarm** (`shell/swarm/`: the field, city, hive, flat, timeline and wall views, the follow view, the board views, the synthetic fleet, `window.__arugula.swarm`/`swarmFake`), its CSS and the three.js dependency: Machines and Running take its place; `#swarm` and `/p/:project/machines/swarm` land on Machines (Lift 6a, Arugula #1483).
- **Swarm's rail cards for a gate or a decision**: the gate shows in its workspace block, on the phone's sheet and in Needs you, which answer it (Approve, Expire; a decision point's choices) (Lift 6a, Arugula #1483, #1546).
- **Swarm's site screenshots** (`swarm.png`, `swarm-phone.png`): `web/screenshots/machines.spec.ts` makes Machines' instead; the site moves in #1544 (Lift 6a, Arugula #1483).
- **The `swarm` view in usage counts**: an old `#swarm` isn't a view any more (Lift 6a, Arugula #1483).

## #2990 (Lift 6a's last removals)

- **The activity drawer** (`shell/ui/shell/activity.tsx`: a session's commands, answers, agent turns and system events under its tab, its filters, All sessions, its footer toggle and its first-open line): `arugula history` reads the same (Lift 6a, Arugula #1483).
- **The shell's Agents page** (`#agents`, `#agents=team`: a machine's recipes with New, Edit, Run here, Offer and Delete; the team's catalog; tasks waiting for you; standing grants). An old link lands on the panes. A machine's agents and the recipes it offers stay in its agents block, and its grants stay in Access (Lift 6a, Arugula #1483).
- **The places bars** (`places.tsx`'s Places: Panes · Chat · Boards · Docs · Agents; MachinePlaces: Machines · Runs · Recipes in a machine's bar): the rail and Go to… have the places. Off the rail, Settings keeps a Panes button; the helpers are `shell/ui/place-links.ts` (Lift 6a, Arugula #1483).
- **Agents only** (`/p/:project/machines?agents`, `machine-list`'s `only`): an old address is the Machines page (Lift 6a, Arugula #1483).
- **The attention drawer** (`shell/ui/shell/attention-drawer.tsx`: what waits on you on every machine, answered in the drawer; its empty state's Set up for Claude Code's hooks and the device's notifications; the sidebar's Needs you heading as its button). Running and Needs you cover it; Go to…'s Needs you opens the Overview's where there's an app, and its next pane that needs you stays. Set up stays in the agents nudge (Lift 6a, Arugula #1483).
- **Columns, the "N panes" switcher and Focus in a tab's head line**: Tamed, Grid and + Pane stay. A pane's own Focus (its header, the key) stays, and Go to… lists the panes (Lift 6a, Arugula #1483).
- **The Runs list** (`/p/:project/runs`: the runs newest first, narrowed by state, trigger and spec; their counts; the Runs tab under Machines and its thread count). What runs now is the Running drawer's, and the old address opens it (`?running`); a spec's (`?spec=`) opens Settings' Runs, where New run and the run specs with their triggers are now. A run keeps its page (Lift 6a, Arugula #1483).
