---
schema: 1
id: "aru-115"
title: "A thread's agent session runs on the project's hosted box"
state: "decided"
area: "Agents and blocks"
source:
  via: "cli"
  issue: "arugula-salad/arugula#1438"
question: "What does @arugula in a thread do, where does the agent it starts run, as whom, and what may it push?"
options:
  - id: "a"
    label: "@arugula starts a session on the project's hosted box, as the asker on the project's key; it pushes only its own arugula/ branch, and the PR stays a draft the asker sends"
    how: "A mention of @arugula in a project's thread makes an agent session (ses_), a platform object: an ACP agent block on the hosted box the project's agents run on, in a worktree of its own, as its own Unix user on the box. It runs as the asker, on the project's key, under the project's standing grant's runs.instruct. Each reply in the thread is a prompt that starts its next turn. The box reports each state change, question, error and turn's result to the platform, signed. It pushes arugula/<session>-<slug> with an installation token minted for that one push, and opens nothing: the PR is a draft its asker sends. The session's status card is the one card a chat timeline shows."
    tradeoff: "Only projects with a hosted box get agents in their threads, and every session costs box time and the project's model use."
  - id: "b"
    label: "@ wakes an agent on any machine, personal ones included"
    how: "A mention of a machine's agents, or of @arugula, starts a turn on whichever machine the mention names, the mentioner's or a teammate's."
    tradeoff: "Anyone in a thread could make a teammate's computer run an agent, with that teammate's credentials and files, while they're away."
  - id: "c"
    label: "Keep aru-091 as it was: mentions are read, nothing dispatches"
    how: "An agent reads its mentions over MCP when its person runs it, and people start and link agents themselves."
    tradeoff: "A thread can't ask for work; someone has to be at a machine to start each turn and carry the answer back."
  - id: "d"
    label: "The box pushes and opens the PR itself"
    how: "The session's box gets a token that may push any branch and open or update the PR, and the platform shows what it did."
    tradeoff: "A machine would act on GitHub as the project with nobody sending it, against \"agents draft, people send\", and a token that pushes any ref can push to main."
choice:
  option: "a"
  reason: "Thread = session (Jake, 2026-10-25, Arugula #129): a thread asks for work and watches it happen, on the project's own box, never a person's machine, as the person who asked, and nothing reaches GitHub's main line without a person sending it. The box, the grant, untrusted text, three sessions per box and @arugula before @<recipe> are the lead's calls in #129. Amended (Jake, 2026-10-26): the push token writes workflows too, since GitHub refuses a branch that began before main changed CI without it; reviewers check a session's workflow changes. Amended again (Jake, 2026-10-26, Arugula #214): with several workspaces and none chosen, the session picks a box with the repo (awake with a free slot, then the least busy) instead of asking, and Move to… on its card sends it to another; what still needs the asker is a form that mentions them. Amended again (Jake, 2026-10-27, Arugula #215): a box runs three sessions and holds six (or as many session users as it says it has, three when it doesn't say); a done one idle 30 minutes is parked (its agent closed, its user, checkout and conversation kept) and opens again on a reply when there's a running slot. Amended again (Jake, 2026-10-27, Arugula #216): a session's agent reads and comments on its own project's native issues, with its asker's rights and as the session itself, through a server of its own and a route that takes the session as the authority, not machine grants. Amended again (Jake, 2026-10-27, the session Docs ask): it also opens issues in that project, as the session's own, telling nobody, and makes, writes (its own) and reads the project's Docs (aru-118). Amended again (Jake, 2026-10-27, Arugula #265, aru-121): a person's agent on a machine, by its owner's sessions.start grant, starts a session as them (they stay the asker and the payer; the card says it was their agent), and by sessions.reply answers a session's question or prompts it; a message an agent posts without those grants still starts and prompts nothing. Amended again (Jake, 2026-10-28, Fleet 6): a repo linked to the project that no box's spec has is one a session may work in: the ask may name it and the which-repo form offers it, and the session's box clones it into the session's home at its start, shallow, with an installation token minted for that clone (contents read, the session's repo alone, revoked after), without adding it to the box's spec."
rejected:
  - option: "b"
    why: "Agents on personal machines are never woken by other people's mentions: a person's machine runs what its person starts."
  - option: "c"
    why: "Superseded for hosted boxes by Jake's thread = session. On personal machines it still holds."
  - option: "d"
    why: "Machines never send, merge or approve (\"Forge blocks use the person's own CLI, and agents draft while people send\"); a session's token pushes its own branch only."
