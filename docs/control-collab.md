# Working together through control: chat, agents and tasks

Status: **draft for review** (alecraso and Jake), 2026-10-15. Nothing here is
built. It replaces how chat, threads, conversations, the agent catalog and A2A
delegation work today (Labs: `chat`, `agents`), after the #415 live check
failed.

## Why

#415 put Jake's agent and alecraso's agent on real machines, on hosted control,
and had them delegate work to each other ([the write-up](../spikes/s34-agents-a2a/README.md#the-live-run-on-hosted-control-415)).
The wire held: tasks crossed, patches came back, cancels and revokes worked.
**The test failed anyway**, because neither agent could do anything without a
person:

- **Consent:** every task from another person waited on a card until its
  receiver picked up their phone. The first task waited 11 minutes, and then
  ran for 72 seconds.
- **Permissions:** every tool call inside a delegated task (run the tests, edit
  `calc.py`) waited for the receiving person to approve it on the block.
- **Coordination:** the agents had no way to reach each other or the other
  person. alecraso and Jake relayed between them by pasting text into an
  Arugula conversation, and by commenting on the issue.
- **Joining:** making the personal machine took three join codes, because each
  one expires in 15 minutes and nobody was watching.
- **Identity:** Jake's chat messages showed as alecraso's (#935). A machine
  outside Jake's team named him `c86f85e963d6740f` (#936).
- **Results:** a blocking `send` never returned (#937), a question lost its
  options on the way back (#938), and apply reported the wrong outcome (#939).

Most of these share one root: chat and A2A live **inside each machine's
multiplexer**, session by session.
- A thread belongs to a pane or a session.
- A conversation lives on one machine, and is reachable only while that
  machine is online.
- The catalog is each machine polling the others through the relay.
- Who someone is depends on the roster that machine last checked.
- A task exists only on the machine that runs it.

None of these things is per session. People and agents work across machines,
so the place that knows all of them is control.

## Goals

1. **Agents work together unattended.** After the people involved set things up
   once, a task can go from one person's agent to another's, run, ask its
   caller a question, and come back with results, with no person acting. A
   person is pulled in only for what policy says needs one.
2. **One place for people, agents and conversations,** reachable whether or not
   any one machine is online.
3. **Names are right everywhere.** A person or agent is named the same on every
   machine, phone and page, because the name comes from one authority.
4. **Execution stays on machines.** Code, credentials, worktrees and terminals
   never move to control.

**The bar:** rerun #415's scenario with **zero** human actions between "both
people set up their offers and policies" and "results are applied". Measure the
end-to-end time and count the human touches.

## Non-goals

- Moving terminals, panes or their output into control. The multiplexer stays
  the execution engine.
- Public A2A for agents outside Arugula. That keeps the later milestone S34
  named; the design below shouldn't block it.
- Huddles. Their audio already goes device to device. Only who may join
  changes, once membership is control's.

## The decisions to make first

Each needs alecraso and Jake to agree. My recommendation comes first in each.

### D1. Control stores chat. Can it read it?

Today's promise is "control can refuse service, but it can't read"
([control-e2e.md](control-e2e.md)). Threads keep that promise only because
they stay on machines. Centralising them means choosing:

- **(a) End-to-end encrypted in control (recommended).**
  - Each conversation has a key, wrapped to every member device's existing
    Noise key. Control stores ciphertext, routes it and fans it out.
  - Removing a member rotates the key. Adding one with history re-wraps the
    old keys for them.
  - This is the sender-key model (Signal groups, Matrix's Megolm). It reuses
    the device certificates and daemon-verified chains we already have.
  - **Cost:**
    - Search runs on each client, not in control.
    - A new device sees history only once another member's device re-wraps
      it for them.
    - Agents need device keys of their own, which they get from the machine
      they run on.
  - MLS (RFC 9420, `openmls`) is the stronger version, and can replace this
    later if groups grow.
- **(b) Control reads chat and tasks.** This is simpler: server-side search,
  history, summaries, and agents acting from control itself. But the promise
  ends for everything except terminals, and control-e2e.md's "What holds
  against control" table has to say so.

Task content (prompts, replies, patches) follows the same choice as chat.

### D2. Policy replaces cards

A task runs when a **standing agreement** covers it, not when a person taps
Allow. The owner of an offered agent sets, once:

- **Who may call it:** their team, named accounts, or their own agents only.
- **What it may do inside a task:**
  - a permission profile applied to the task's sandbox, a fresh worktree;
  - built from today's standing rules (`rules.rs`), such as "Bash in the
    worktree, Edit in the worktree, no network except the forge";
  - the recipe's own `tools` and `permissionMode` stay the upper bound.
- **A budget:** turns, model spend and wall time, per caller and per day.
- **How results leave:** a branch or PR (the default), or a patch.

A person is asked only when something falls outside the agreement: an
unknown caller, a budget exhausted, or a tool outside the profile. Then the
question goes to the agent's owner, and if nobody answers within a set time,
to the team.

Today's per-task consent card and per-tool approvals become the exception
path.

### D3. Agents are first-class participants

- **Identity:** an agent gets an identity from control: an agent principal,
  owned by a person, acting for them, and shown as "fixer (alecraso's)".
- **Inbox:**
  - it can be @mentioned, added to conversations, and sent tasks;
  - control delivers to the machine hosting it, which wakes the agent (the
    inbox and woken turns of #700);
  - there's no polling, and the caller's session doesn't have to stay alive.
- **People's agents:** a person's own Claude Code sessions (terminal, agent
  block or desktop app) can join under that person's agent identity. That's
  how "Jake's agent" talks to "alecraso's agent" directly, with no one
  pasting.

### D4. A task is a conversation

- **State in control:** a delegated task is a conversation with a state
  machine (submitted, working, input-required, done, failed, cancelled), held
  in control. So:
  - the caller can go offline, and the receiver's machine can restart, #937's
    lost send among them;
  - the task resumes when the receiver's machine is back;
  - its questions, answers and status are messages, with structured parts:
    a question keeps its options (#938).
- **Results:**
  - **Code: a branch or PR by default,** pushed with the receiver's forge
    credentials as today's `--pr` does. Applying is then the caller's git
    work, not a patch to replay (#939).
  - Small artifacts, such as replies, files and logs, are stored in the
    conversation, encrypted under D1.
- **A2A** stays the wire format where it helps: task and part shapes, and
  external interop later. But control is the hub, not each machine.

### D5. What machines keep

Execution only:
- running a task's agent in a sandboxed worktree, under the agreed profile;
- holding the credentials it pushes with;
- reporting the task's progress back to control.

Pane threads become conversations linked to a pane, as #856's links do now,
with the pane's events posted into them. Control holds the conversation; the
machine holds the pane.

### D6. One authority for names

- **Names:** a person's name comes from their account in control, signed
  where it matters. It no longer comes from whichever roster a machine last
  saw. That fixes the #935/#936 class. On machines, the multiplexer's owner
  collapse (#935's remaining sites: history, card answers, conversation
  ownership) still needs fixing.
- **Agent names** follow D3.

## What we keep from today

- **The recipe format.** It's the Claude Code subagent file, unchanged (S34
  q1), and so is offering it from its project.
- **Sandboxed task worktrees,** and `--pr` delivery.
- **Wearing a recipe in an agent block** (`Def.recipe`, skills and MCP
  servers by reference).
- **Device keys, certificates and the relay** for reaching machines.
- **Links between panes and conversations,** and the conversation's lead
  agent (#856–#858). Their logic moves to control's side of the link.
- **The lessons:**
  - an agent asks its caller through AskUserQuestion, never in prose (#940);
  - names come from one place;
  - a result read once must not vanish before review (#937).

## What goes

Once each replacement ships, these are removed (Labs `chat` and `agents`):
- **Machine-hosted threads and conversations:** `mux:labs/threads.rs`,
  `mux/thread_ops.rs`, and the conversation homes in `labs/events.rs`, about
  2,100 lines.
- **The per-machine catalog and A2A server:** `labs/agents/` and `peer.rs`,
  about 4,200 lines.
- **The consent cards and the Team agents block,** as primary paths.
- **The web UI's thread and chat pages** (`threads.tsx`, `chat.tsx`,
  `chat-find.tsx`, `agents-page.tsx`, about 2,700 lines). They're rebuilt
  against control's API.

No fixes go into the old code from now on, unless something is broken badly
enough to block people.

## Phases (each becomes tickets)

1. **Decide D1–D6.** This doc, reviewed by alecraso and Jake.
2. **Identity and conversations in control.**
   - agent principals;
   - conversations stored and fanned out by control, encrypted per D1;
   - delivery to machines and devices;
   - the web client's chat against control.

   Done when a person and an agent on another machine talk in a
   conversation while the conversation's creator is offline.
3. **Tasks under policy.**
   - agreements (D2), and tasks as conversations (D4);
   - a sandboxed run under the profile;
   - branch or PR results, and escalation to the owner.

   Done when #415's scenario reruns with zero human actions.
4. **Pane links and leads,** moved onto control's conversations.
5. **Remove the old Labs code** and its flags.

## Open questions

- **Spend:** who pays for a delegated task's model use? Today it's the
  receiving machine's Claude login. Budgets (D2) cap it, but should a caller's
  account be charged?
- **Agent discovery:** a searchable catalog in control. Should an offer
  describe its inputs and outputs (an A2A card's skills) so agents can choose
  without a person?
- **Self-hosted control (#662):** D1(a) keeps a self-hosted control as blind
  as the hosted one. D1(b) means trusting whoever runs it with every
  conversation.
- **Fountain:** Jake's agent platform already runs agents centrally. Should
  control's agent principals and Fountain's agents be the same thing, or
  should they meet only at the A2A wire?
- **Joining:** should a machine join without a 15-minute code, e.g. by
  approving it from the machine's own page while signed in? That's separate
  from this rewrite, but #415 tripped on it.
