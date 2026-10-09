# S34: agents working together across people (A2A, an agent catalog without Fountain)

Run 2026-10-06 on geek, against `main` at 0.23.0 (aa275a30). Issue #397; tracker #403.

Only this directory was merged (2026-10-09). The prototype it describes (`crates/daemon/src/a2a/`, `Def.recipe`, `e2e::Caller`, `illogical a2a`) stays on the branch `s34-agents-a2a` (PR #410): the milestones port it, starting with M76 (#398). The scripts here call the pre-rename `illogical` binaries and run against that branch.

**Result: go.** One person's agent found another person's agent, sent it a task, and got back a patch it could apply, with the other person's consent in between. A2A 1.0 rode illogical's end-to-end channel through control unchanged, apart from its headers. Nothing in the run used Fountain:

- A Claude Code agent on jake's machine was told only that teammates offer agents and where `a2a --help` is. It found `fixer` on ale's machine, sent it the failing tests, and brought the fix back. 62 s, haiku on both ends.
- The scripted run takes 27 s end to end, including four approvals by ale ([results/flow.txt](results/flow.txt)).
- The official A2A SDK (`a2a-sdk` 1.2.2, Python) parses everything the daemon serves strictly, in [conformance.py](conformance.py): the card, a blocking `SendMessage`, `GetTask`, `ListTasks`, `CancelTask`, `returnImmediately`, and the error codes.

The prototype is in the daemon:
- `crates/daemon/src/a2a/` adds `/api/a2a/*`.
- `e2e::Caller` gives handlers the caller's account.
- `Def.recipe` dresses an agent block as a recipe.

The CLI has `illogical a2a …` (hidden). The rig is [rig.ts](rig.ts), [up.sh](up.sh), [flow.sh](flow.sh) and [act.ts](act.ts).

## The rig

The rig has:
- **one local control** with a fake GitHub;
- **two people:**
  - `jake`, who makes team `crew`;
  - `ale`, who joins as an editor;
- **three machines:**
  - `jake-box` and `ale-box` are in the team;
  - `ale-solo` is ale's alone;
- **each person's CLI** logged in with its own config dir.

ale's repo `calc` has two failing tests and `.claude/agents/fixer.md`. jake has a clone. Agents run real Claude Code (`claude-agent-acp` 0.85.0) under geek's own login.

## q1: the recipe format

**A recipe is a Claude Code subagent file, `.claude/agents/NAME.md`, with nothing added.**
- It already has everything S24's Fountain recipe had: `name`, `description`, `tools`, `disallowedTools`, `model`, `permissionMode`, `skills`, `mcpServers`, plus the system prompt as the body.
- It lives in the project (where its team can review it) or in `~/.claude/agents/`.
- People already write these files, and the same file works as a subagent in their own Claude Code.

**Offering is the machine owner's choice, kept by the machine.** `illogical a2a offer NAME --dir PROJECT` writes `<state>/a2a-offers.json`. The file is never touched, so a recipe stays plain Claude Code. Nothing is offered by default.

**How a block wears one:**
- `claude-agent-acp` deletes the SDK's `agent` option on purpose ("main-thread agent selection is intentionally not part of this adapter's ACP contract"). So the block can't just select the subagent.
- `Def.recipe` dresses the session the way M44 dresses a worn Fountain agent:
  - the body goes on `_meta.systemPrompt.append`, after a line naming the agent;
  - `model` (`inherit` means none) and `tools`/`disallowedTools` go into `claudeCode.options`;
  - `settingSources: []` stays.
- `--agent` *replaces* the system prompt, but appending keeps Claude Code's own tool instructions. Appending worked for `fixer`.

**Not carried yet:** `skills` and `mcpServers`.
- M44's wear code carries over almost whole, since a subagent's frontmatter maps field for field onto Fountain's `Agent`:
  - named skills become a local plugin (`wear::bundle`);
  - servers go through `wear::servers`, credentials by reference, never on argv. Its sources would be `.mcp.json`/`~/.claude.json` and the environment, not Infisical-by-agent-specs.
- `serde_norway` was added to parse the frontmatter. The daemon had no YAML parser.

## q2: A2A, mapped

The prototype speaks **A2A 1.0** (spec released 2026-03-12, repo at v1.0.1), JSON-RPC binding:
- methods `SendMessage`, `GetTask`, `ListTasks` and `CancelTask`;
- ProtoJSON enums (`TASK_STATE_*`, `ROLE_*`);
- parts like `{text, mediaType, filename}`.

No streaming yet (`capabilities.streaming: false`): the caller polls `GetTask`.

| illogical | A2A |
|---|---|
| an offered recipe | an agent card, at `GET /api/a2a/agents/NAME/card`; `GET /api/a2a/agents` lists all |
| the card's interface | `illogical://MACHINE/api/a2a/agents/NAME`, `JSONRPC`, `1.0`, plus a *required* extension (`…/a2a/ext/channel/v1`): reached over illogical's channel only |
| waiting for the owner's OK | `TASK_STATE_SUBMITTED`, with a status message ("waiting for this machine's owner…") |
| the owner says no | `TASK_STATE_REJECTED` |
| the agent block is working | `TASK_STATE_WORKING` |
| the agent waits on a **permission** (it runs a tool on the receiver's machine) | still `TASK_STATE_WORKING`, with "waiting for this machine's owner to approve: Edit calc.py": the receiver's to answer, on the block (q6) |
| the agent asks a **question** | `TASK_STATE_INPUT_REQUIRED`; the caller answers with the next `SendMessage` on the task |
| the turn ends | `TASK_STATE_COMPLETED`, with two artifacts: `reply` (the last agent message, `text/markdown`) and `patch` (`text/x-diff`, q5) |
| Cancel (either side) | the block's `cancel`, `TASK_STATE_CANCELED` |
| a follow-up in the same task | `SendMessage` with `taskId`, a new turn in the same block |

**A task is an agent block in a git worktree of the project** (`<state>/a2a/TASK`, detached at `HEAD`). The receiver sees it in the swarm like any other agent. That is the "both people watch and can take over" part: jake's agent watched through A2A, and ale could have opened the block.

## q3: transport between people

**Daemon to daemon isn't possible today**, for three separate reasons:
1. **Control** lets only browser sessions and *CLI* signatures onto `/api/relay/c/ID` (`Session`, `control/src/auth.rs:80`).
2. **The receiving daemon** accepts only `browser` and `cli` certificates as clients (`Kind::connects`, `e2e/src/cert.rs:59`). A daemon key is "unknown device".
3. **There's no async initiator** in the daemon. The CLI's `Enrolled`/`Link` is blocking and lives in a binary crate.

**The person's CLI works today.** It reaches any machine its account may reach, with no transport changes.
- The prototype carries A2A on it: jake's CLI (and jake's agent, through it) called ale-box through the relay.
- A2A's JSON-RPC rode the channel unchanged.

**Measured** on the local rig, 10 calls each:

| Path | Per call |
|---|---|
| `illogical --host ale-box a2a agents` (relayed) | 273 ms |
| `illogical --host jake-box a2a agents` (relayed) | 294 ms |
| the same request to ale-box's port, direct | 12 ms |

The relayed time is one CLI process per call: control's directory, the Noise handshake, then the request. Hosted control adds a round trip to Fly on top. A task itself is seconds to minutes, so this doesn't matter for delegation. It matters for a catalog that asks every machine (q7).

**The channel carries only `Content-Type`.** A2A's service parameters are HTTP headers (`A2A-Version`, `A2A-Extensions`). The first call through the relay arrived with no version, read as 0.3, and was refused. The prototype also takes `?a2a-version=1.0`. M78 should add a small header allow-list to `RequestHead` (`A2A-Version`, `A2A-Extensions`), which older daemons ignore.

**Recommendation:** M78 makes the calling *daemon* the client, signed with its own key:
- control accepts daemon signatures on the relay for this;
- the receiver accepts a `daemon` certificate as a caller on `/api/a2a/*` only.

Then a task proves which machine it came from, not just which person, and an agent block needs no CLI login on its machine. Until then, the CLI path is the fallback.

## q4: identity

**The receiving daemon couldn't tell jake from ale.** jake made the team, so he's its owner, and every owner of a team is `Principal::Owner` on all its machines. On ale-box, jake's task looked like the owner's own, and the first run skipped consent entirely. This is #386's root again.
- **Fix in the prototype:** the channel now hands handlers the device on the other end (`e2e::Caller`: account, device id and name), next to the principal.
- **Consent keys on the account:**
  - a task from ale-box's own account (ale's CLI) needs no OK;
  - a task from any other account does, even from a team owner.