supersedes: []
evidence:
  - title: "#1438"
    url: "https://github.com/arugula-salad/arugula/issues/1438"
  - title: "Arugula #129"
    url: "https://app.arugula.io/issues/iss_c2af265d6402c7a2f22e8d24154fb215"
  - title: "Arugula #130"
    url: "https://app.arugula.io/issues/iss_e2a536bc47833538c4a29793052acbc1"
  - title: "Arugula #131"
    url: "https://app.arugula.io/issues/iss_9eac7586846748abea4bf7264dfd9771"
  - title: "Fleet 6: dispatch into a linked repo no box clones yet"
    url: "https://app.arugula.io/issues/iss_2c4191592f7e2be3e77c5db0d4c6f5e3"
decided_by: "jhgaylor"
decided_on: "2026-10-25"
reviews: []
constrains:
  - "aru-091"
  - "aru-041"
  - "path:crates/platform/src/db/sessions.rs"
  - "path:crates/platform/migrations"
  - "path:crates/e2e/src/session.rs"
  - "path:crates/control-wire/src/work.rs"
  - "path:crates/platform/src/chat/objects.rs"
  - "path:crates/arugulad-work/src/sessions_push.rs"
  - "path:crates/platform/src/sessions/push.rs"
  - "path:crates/platform/src/sessions/pr.rs"
  - "path:crates/platform/src/sessions/tries.rs"
  - "path:crates/platform/src/sessions/review.rs"
  - "path:crates/platform/src/sessions/issues.rs"
  - "path:crates/daemon/src/mcp/session.rs"
  - "path:crates/platform/src/sessions/pool.rs"
---

# A thread's agent session runs on the project's hosted box

