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
  reason: "Thread = session (Jake, 2026-10-25, Arugula #129): a thread asks for work and watches it happen, on the project's own box, never a person's machine, as the person who asked, and nothing reaches GitHub's main line without a person sending it. The box, the grant, untrusted text, three sessions per box and @arugula before @<recipe> are the lead's calls in #129. Amended (Jake, 2026-10-26): the push token writes workflows too, since GitHub refuses a branch that began before main changed CI without it; reviewers check a session's workflow changes. Amended again (Jake, 2026-10-26, Arugula #214): with several workspaces and none chosen, the session picks a box with the repo (awake with a free slot, then the least busy) instead of asking, and Move to… on its card sends it to another; what still needs the asker is a form that mentions them."
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
  - "path:crates/daemon/src/work_sessions_push.rs"
  - "path:crates/platform/src/sessions/push.rs"
  - "path:crates/platform/src/sessions/pr.rs"
  - "path:crates/platform/src/sessions/tries.rs"
  - "path:crates/platform/src/sessions/review.rs"
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
  as a form that mentions the asker until it's answered.
- **On the box:** an ACP agent block, as its own Unix user, in a worktree of
  its own, at most three sessions with a turn under way per box. How the
  user and the worktree are set up is the box's to settle (S42).
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
- **Signed and append-only.** The box signs everything it reports of a
  session (`arugula session event v1`), numbered so a report can't be
  replayed, and the platform keeps each state change, error and turn's
  result as an append-only event, as run events are ("A run is a platform
  object on a hosted workspace"). The platform marks a session whose box
  goes quiet for 60 s mid-turn `unresponsive`, until it's heard from again.

What doesn't change: agents on people's machines read and post through
their machine's grants ("Agents are in conversations"), an agent's
"approve" is never an approval, and an agent's GitHub write from a person's
machine is still a draft its person sends.
Where: the sessions tables and their rules (`crates/platform/src/db/sessions.rs`, migration `0050_agent_sessions`), the states and the signed event (`crates/e2e/src/session.rs`), what the box pulls and reports (`crates/control-wire/src/work.rs`), sessions as objects (`crates/platform/src/chat/objects.rs`); dispatch (M256), the card (M257), and the push and the PR (M259: `crates/daemon/src/work_sessions_push.rs`, `crates/platform/src/sessions/push.rs` and `pr.rs`, migration `0053_pr_drafts`).
From: thread = session (Arugula #129, native, behind #1438), M254 (Arugula #131), S42 (Arugula #130).