- **What the receiver sees:** "fixer from jake on jake's cli (e95a8f3e735c3d01) (says {"machine":"geek","pane":null})".
  - The person and device are proven by the key.
  - The machine and pane in the message's `metadata.illogical` are only claims, shown as such. With daemon-to-daemon (q3), the machine is proven too.

**Cards can't do this yet:**
- The mux refuses an ask on an agent block ("%3 is an agent or remote block: it asks through its own methods").
- An answer through `/api/attention/act` is recorded by principal, where every team owner is `owner`, so it can't prove which account answered.
- The prototype therefore takes consent on its own route, `POST /api/a2a/tasks/ID/consent` (`a2a answer`), which checks the caller's account.
- jake trying to allow his own task got "only this machine's own account allows tasks for its agents".

**Personal machines are invisible to teammates.** ale-solo isn't in control's directory for jake, and the daemon would refuse his channel anyway. Today a teammate's agent can be reached only on team machines. Fixing that (q7) is a narrower grant, not a share.

## q5: results

**A patch against the commit the task started from**, applied with `git apply --3way` in the caller's checkout. It worked in both runs.
- New files are included (`git add -N`).
- **The first live run found a trap:** the receiver's pytest left `__pycache__/*.pyc` in the worktree. Those went into the patch, and `git apply` refused the whole patch because jake's clone had its own `__pycache__`. jake's agent then read the diff from `GetTask` and made the edit by hand.
- **Now:** new binary files the project doesn't track are left out, and the artifact's `metadata.illogical.left_out` names them.
- **Rerun with a `__pycache__` already in the clone:** "Applied patch to 'calc.py' cleanly", and 2 passed.

