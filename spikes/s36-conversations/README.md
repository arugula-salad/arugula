# S36: conversations that link panes across machines, driven by a lead agent (#852)

Run 2026-10-13 on geek, against `main` at 0.38.1 (a8043362). Issue #852; tracker #851.

**Result: go.** The prototype ran end to end on a local rig: one control and two machines of one
account, box-a and box-b, each reaching the other only through the relay. The conversation lived on
box-a and linked Claude Code (haiku) in a pane on each machine.
- `@lead have L1 and L2 each run python3 -m pytest -q and fix what fails…` went to the lead, a
  Claude Code agent block on box-a. The lead prompted both links and waited on them.
- L2's three permission prompts (Bash, Edit, Bash) came through the relay into the timeline and were
  answered there.
- Both links' turn-ended events came in, and the lead reported what changed.
- Both repos were fixed. The run took 41 s ([results/flow.txt](results/flow.txt),
  [results/timeline.txt](results/timeline.txt)).

The prototype is throwaway code on this branch, behind the labs `chat` flag:
- `ThreadTarget::Conv` in `crates/proto/src/lib.rs`;
- conversations kept beside the threads in `crates/mux/src/labs/threads.rs` and
  `crates/mux/src/mux/thread_ops.rs`;
- `crates/daemon/src/labs/convs.rs` (routes, link grants, watchers, the lead, MCP link names);
- `crates/daemon/src/e2e.rs`, which lets a granted link through;
- `crates/daemon/src/peer.rs`, with long and streamed requests;
- small type fixes in `web/src` for the third target.

The rig is [rig.ts](rig.ts), [setup.sh](setup.sh), [flow.sh](flow.sh), [c.sh](c.sh) and
[restart.sh](restart.sh). No web UI was built: everything was driven over HTTP.

**Not proven:**
- a real second machine (jake-mini). The local rig's two daemons talk only through control's relay,
  the same path, so jake-mini wasn't needed;
- a teammate's machine;
- the phone and push;
- the Chat page;
- a lead woken by a link's turn ending after its own turn ended. This is built, but in the final run
  the lead waited inside its turn instead;
- a lead starting sub-agents.

## Answers

1. **Identity and storage.** The id is 16 random bytes, written `c-` plus 32 hex digits
   (`ConvId`, which is `Copy`, so `ThreadTarget` stays `Copy`). A conversation is a third
   `ThreadTarget`, `Conv(id)`. Its record sits in `<state>/threads/conv-<id>.json` and its timeline
   in `conv-<id>.jsonl`, next to the pane and session threads.
   - `read_thread`, `post_thread`, unread counts, mentions, `ServerMsg::Thread` and history search
     worked for it with no changes beyond the match arms.
   - Existing threads aren't moved, and a pane thread doesn't become a conversation: the two stay
     side by side (q1 below says why).
   - "From now" becomes a per-member `joined` time.
2. **Links across machines.**
   - **What a link stores:** the machine's id (the prototype stored its name, which breaks on a
     rename), the pane, the pane's `epoch`, and a title. A pane that closes stays in the conversation
     as a closed link.
   - **How it's reached:** the M77 relay channel, *not* A2A tasks. The linked machine keeps a
     **link grant** (this daemon, this pane, this conversation). The grant lets the home daemon's
     channel through for that pane's `prompt`, `wait`, `capture`, `events` and `act`, and nothing
     else. Without a grant the machine refuses, as it did in the run.
   - **A teammate's machine** needs the home account routed to it, as M77's `agents` list does.
3. **The lead's scope.** The block token plus "the links of the conversation this block leads",
   addressed by name (`L1`, `L2`), never by pane number.
   - **Tools:** `prompt_agent`, `wait`, `read_output`, `read_thread`, `post_thread` (which default to
     the conversation), and `list`. Later, `start_agent`, whose block becomes a link.
   - **Not given:** `agent_respond` on links (people answer cards), `run`, `send_input`, or anything
     outside its tab.
   - The first run showed why links need names: the lead was shown pane numbers and drove its own
     `%3`, which happened to share box-b's number.
