---
schema: 1
id: "aru-130"
title: "An issue opened on GitHub stays native, and only new comments cross"
state: "decided"
area: "Federation and control"
source:
  via: "cli"
  issue: "arugula-salad/arugula#1276"
question: "Once a person opens a native issue on GitHub (M205), what becomes of the native issue, and which comments cross between the two, which way, as whom?"
options:
  - id: "a"
    label: "The native issue stays, linked; its thread's new top-level comments and the GitHub issue's new comments cross, each once"
    how: "The export is a draft (drafts.native_issue) a person sends through aru-085's path; sent, it sets the issue's GitHub origin and writes exported. The native issue keeps its number, state, timeline and thread, and its page says where it lives. A person's new top-level comment is a draft sent as them at once through one of their machines (waiting in their drafts when none can); an agent's is a draft its person sends. A GitHub comment on a public repo (webhook, or the poller's turn when a webhook was missed) lands in the thread as GitHub's: no author, via github:<login>; a private repo's stay on GitHub (aru-086). issue_github_comments keeps each GitHub comment that crossed, either way, so nothing comes back; a comment GitHub echoes before the send answers is matched to its draft by its text."
    tradeoff: "Two copies whose titles, bodies, states, labels and assignees can drift; edits, deletions and sub-thread replies don't cross; a private repo's comments don't come back, so its pair's thread is one-way."
  - id: "b"
    label: "The native issue closes on export and points to GitHub"
    how: "Open on GitHub closes the native issue (not planned, or a new moved reason) with a link; its thread goes read-only; comments live on GitHub."
    tradeoff: "Custody becomes one-way after all: the native thread, boards' rows and the inbox lose the issue, and leaving GitHub again needs an import."
  - id: "c"
    label: "Everything in the thread crosses, including what was there before"
    how: "On export, post the thread's comments to GitHub as their authors, then keep sub-thread replies and edits in step."
    tradeoff: "A burst of comments sent as several people who didn't each choose to send them, and edit sync is the reconciler aru-089 rules out."
choice:
  option: "a"
  reason: "Jake's calls for M205 (#62, aru-089): per issue, explicit, sent as the person through aru-085's path, an agent only drafts, comments cross both ways for exported pairs only through the mirror, never a reconciler. Keeping the native issue, linked, is what keeps custody reversible; sending only a person's new top-level comments keeps every write to GitHub one a person made; bringing in only a public repo's keeps aru-086 (linking never widens who reads a private repo); and the rest of the line (no edits, no sub-thread replies, nothing from before) is where a reconciler would begin."
rejected:
  - option: "b"
    why: "It turns Open on GitHub into a move, which aru-089 doesn't ask for and the native boards and inbox would lose."
  - option: "c"
    why: "It sends what people didn't choose to send, and keeping edits in step is the continuous sync aru-089 rejects."
supersedes: []
evidence:
  - title: "#1276 (native #62)"
    url: "https://github.com/arugula-salad/arugula/issues/1276"
  - title: "#1263"
    url: "https://github.com/arugula-salad/arugula/issues/1263"
decided_by: "jhgaylor"
decided_on: "2026-11-02"
reviews: []
constrains:
  - "path:crates/platform/src/forge/export.rs"
  - "path:crates/platform/src/forge/send.rs"
  - "path:crates/platform-chat/src/chat.rs"
  - "path:crates/platform/migrations"
---

# An issue opened on GitHub stays native, and only new comments cross

"Open on GitHub" (M205) is what "Taking custody of issues stays reversible"
(aru-089) promised: a person sends one native issue to a repo linked to its
project, through "Sending as a person uses a one-use user token, falling
back to their machine" (aru-085). This record says what happens next.

- **The export is a draft.** `drafts.native_issue` names the issue; its
  title and body are read again as it's sent. An owner or editor of the
  issue makes it, or their agent with its machine's `issues.write` grant:
  an agent's waits for its person ("Forge blocks use the person's own CLI,
  and agents draft while people send", aru-041). One export per issue at a
  time, and none once it's on GitHub.
- **Sent, the issue's GitHub origin is set** (`issues.github_*`) and its
  timeline gets `exported`, as the person who sent it. The mirror then has
  the GitHub issue, and work items take the two as one.
- **The native issue stays.** Its number, state, labels, assignees,
  timeline and thread are its own, and its page says where it lives on
  GitHub, with a link. Nothing else is kept in step: the two copies'
  titles, bodies and states may drift.
- **Comments cross, for exported issues only, each once.**
  - A person's new top-level comment in the thread (or a reply also sent
    to it) is a draft sent as them at once, through one of their machines;
    with none awake it waits in their drafts, and the post says so. A
    reply kept in a sub-thread stays native.
  - An agent's comment is a draft its person sends; it stays native until
    then.
  - A comment made on the GitHub issue lands in the thread as GitHub's: no
    author, `via` `github:<login>`, labelled with the login. A webhook
    brings it; the poller's turn over the issue brings one whose webhook
    never came. **Only from a public repo:** the thread is the whole
    project's, and a private repo's text reaches only those GitHub lets
    read it ("Private items are shown per viewer, by their own GitHub
    access", aru-086), so on a private repo GitHub's comments stay there.
    A person's comment still goes out.
  - `issue_github_comments` keeps every GitHub comment that crossed, by
    GitHub's id, so neither way brings one back. GitHub's copy of a
    comment that went out, even one whose webhook comes before the send
    answers, is matched to its draft by its text and kept out.
  - Edits and deletions don't cross, and comments from before the export
    stay native.
- **GitHub's text in the thread** is a public repo's, readable by everyone
  in the project, as the issue was. It is never logged.

Where: `crates/platform/src/forge/export.rs` (the draft, the hooks on a
send, the webhook and the poller), `crates/platform-chat/src/chat.rs`
(a post's way out, and GitHub's comment in), `crates/platform/migrations`
(`0090_issue_github`).
From: #1276 (M205, native #62), aru-089, aru-085.