**What the patch needs:** the caller has the same repository at or near the base commit (`--3way` covers drift).
- A pushed branch is the alternative when both have a shared forge remote, with credentials that stay on the receiver.
- Files moved as in the Images track (#267) suit work that isn't a repository.

**Recommendation:** the patch by default, a branch as an option on the recipe, and both reviewed in a diff block before they're applied (M80).

## q6: input-required, and who answers

**Permissions are the receiving owner's. Questions are the caller's.**
- A permission is a tool running on ale's machine, with ale's files and credentials. The caller can't judge it and mustn't grant it, so the task stays `WORKING` and says what it waits for. ale approves it on the block, from any device.
- A question (AskUserQuestion, an MCP form) is about the task, so it's `INPUT_REQUIRED` for the caller.
- **Not exercised live:** `fixer` never asked a question.

**Permissions were the noise.** Every `fixer` task asked ale four times: `pytest`, Edit, Edit, `pytest`. That's 2 to 5 s each with ale's script answering at once, and a person's minutes otherwise. The standing grant (q4) covers only "may this caller use this agent at all". Recipes need their own allow rules (`permissionMode: acceptEdits`, and #163's per-tool rules) so that offering an agent includes how far it may go unasked.

## q7: catalog discovery

**Live, from each machine:** `GET /api/a2a/agents` returns that machine's cards.
- Control never sees a recipe or a card.
- For a team that's one call per online machine: about 0.3 s each through the CLI here, less with a long-lived daemon link.
- **Recommendation:** the calling daemon caches cards per machine, and shows an offline machine's last cards marked offline.

