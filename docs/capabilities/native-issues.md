---
id: "native-issues"
title: "Native issues"
verified_through: "665cdb10ed705245af5afaf697068c829f8504b6"
entry: ["crates/platform/src/issues.rs", "crates/platform/src/issues/agent.rs", "crates/arugulad-control-tools/src/issues.rs", "crates/daemon/src/mcp/issues.rs", "crates/cli/src/cmd/issue_native.rs", "web/src/platform/pages/issues.tsx", "web/src/platform/pages/issue.tsx"]
paths: ["crates/platform/src/db/issues.rs", "crates/platform/src/forge/export.rs", "crates/platform/src/forge/closes.rs", "crates/control-wire/src/issues.rs", "crates/arugulad-forge/src/native.rs", "web/src/shared/issues"]
shared: ["crates/platform/src/forge/send.rs", "crates/platform/src/forge/webhook.rs", "crates/platform/src/forge/ingest.rs", "crates/platform/src/forge/poller.rs", "crates/platform/src/db/work_items.rs", "crates/platform-chat/src/chat.rs"]
tests: ["crates/platform-chat/src/issues_tests.rs", "crates/platform-chat/src/issue_github_tests.rs", "crates/platform-chat/src/session_issues_tests.rs", "crates/platform/src/forge/closes_tests.rs", "crates/platform/src/forge/auto_attach_tests.rs", "crates/platform/src/db/work_items_tests.rs", "crates/arugulad/tests/integration/issue_pins.rs", "web/e2e/issues-platform.spec.ts"]
decisions: ["aru-041", "aru-084", "aru-085", "aru-086", "aru-088", "aru-089", "aru-130"]
---
# Native issues

## What a person can do
A project has numbered issues of its own on the platform: open, edit, label, assign, close, reopen, comment and export them, with every machine asleep. A person can send one native issue to a repo linked to its project, as a new GitHub issue in their own name, and after that new top-level comments cross between the two.

Project membership and roles alone decide who reads and changes native issues. Control's chat setting doesn't (Arugula #746), for a person or for an agent acting for them. An issue's comments are its chat thread, so they still need chat. With chat off for the reader (the asker, for a hosted session), the page, `arugula issue show` and `arugula issue agent`, the export and a session's read of an issue leave the comments out and say they're in chat.

## Where it runs
Short paths below are under `crates/platform/src`.

**The platform process owns them.** Its tables are `issues`, `issues_fts` and `issue_events`, in plain text. An issue's comments are the messages of its object thread (`chat_messages`), posted through chat's routes. `POST /api/projects/{project}/issues` goes `create_route`, `create`, `place`, then `db::issues`.

**Who may** (`place` in `issues.rs`): a personal project's account does everything. In a team's project, owners and editors open, edit, close, reopen, label and assign; viewers read, search, export and comment. No one deletes a single issue. Anyone else gets 404.

**Agents and sessions:**
- A machine's agents use `issue_write`, `forge_read` and `read_forge`, which call `/api/daemon/issues/...` signed by the machine (`issues/agent.rs`). Each call needs a grant over the project, with the owner's role as the ceiling: `threads.read` to read, `issues.write` to change, `threads.post` to comment. Their writes land at once, labelled with the machine.
- A hosted session's agent reads, comments and opens through `read_session_issues` and its neighbours (`crates/arugulad-mcp/src/session.rs`, `crates/platform-chat/src/sessions/issues.rs`), with its asker's rights, in its own project only.
- `crates/arugulad-forge/src/native.rs` is a pin block on a machine, read as its owner. It keeps only the id, number, project and title.

**The GitHub mirror is a separate thing.** For a linked repo, `forge/webhook.rs` and `forge/ingest.rs` write `forge_items` rows, metadata only, and `forge/poller.rs` backfills and reconciles. A mirrored GitHub issue is not a native issue, and I found no path that turns one into a native issue. `home_of` says where a project's new issue goes: GitHub while a repo is linked and nobody chose, else native.

**Sending one to GitHub:**
- `draft` in `forge/export.rs` runs from `POST /api/forge/drafts` with `native_issue` set (`forge/drafts.rs`). It makes a draft for the project's one linked repo, or the repo named. An owner or editor makes it, or their agent with `issues.write` and `cloud.drive` (`forge_draft` kind `open_on_github`).
- Only a person sends it (`forge/send.rs`): a one-use user token, else one of their machines through `gh`. A machine asking to send is refused (`AGENTS_DRAFT`).
- `before_send` reads the title and body again. When it's sent, `sent` calls `set_issue_github`, which sets the issue's `github_*` columns and adds an `exported` event.
- The native issue stays, with its own number, state, labels, assignees and thread. Title, body, state, labels and assignees are not kept in step afterward.
- `db/work_items.rs` treats the native issue and the mirrored GitHub issue as one work item when the repo is linked to the same project, so a board shows one row.

**Comments crossing, for exported issues only:**
- Out: `post_replied` in `crates/platform-chat/src/chat.rs` calls `export::posted` for a top-level comment in an `iss_` thread, or a reply also sent to it. A person's becomes a draft sent at once through one of their machines (`send_out`), and the result comes back on the live stream. An agent's stays a draft its person sends.
- In: `export::webhook` and `export::polled` call `bring_in`, which posts through `chat::github_comment` with no author and `via` `github:<login>`. It takes comments only from a repo still linked to the issue's project and public. A private repo's comments stay on GitHub, while a person's comments still go out.
- `issue_github_comments` keeps each GitHub comment id that crossed, so none comes back. `issue_github_held` holds a GitHub comment with the same text as one being sent, until the send's answer says which is ours.
- Edits, deletions, sub-thread replies and comments from before the export don't cross.

**A merged PR** closes the native issues it names, in the project its repo is linked to (`forge/closes.rs`), as whoever merged it if they're in the project, else as the App. It never reopens one. What closes: `Closes #N` (or `Fixes`, `Resolves`) and `Arugula #N`, `(#N)` in the title, a branch `feat/N-…`, and a board row the PR was attached to. `part of Arugula #N`, and any other `#N` in the body (`(#N)` in passing among them), only link; so does the branch's number when the text says `part of` it (`boards/refs.rs`). The `closed` event keeps the PR and the words that closed it (`by`), which the timeline and the board row show.

## Compatibility
- `export_route` gives every issue with its timeline and comments as JSON or Markdown (`IssueExport`), and `issues_tests.rs` reads an export back. That is the way out aru-089 asks for.
- A draft is never lost. A failed send leaves it `failed`, and with no way to send it waits in the person's drafts (`NO_WAY`). `send.rs` passes over a machine that is away, too old to know the `forge.send` op, or has no `gh` login.
- With every machine asleep, a person's comment on an exported issue stays native and waits in their drafts. Nothing else about native issues needs a machine. A comment whose webhook never came is found by the poller's next turn over that issue, but only among its first 100 comments: `forge/export.rs` reads one page and drops the rest.
- A daemon on a control without these routes tells the agent that native issues aren't available yet (`said` in `crates/daemon/src/mcp/issues.rs`).
- The pin block keeps what the platform last said, marked stale, while the platform is out of reach.

## Not known
- Is there meant to be a way to bring a GitHub-first issue in as native? I found none, and aru-089 speaks only to going out.
- Is drift between the two copies acceptable to people using them, or only a limit of the current design? The decision accepts it as a tradeoff, and only a person can say what we tell users.
- What do we promise about export staying complete as new things attach to issues? The test covers issues, events and comments.
- The tests use a fake GitHub, so I can't say from the code how this behaves against GitHub's real webhook timing.