4. **Owner's rights, made safe.**
   - The invite and the members list say plainly that members can have the lead type into and read
     every linked pane.
   - With an owner's-rights lead, hiding a linked pane's screen from some members doesn't hold: any
     member can ask the lead to describe it. The recommended rule is that a member sees everything
     linked, with a warning when a link is added and when someone is invited (For Jake 1).
5. **Events into the timeline.**
   - **Signals:** the pane's attention events (working, done or idle, needs input with a reason)
     and its exit, from the mux's event bus at home, or from `/api/events?pane=N&follow=1` streamed
     through the relay. That ran in the prototype.
   - **Rules the run forced:**
     - a turn ends only after the link worked (a prompt sets it idle first);
     - only a card (`ReasonKind::Ask`) is "needs input". Claude Code's "waiting for your input",
       a minute after each turn, is noise;
     - the same line twice is one line.
   - **Cards** are answered through the linked machine's own `act`, over the relay. It worked three
     times in the run.
6. **The lead's context.**
   - **Each `@lead`:** the message, the link events since its last turn, the links by name, and how
     to answer. The standing brief goes in the system prompt.
   - **A link it waits on** wakes it when that link's turn ends.
   - **Restarts:** the agent block resumed its session across two box-a restarts. But the brief is
     sent only on `session/new`, so the per-turn context has to carry everything, and the "since
     last turn" cursor has to be stored (it was in memory).
   - **Long conversations:** Claude Code compacts its own context. The timeline is the record, and
     `read_thread after` pages it.
7. **The Chat page.**
   - A *Channels* section above the machines, listing the conversations from every machine's
     threads.
   - *New conversation* in its header, with home set to the machine you're on.
   - *Add to conversation…* in `threadItems()`, the pane menu.
   - A header of link chips with their state, plus the lead.
   - Cards render inline with Allow and Deny.
   - The web client switches on pane-or-session in 10 places (all patched here to compile).
8. **Phone and push.** Today `notify_mentions` needs a pane, and the phone's tap only knows
   `pane-N` and `session-N`. A conversation's notification needs:
   - its own `thread: conv-…` and a `#chat=…` tap URL;
   - a push for each card, to members;
   - the lead's reply to an `@lead`, notifying whoever asked.

## The rig

- One local `arugula-control` with a fake GitHub, and one person, `jake`.
- Two machines of his joined to it: `box-a` (home) and `box-b`. Each has Labs on and its own state
  dir and port, and is reached by the other only through the relay. `--shell "bash --norc"` keeps
  the panes on this checkout's `arugula`.
- Claude Code 2.1.296 under geek's own login, with the user's hooks:
  - `arugula hook` puts permission prompts on cards;
  - `arugula attention done` marks a turn's end.
- L1 runs in auto mode. L2 runs in `--permission-mode default`, so it asks.
- Each machine's `calc.py` has a failing test.
- The environment is stripped of `ARUGULA_*`/`CLAUDE*`, and every call names a rig port and token.
- Rig quirks:
  - Restarting a rig daemon kills its panes' programs: there's no systemd user manager here, so
    panes come back as shells.
  - A machine that joined after the home one is unreachable until the home daemon's trusted set
    refreshes. That took about two minutes (`REFRESH` is 60 s).

## q1: identity and storage

**The id.** `ConvId([u8; 16])`, written `c-` plus 32 hex digits, serialized as a string (`ts` type
`string`, so JavaScript never sees a 128-bit number).
- It's global without asking anyone.
- The home machine isn't part of the id: it's where the record is. Control's directory and the link
  grants name it.
- `ThreadTarget::Conv(ConvId)` keeps the enum `Copy`. Its key is `conv-c-…`, so `/api/threads/{target}`
  and the files need no new routes.