A thread can ask for work (Jake, 2026-10-25, thread = session, Arugula #129).
Writing `@arugula` in a project's thread starts an **agent session**: a
platform object (`ses_`), like a run, that lives across many turns. Each
reply in the thread is a prompt that starts its next turn. This amends
"Agents are in conversations" and "Forge blocks use the person's own CLI, and
agents draft while people send".

- **It dispatches, on hosted project boxes only.** `@arugula` is the one
  mention that starts work, and only on the project's hosted box ("A hosted
  machine is arugulad in a rented sandbox"). Agents on personal machines
  are never woken by other people's mentions: on them, mentions are still
  read, not pushed. `@arugula` comes first; `@<recipe>` (a project recipe's
  agent) comes later.
- **Try 3 and `@codex review`** (M264). From a session's card, an owner or
  editor can run it two more ways at once: two siblings of it re-run its
  first prompt on the same box, each in a slot of its own, on Claude Code
  or Codex, and they pick one to keep; the others stop. `@codex` is
  reserved as `@arugula` is, with the same rule for who may ask: `@codex
  review` in a session's thread starts a read-only session given its pushed
  diff and the thread as data, which pushes nothing and posts its review
  in the thread. Both are sessions on the project's hosted box, as the
  asker, like any other.
- **One card in the timeline.** The session's status card is the one card
  kind a chat timeline shows; everything else in it stays a message.
- **It runs as the asker, on the project's key.** The session is the
  asker's: it acts for them, under their access. Its model use is on the
  project's key ("Hosted inference: strangers bring their own key"), and its
  box time is the project's workspace's, metered as hosted usage already is.
  Nobody else's credentials or subscription are used.
- **The grant is the project's standing `runs.instruct`** ("One standing
  grant per project starts runs"): no new grant kind.
- **Which box:** the project's setting "agents run on `<hw_>`". With none
  chosen it's the project's only workspace; with several, the session
  picks one whose spec has the repo, awake with a free slot first, then
  the least busy, then by name, and its card's Move to… sends it to
  another (amended, Arugula #214: it no longer asks which box). It asks
  only which repo, when the boxes have several and the ask names none,
  as a form that mentions the asker until it's answered. A repo linked to
  the project that no box's spec has is a repo it may work in too
  (amended, Jake, 2026-10-28, Fleet 6): the ask names it by
  `owner/name`, or the form offers it beside the boxes' repos, and the
  session goes to the best of the boxes, whose daemon clones it into
  the session's home at its start, shallow, from GitHub. The box's spec
  stays as it was: the clone is the session's, and goes with its
  checkout. "Least busy"
  is the fewest live sessions (queued, or a turn under way), then the
  fewest done ones (Arugula #215): a done session still holds its slot
  and its checkout, but it isn't working the box now, so asks in a row
  spread over the awake boxes. A paused box is woken only when no awake
  one has a free slot, and a box holding six is full (below).
- **On the box:** an ACP agent block, as its own Unix user, in a worktree of
  its own. How the user and the worktree are set up is the box's to settle
  (S42).
- **Three running, six held** (amended, Jake, 2026-10-27, Arugula #215).
  A box runs at most three sessions at once: a turn under way, or done
  less than 30 minutes ago, its agent still up for the next turn. Each
  running agent takes the box's memory and CPU, and three is what a box is
  sized for. It holds at most six: each keeps its own session user
  (`ses1` to `ses6`), and that user's checkout and `~/.claude`, for its
  next turn. A box says how many session users it has on each pull, and
  holds no more than that. One that doesn't say has three: the template
  from before this, on boxes their owners haven't rebuilt, which hold
  three as they did.
  - A done session idle 30 minutes is **parked**: its box archives its
    transcript, then closes its agent block, keeping its user, checkout
    and conversation. That frees the memory and a running slot, so a
    fourth session can run while three wait for replies. Its card says
    it's parked.
  - A reply to a parked session waits, queued, until the box has a
    running slot. Then the box opens a block on the same user and
    checkout, loading the conversation, and the turn runs there.
  - Before this, a done session held its slot until it ended, so a box
    with three done sessions took no new one, and one box held every
    recent checkout while the others slept.
  - The day-long idle stop still ends a session for good, and its user
    and checkout go then.
- **Untrusted text.** A project member's text goes into a prompt plain.
  Text from GitHub (a review, a comment, a check's log) goes in fenced as
  quoted data, saying whose it is and its `author_association`, so the
  agent weighs it and doesn't follow it.
- **The push path.** The box pushes `arugula/<session>-<slug>`, the
  session's one branch, with a short-lived installation token minted for
  each push and never written to disk. Any other ref is refused. Machines
  never send, merge or approve: the PR is still a draft the **asker**
  sends, with their one-use user token ("Sending as a person uses a one-use
  user token, falling back to their machine").
  - Tokens can't be confined to a ref, so the box confines the push (M259,
    S42 q2): the session's user writes a bundle of its one branch, and the
    daemon fetches it into a repo of its own and pushes one refspec from
    there, with no hooks and none of the agent's config. `main` and other
    sessions' branches are refused before git runs.
  - The platform mints the token only for the session's own box, for the
    session's branch exactly, on its linked repo, with `contents: write`
    and `workflows: write` (amended below). The box revokes it once it
    pushed.
  - A session whose repo no box's setup cloned (amended, Fleet 6) gets a
    token for its clone the same way, after the same checks, with
    `contents: read` alone, revoked once it's cloned. Its daemon clones
    with its own git (no hooks, no config of the agent's), and the token
    reaches git as the push's does, never in the checkout's config.
  - **Amended (Jake, 2026-10-26): the push token writes workflows too.**
    It was `contents: write` alone, so that GitHub would refuse a push that
    changes `.github/workflows/`. But GitHub compares a new branch's tree
    with the default branch's, not with the commit it began from: once
    main changes a workflow file after a session began, the session's
    branch, which has the older file, reads as an edit, and GitHub refuses
    every push of it ("refusing to allow … to create or update workflow
    … without workflows scope"). Fetching main first doesn't help. The
    v0.63.0 live test hit this on a real repo, and any session that falls
    behind a main that changed CI would. So the token now has `workflows:
    write`, still for the one repo, per push, revoked after, and still
    only for the session's own `arugula/*` branch. What it means: a
    session's agent can change CI files on its branch, and a person
    reviewing its PR must check workflow changes before merging. The PR's
    draft says when the branch changes anything under
    `.github/workflows/`.
  - Even so, an `arugula/*` push runs the repo's existing push workflows on
    code the agent wrote, as a member's pushed branch would. Since the
    amendment that includes a workflow the agent itself changed on its
    branch: an `on: push` workflow runs from the pushed branch's own file,
    with the repository's secrets, before anyone reviews the PR.
  - What this doesn't stop is an agent that gets from its session user to
    the daemon's user, which could use a token in the seconds it lives. A
    git proxy in control that holds the token and reads the ref list would
    close that, later.
  - The PR's title and body come from the repo's PR template and the
    session's turns, and its asker edits them before sending. Once sent,
    the session links the PR, and the PR's thread says which session it
    came from.
- **Its project's native issues and Docs** (amended 2026-10-27, and twice
  more the same day). The session's agent can read its project's native
  issues, comment on them and open new ones, and make, write and read its
  project's Docs ("A project has Docs of its own, on the platform",
  aru-118), and nothing else of Arugula's: no edits, new labels, assigning
  or closing, no deleting a Doc or writing one it didn't make, no other
  project, none of `/mcp`'s tools (which start and drive panes as the
  daemon's user).
  - **Whose rights.** The asker's. Control's `POST
    /api/hosted/work/session/issues`, signed as the box, takes the session
    id and an issue's number: it checks that the session is this machine's
    and hasn't ended, takes the project from the session (a call names none,
    so there is no other project to name), and reads or comments as the
    asker, so their role in the project is the ceiling (a viewer comments;
    a review session comments on nothing). A comment is the session's
    own, as its answers in its thread are: no author, `via` `said:<ses>`,
    with no mentions, so nobody is pushed for it and it isn't counted as
    the asker's words. An issue it opens is the session's own the same
    way: no author, the session's id where an agent's machine would be (the
    page says `Agent session <id>`), its body's mentions text and nobody
    assigned, so nobody is pushed; it needs the asker's right to open issues
    (a viewer's session opens none, a review session none), and its labels
    are ones the project's issues already have. A Doc it makes is its own
    too, with no maker and the session as its agent; it needs the asker's
    right to make Docs, it writes only the Docs it made (a person who may
    write may write those), it reads any of the project's, and a review
    session only reads. Each answer has the Doc's page link, for the agent
    to post. This is not aru-091's
    machine grants: a hosted box has no owner account to grant for, and the
    session is what authorises it, as it is for the push token.
  - **How the agent reaches it.** The daemon serves six tools,
    `read_session_issues`, `comment_session_issue`,
    `create_session_issue`, `create_session_doc`, `fill_session_doc` and
    `read_project_doc`, on a loopback
    listener of its own (`mcp/session.rs`), and gives the session's agent
    that as its `arugula` MCP server over HTTP. The agent's port to the
    daemon stays closed, and it gets nothing of `/mcp`. The listener knows
    who calls by the uid that owns the client end of the connection
    (`/proc/net/tcp`) and the session that uid runs, not by a token: a token
    would be on the command line of `sudo env -i …`, readable by the box's
    other session users, who are other sessions asked by other people.
  - **What it doesn't stop.** A prompt-injected agent can comment on and
    open issues, and make and write its own Docs, in the one project, with
    its asker's rights, as the
    session's thread replies already can post; the box's hourly limit caps
    how many. An agent that gets from its session user to the
    daemon's reaches the signing key, as it could before.
- **Signed and append-only.** The box signs everything it reports of a
  session (`arugula session event v1`), numbered so a report can't be
  replayed, and the platform keeps each state change, error and turn's
  result as an append-only event, as run events are ("A run is a platform
  object on a hosted workspace"). The platform marks a session whose box
  goes quiet for 60 s mid-turn `unresponsive`, until it's heard from again.

- **A person's agent, by grant** (amended 2026-10-27, Arugula #265,
  "A person's agent starts and answers sessions as them, by grant",
  aru-121). A message an agent posted never started or prompted a session.
  Now, with its owner's `sessions.start` grant over the project, a
  machine's agent's `@arugula` starts one as its owner, who is the asker
  and pays, and the card says "started by <owner>'s agent on <machine>";
  with `sessions.reply`, its reply in the session's thread answers the
  question it waits on, or else prompts it. Without them, nothing changes.
  It never allows or denies a permission.

What doesn't change: agents on people's machines read and post through
their machine's grants ("Agents are in conversations"), an agent's
"approve" is never an approval, and an agent's GitHub write from a person's
machine is still a draft its person sends.
Where: a session's issues (`crates/platform/src/sessions/issues.rs`, `crates/daemon/src/mcp/session.rs`, `WORK_SESSION_ISSUES` in `crates/control-wire/src/work.rs`); the sessions tables and their rules (`crates/platform/src/db/sessions.rs`, migration `0050_agent_sessions`), the states and the signed event (`crates/e2e/src/session.rs`), what the box pulls and reports (`crates/control-wire/src/work.rs`), sessions as objects (`crates/platform/src/chat/objects.rs`); dispatch (M256), the card (M257), and the push and the PR (M259: `crates/arugulad-work/src/sessions_push.rs`, `crates/platform/src/sessions/push.rs` and `pr.rs`, migration `0053_pr_drafts`).
From: thread = session (Arugula #129, native, behind #1438), M254 (Arugula #131), S42 (Arugula #130).