**Reaching personal machines** needs a grant narrower than a share: "members of my teams may call `/api/a2a/*` here, nothing else".
- The daemon adds those accounts to `others` with an A2A-only principal, and lists them in `/api/daemon/access`, so control's `may_reach` routes them.
- Today only team machines work.

## q8: Fountain's M43 pieces

**Leave them where they are, behind labs (#385), and don't build on them.**
- `fountain/wear.rs` is the useful part: the recipe path reuses its bundling and its by-reference secrets (q1).
- `list_agents`/`read_agent` name Fountain's catalog. M77's team catalog should take the plain names and leave Fountain's tools as `fountain_*`.

## What the lead agent tripped on

The haiku lead on jake-box worked out the CLI from `--help` alone. Three things cost it turns:
- it typed `a2a agents` before `a2a a2a agents`;
- it left out the agent name on `patch`, which needs both agent and task;
- it took a failed `git apply` for "the patch didn't apply" and edited by hand.

**For agents, MCP tools beat the CLI:**
- one to list the team's agents;
- one to delegate and wait, which returns the reply, the patch's stat and what was left out;
- one to apply a task's patch in the agent's own checkout.

The CLI stays for people.

## The track, adjusted (#398–#402)

- **M76 (#398), recipes and cards:**
  - Claude Code subagent files as recipes, unchanged;
  - an owner-kept offer list;
  - A2A 1.0 cards;
  - `Def.recipe` plus M44's bundle for skills and MCP servers;
  - recipe-level allow rules (q6);
  - `illogical agent --as NAME` to run one locally.
- **M77 (#399), the team catalog:**
  - a block and one MCP tool;
  - cards cached per machine;
  - the A2A-only grant that makes personal machines' offered agents reachable by teammates (q7);
  - needs #386's account-level identity, which `e2e::Caller` starts.
- **M78 (#400), delegating:**
  - daemon-to-daemon transport (q3) and the `RequestHead` header allow-list;
  - MCP tools to delegate and apply;
  - `SendStreamingMessage` over the channel's streamed responses;
  - tasks that survive a restart (here they're in memory);
  - blocks and worktrees cleaned up after the result is fetched (every task leaves both behind).
- **M79 (#401), consent:**
  - a card held on an invite-style block (not the agent block), answerable only by the machine's own account, checked by account rather than principal;
  - a push;
  - standing grants listed and revocable;
  - permission prompts that say which task and caller they're for.
- **M80 (#402), results:**
  - the patch with `left_out`;
  - a branch option;
  - review in a diff block before applying.
- **Later:** outside A2A agents. The card's only interface is `illogical://` with a required extension, so an outside client knows it can't call in. A public interface needs its own authentication (OAuth or mTLS from the card's `securitySchemes`), which is its own milestone.

## Reproduce

```
CARGO_TARGET_DIR=$PWD/target cargo build -p illogical-control -p illogicald -p illogical
(cd web && pnpm install)
spikes/s34-agents-a2a/up.sh /tmp/s34-rig     # control, 2 people, 3 machines; fixer offered on ale-box
spikes/s34-agents-a2a/flow.sh /tmp/s34-rig   # jake → ale-box/fixer → patch in jake's clone
. /tmp/s34-rig/jake.env; illogical --host ale-box a2a agents
python3 -m venv v && v/bin/pip install a2a-sdk && \
  v/bin/python -I spikes/s34-agents-a2a/conformance.py http://127.0.0.1:PORT "$(cat /tmp/s34-rig/ale/ale-box/local-token)" fixer
pkill -f "rig.ts /tmp/s34-rig"
```

`PORT` is `machines["ale-box"].port` in `rig.json`.
- Agents use the machine's own Claude Code login.
- The rig strips `CLAUDE*`/`ILLOGICAL*` from its environment, so nothing reaches your own daemon.
- Each person's CLI keeps its key under the rig's state directory, never in `~/.config/illogical`.