**Storage**, in `<state>/threads/`:
- `conv-<id>.json`: the record (title, owner, members, links, lead). It's written whole on each
  change.
- `conv-<id>.jsonl`: the timeline, as any thread's, with numbering, cutting and torn-line recovery
  unchanged.
- `reads.json`: the read marks, shared with pane and session threads.

**What worked unchanged once the match arms were added:**
- `ThreadPost` and `ThreadGet` (HTTP and MCP);
- mentions, and which `@`s landed;
- `threads_for` (unread and mention counts);
- the `ServerMsg::Thread` broadcast to every client whose person may read it;
- history search over threads (`history.rs` gives a conversation pane 0).

`thread_role` gives the owner `Owner` and a member `Editor`, from the record's own list. There's no
session ACL.

**Threads stay where they are.** The brief proposed that a pane thread become a one-link
conversation. The code argues against it:
- a pane thread's access *is* the pane's (`thread_role` follows `readable`), and its unread marks,
  invites (#297) and push target are all keyed by the pane;
- a conversation's access is its member list.

Folding one into the other moves every pane thread under a model it doesn't need, for no gain the
Chat page can't get by listing both. **Recommendation:**
- pane and session threads stay as M61 made them;
- *Add to conversation…* on a pane links it and copies nothing;
- a conversation's header links back to each pane's own thread.

**"From now".** Pane and session shares have `thread_floor`. A conversation needs the same per
member: `members: [{who, joined}]`. A member sees messages from `joined` unless the owner adds them
"with history". The prototype has no floor (0).

## q2: links across machines

**What a link stores:** `{name: "L2", machine, pane, epoch, title}`.
- **`machine`:** its device id, not its name. The prototype stored the name, and `peer::find`
  resolves names, so a rename would orphan the link. The name is kept only to show it.
- **`epoch`:** the pane's own (`PaneSummary.epoch`). A restored pane keeps its id. Comparing the
  epoch tells "the same pane, restarted" from "a new pane with the same number".
- **When the pane closes:** the watcher posts `exited`, the link is marked closed in the header and
  stays in the timeline, and the lead is told when it next runs. A closed link isn't reused.
- **When the machine is offline:** the watcher retries every few seconds (it did across box-a's
  restarts), and the header shows the link as offline.

**Which channel.** The home daemon reaches the linked machine exactly as M77's catalog does: signed
with its own key, through control's relay, Noise end to end.
- Today the far daemon answers `/api/a2a/` only (`serve_agent_caller`, `agents_only`).
- The prototype adds one more case: a request for a **granted** pane's `prompt`, `wait`, `capture`,
  `events?pane=N`, or `attention/act` with that pane in its body (`convs::linked_request`).
  - From a machine of the same account, it runs as the owner.
  - From any other account, it runs as that account's own principal, so the ACL still applies.
- Everything else still gets "a machine reaches only the agents here". The run checked both cases:
  before the grant, and on box-b's unlinked `%1`.

**The grant** lives on the linked machine, `<state>/conv-grants.json`: `{device, pane, conv, at}`.
- In the prototype the owner made it with the machine's local token.
- In the product, the person who links the pane makes it from their own device. *Add to
  conversation…* calls the linked machine's grant op as them, and that op requires that they may
  drive the pane (`Access::Pane(Editor)`). So "you can only link a pane you may drive" is checked
  where the pane is, not at home.
- Requests under the grant then run with **the grant maker's rights on that pane**, capped at
  Editor. They don't run with the conversation owner's rights: the owner may have none on a
  teammate's pane. A grant is listed on the pane and revocable there, and revoking it cuts the link.

**Why not A2A tasks (M78).** A task is a *new* agent block in a worktree, with its own consent, its
own result (a patch), and a lifetime of one request.
- A link is an *existing* pane that people are already working in. It's driven turn by turn, its
  cards are answered live, and it lives as long as the conversation.
- Nothing in A2A's task model maps onto "type this into Claude Code in %2 and tell me when its turn
  ends".
- A2A stays the way to hand a whole job to someone else's agent. A conversation can link the A2A
  task's block once it exists.

**What the relay needs:**
- **A standing channel per machine.** The prototype opens one channel per request: each prompt
  holds one for up to 90 s, and each link's event follow holds one open. The channel protocol
  already numbers requests (`Msg::Request { id }`), but `peer.rs` always uses id 1. One channel per
  (home, machine), carrying every link's requests and follows, is M78's "a channel that stays".
- **A prompt that doesn't block a channel for a whole turn.** Prompt with a short timeout, then
  follow events. `prompt_agent` already returns `still_running`.
- **Routing to a teammate's personal machine.** Today the home account's daemon gets in only on
  team machines, or on machines that offer agents (M77's `agents` list). A link grant should add the
  home account to a `links` list the machine sends control, routed and listed like `agents`.
- **A refresh on a miss.** "No machine X" should call `refresh_now` and try again once: the rig
  waited two minutes for box-a to trust a machine that joined after it.

## q3: the lead's scope

The prototype keeps `Scope::Block(lead)` and adds a check:
- `readable` and `drivable` let a lead through to a pane that is one of its conversation's links on
  this machine (`convs::lead_reaches`);
- the tool dispatch rewrites a link name (`"pane": "L2"`) to the pane, or does the call on the
  linked machine (`convs::route_link`).

Everything else is the block scope as it was: its own tab, and what it started.

**Recommendation:** a scope of its own, `Scope::Lead { block, conv }`, so the check isn't a lookup
on every call. Its rules:
- **Links by name only.** A pane number from a lead is refused unless it's in the lead's own tab.
  The prototype's first run is the reason: it gave the lead "L2 (%3 on box-b)", the lead called
  `prompt_agent` with `%3`, and that was the lead's *own* block on box-a. It was refused, but only
  because the lead hadn't started itself.
- **Tools on links:**
  - `prompt_agent`, `wait` (idle or needs_input on a remote link), `read_output` (screen);
  - `read_thread` and `post_thread`, which default to its conversation;
  - `list` kind `links`.
  These six were on the lead's `allow` list and ran without asking.
- **Not on links:**
  - `agent_respond`: cards are the people's, which the brief told the lead and it followed;
  - `send_input` and `run`: raw keys and new shells in someone else's pane are a step past
    "prompt its agent". Add them later if a real need shows up.
- **Sub-agents:** `start_agent` from a lead opens beside it on the home machine, as a block's does
  now, and the daemon adds the new block as a link (`L3`) with no grant needed, since it's local.
  Starting one on another machine goes through that machine's `/api/blocks` under a grant for "new
  blocks in session S". That's a later step.
- **Nothing else:** no session or VM, no other tab, no thread but its own.

## q4: owner's rights, made safe

**The lead acts with the owner's rights (decided), so membership is a drive grant.** Members'
screens say so:
- **The invite:** "Members can ask the lead to work in the linked terminals: type there, run
  commands, and read what they show. Linked now: claude in calc on geek, claude in calc on
  jake-mini. Invite only people you'd hand those terminals to."
- **The members list:** each member's row says "can drive 2 linked panes through @lead".
- **Linking a pane** into a conversation that has members shows the same warning: "sam and riley
  will be able to drive this pane through @lead".

**Hiding a linked pane from some members doesn't work.** The brief's layered rule, a placeholder
for people the pane isn't shared with, protects only the raw screen.
- Any member can write "@lead what does L2 show?", and the lead, acting as the owner, will answer.
- The prototype's lead read L1's screen with `read_output` unprompted, to write its report.
- An LLM can't be relied on to withhold by asker. Each `@lead` arrives labeled with its author, but
  that is advice to the model, not a check.

**Recommended rule:** being a member means seeing every link: screens, quotes, the lead's summaries.
Say it at invite and at link time, as above. Keep the per-pane rule where it's real:
- a pane's own thread stays the pane's;
- a member can't open the pane itself (its terminal, its block) unless it's shared with them.

That's For Jake 1, because it drops a line of the tracker's shape.

**What stays enforced in code, not in the prompt:**
- links are added only by someone who may drive the pane, on that pane's machine (q2);
- the lead reaches links only;
- cards are answered by people, through `act`, recorded with who answered.

## q5: events into the timeline

**What exists, and what was posted:**

| signal | where it comes from | in the run |
|---|---|---|
| turn ended | Claude Code's `Stop` hook → `arugula attention done` → `Attention{Done}`; an agent block's own turn end | posted once per real turn |
| needs input (a card) | `PermissionRequest` → `arugula hook` → a permit card, `Attention{NeedsInput, reason.kind: ask}`; `AskUserQuestion` → `arugula ask` | 3 cards from L2, each answered in place |
| needs input (not a card) | the `Notification` hook ("Claude is waiting for your input", a minute after a turn); a bell | dropped |
| exited | `EventKind::Exit` | built, not seen (when a restart killed L1's claude, the pane came back as a shell) |
| PR checks done | a forge block's attention (`ReasonKind::Failed`/`Done`), when a PR is linked | not tried |

**How remote events arrive:** `GET /api/events?pane=N&type=attention,exit&follow=1` on the linked
machine, streamed through the channel (`head.stream`, the parts as NDJSON). `peer::follow` reads it.
- The watchers came back after each box-a restart (`convs::resume`).
- At home the watcher subscribes to the mux's broadcast instead.

**Rules, from the first run's noise:**
- **A turn ends only after the link worked.** Prompting a link marked it idle first, which posted a
  "turn ended" the moment it was prompted.
- **Only an ask is "needs input".** Without this, every link also posted "Claude is waiting for your
  input" a minute after each turn, with nothing to answer.
- **The same line twice in a row is one.**
- **Coalesce per link**, which the product should add: while a link is working, hold "needs input"
  lines that are answered within a second or two by a standing rule, and post "worked 40 s, 3
  approvals" with the turn's end.

**The turn's summary.** The `Stop` hook's attention says only "set by the API". The summary should
be the turn's last assistant message:
- an agent block already has it;
- for a terminal, the `Stop` hook's input carries the transcript path, from which `arugula attention
  done` can send the last message (to verify against Claude Code's hook payload).

The prototype's lead made up for it by reading the screen.

**Cards.** The prototype posts a card as a text line, and `POST /api/convs/{id}/act {link, action}`
answers it on the linked machine through `/api/attention/act`. In the product:
- the message carries the ask (`event: {kind, link, ask: {id, tool, input}}`), so the page renders
  Allow and Deny;
- the answer passes the ask's `id`. The prototype's didn't, so an answer could land on a newer
  prompt;
- "who answered" is recorded at home (the member) and on the linked machine (the grant, "via
  conversation …").

## q6: the lead's context

**Each turn** (`convs::tell_lead`):
- who wrote and what;
- the link events since the lead's last turn (up to 20);
- the links by name with their titles and machines;
- one line on how to answer.

The standing instructions (drive links by name, prompt several before waiting, don't answer cards,
reply with `post_thread`) go in the block's `brief`, which is appended to Claude Code's system
prompt.

**Woken by links.** A link the lead prompted and didn't see finish is remembered. When its turn
ends while the lead is idle, the lead gets "L2 (you prompted it) ended its turn…" as its next
prompt.
- The first run is why: the lead posted "L2 is waiting for permission", ended its turn, and never
  heard that L2 finished.
- In the final run the lead `wait`ed on L2 inside its turn, so the wake wasn't needed.
- The prototype's lead never posted on its own: replies were its `post_thread` calls. The product
  should also post the turn's last message when the lead ends a turn without posting.

**Restart.** The lead's block came back after each of two box-a restarts: "Started the agent
again", in the same ACP session (`session/resume` or `session/load`, M44/M50). Its next answer
said it had fixed `mean()` "again", so it remembered. Two things don't come back:
- **The brief.** On resume only imported or worn meta is re-sent, not `brief`. So the per-turn
  context must carry all the lead needs, and the brief is a convenience.
- **The prototype's in-memory state:** the events waiting for the lead's next turn and the links it
  waits on. The product keeps a cursor instead, the id of the last timeline message the lead has
  seen, in the record. "Since your last turn" is then the timeline after the cursor, and nothing is
  lost on restart.

**Long conversations.** Claude Code compacts its own context, and the agent block does nothing
about it. The record is the timeline, not the lead's memory: the per-turn context says "read_thread
has the whole conversation", and `read_thread after` pages it. A lead that's lost its way can be
replaced. A new block gets the record's summary of links and the last N messages, and the
conversation keeps its id and timeline.

## q7: the Chat page

From `web/src/ui/chat.tsx` and `threads.tsx`:
- **Channels**, the slot M73 left, goes after Activity and Huddles and before the machine sections
  (`chat.tsx:231-243`). It lists every conversation the person may read, from each machine's
  `threads` summaries, which already carry them: `threads_for` includes `conv` targets. A row shows
  its title, its home machine, the unread count, and a dot when a card waits.
- **Creating one:** *New conversation* in the Channels header asks for a title. Home is the machine
  you're on, or a choice among your online machines. The creator is its owner.
- **Linking:** *Add to conversation…* goes in `threadItems()` (`threads.tsx:42-59`), which the pane
  menu spreads (`commands.tsx:139`). It lists conversations and *New…*, and makes the grant on the
  pane's machine, then the link at home (q2).
- **The header:** the title, then a chip per link (`L1 claude · geek`, with its state: working,
  waiting on a card, done, closed, offline), then the lead (`@lead`, with its state). A chip opens
  the pane, or the pane's own thread for someone who can't open it.
- **Cards** are inline, with Allow and Deny (q5).
- **The members list** carries q4's sentence.
- **What has to change:** the web client assumes two kinds of thread in 10 places (`threadKey`,
  `inviteToThread`, `mayPost`, `goTo`, `topicOf`, the find view, `ThreadBody`'s title and "gone").
  This branch makes them compile with a third kind and does nothing else.

## q8: phone and push

- **Mentions.** `notify_mentions` opens a pane, and returns silently when there isn't one
  (`thread_ops.rs`). The prototype uses the lead's block for a conversation. The product sends
  `thread: conv-c-…` with no pane.
- **The phone's tap.** It only knows `pane-N` and `session-N` (`tap.ts:7-9`, `tapUrl`), and opens
  the drawer (`main.tsx:351-386`). A conversation needs `#chat=conv-c-…&host=HOME` to open the Chat
  page on that conversation.
- **The lead's answers.** They're agent posts and mention no one, so today they don't notify. A
  reply to an `@lead` should notify whoever asked, and their phone tap opens the conversation.
- **Cards** in a conversation notify every member, as attention does for a pane's people. They're
  sent from the home machine, since members may not have the linked one.
- **Where push comes from.** Pushes go out from the home machine's `config.notify` (Web Push and
  control push), so a member on the phone gets them without reaching the linked machines.

## For Jake

1. **What membership shows.** With the lead acting as the owner, any member can ask it to read any
   linked pane, so the tracker's "a linked pane's screen and quotes are shown only to people that
   pane is shared with" can't hold (q4). Choose one:
   - **a) Members see everything linked** (recommended). Drop the placeholder rule. Say so at invite
     and at link time, and keep the pane itself (its terminal, its own thread) to the people it's
     shared with.
   - **b) Keep placeholders for screens and quotes anyway,** as a courtesy, and say the lead can
     still describe them. It looks safer than it is.
   - **c) Two kinds of member:** *drivers*, who may `@lead`, and *readers*, who see only the
     timeline's messages. Events and the lead's replies are still content, so readers still learn
     what links do; it only stops them asking.

## The milestones (from M81, for #851)

Each builds on the previous one. The prototype's pieces map onto them. Nothing from this branch
merges as is.

- **M81: conversations as threads.**
  - `ConvId` and `ThreadTarget::Conv`.
  - The record and timeline in `<state>/threads/`.
  - Members with a `joined` floor.
  - Operations (with policies) to create, rename and delete a conversation and to add or remove
    members, and the invite with q4's sentence.
  - The Chat page's *Channels*, *New conversation*, the conversation view and its members list.
  - Unread, search and mention pushes, with the `conv` tap URL (q8).
  - Pane and session threads unchanged.
- **M82: links, here and on other machines.**
  - Link grants on the pane's machine, made by someone who may drive it, listed on the pane and
    revocable.
  - `e2e.rs` lets the home daemon through for a granted pane's prompt, wait, capture, events and act,
    with the grant maker's rights.
  - A `links` list to control, so a teammate's personal machine is routable.
  - One standing channel per (home, machine), multiplexed by request id.
  - Links store the machine id and pane epoch, with closed and offline states.
  - *Add to conversation…* on the pane menu, and link chips in the header.
  - A `refresh_now` on an unknown machine.
  - A DECISIONS entry: "A conversation's home drives a linked pane through the relay, by a grant on
    that pane".
- **M83: events and cards in the timeline.**
  - A watcher per link, from the mux's bus or a followed `/api/events` stream.
  - q5's rules: a turn ends after work, only asks are cards, repeats collapse, and lines are
    coalesced per turn.
  - Typed event messages (`event: {kind, link, ask}`), rendered as cards with Allow and Deny that
    answer by ask id through the linked machine's `act`.
  - The turn's summary from its last message.
  - Pushes for cards to members.
  - PR links: a forge block's checks finishing.
- **M84: the lead.**
  - A lead block per conversation on the home machine, started with the conversation, its model the
    user's default.
  - `Scope::Lead`, with links by name only and q3's tools.
  - `@lead` as its next prompt, with q6's context.
  - A stored cursor for "since your last turn".
  - Woken by links it waits on.
  - Its replies as messages (`post_thread`, or its last message when it doesn't post).
  - The lead's reply notifies whoever asked.
  - `start_agent` from the lead adds the new block as a link.
  - It survives a restart without losing a turn's events.
- **M85: the tracker's run, live.**
  - geek and jake-mini from the phone, in the order of #851's done-when: a conversation on geek
    linking a pane on each, "@lead have both agents run their tests and fix what fails", the
    turn-ended and needs-input events, a permission card answered from the phone, and the lead's
    report.
  - Pane threads checked unchanged.
  - Anything the run finds.

## Reproduce

```
export CARGO_TARGET_DIR=$PWD/target
cargo build -p arugulad -p arugula -p arugula-control
(cd web && pnpm install)
env -u ARUGULA_SOCK -u ARUGULA_PANE TARGET_DIR=$PWD/target/debug \
  node --experimental-strip-types --no-warnings spikes/s36-conversations/rig.ts /tmp/s36-rig &
spikes/s36-conversations/setup.sh /tmp/s36-rig   # panes, conversation, grant, links, lead
spikes/s36-conversations/flow.sh /tmp/s36-rig    # @lead, cards answered, the lead's report
spikes/s36-conversations/c.sh /tmp/s36-rig box-a GET /api/threads/conv-$(cat /tmp/s36-rig/conv)
```

- The panes run `claude` under the machine's own login, with the user's Claude Code hooks (`arugula
  hooks install`), which reach the rig's daemon through the pane's `ARUGULA_SOCK`.
- `restart.sh STATE box-a` restarts a machine's daemon by its exact PID, with the freshly built
  binary.
- Stop the rig with SIGTERM to the `rig.ts` process (it takes its daemons and control with it).
  Then stop the panes' `arugulad _shim` processes and their `claude` by PID: they outlive a daemon
  on purpose.
