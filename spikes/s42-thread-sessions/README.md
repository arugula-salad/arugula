# S42: sessions on the hosted box, pushing `arugula/*`, snapshots, and #124 (Arugula #130)

Run on 2026-10-25 on geek, against `main` at 0.61.0 (d0cb8bdb), with static binaries built from this
branch (`just static`; the daemon is unchanged). Issue Arugula #130, tracker Arugula #129 (thread =
session). The milestones it feeds are M255 (Arugula #132), M258 (#135), M259 (#136) and M263 (#140).

**Result:** all four questions have an answer, and none is "drop it". Each changes its milestone,
mostly by adding steps the plan didn't have:

| q | answer | verdict |
|---|---|---|
| 1. sessions on the box | Yes. An ACP agent block's agent runs as `ses1` (not the daemon's `user`) in its own checkout, through relay, permission, question, commit, Stop and a daemon restart, on a hosted loam box. | **Changes M255 like so** (q1 below): fixed session users in the template; a checkout that is a clone, not a `git worktree`; all of the daemon's git in it runs as the session user; the firewall covers session users; no secrets on sudo's command line. |
| 2. pushing | `arugula-io` has `contents: write`, and also `workflows: write`. Installation tokens can't be scoped to refs. The box enforces the ref: the daemon pushes from a repo of its own, with a bundle from the session, one refspec, and a token minted per push without `workflows` (since 2026-10-26, with `workflows`: q2's note). | **Changes M259 like so** (q2 below). |
| 3. snapshots | E2B: yes. A box made from a wiped snapshot started clean, and its daemon made its own key (after one leak was fixed: the journal). loam's E2B API has no snapshots (404). Its own API has checkpoints and forks, and the same test passes there. | **Changes M263 like so** (q3 below): the wipe has a list, the journal included; full-memory snapshots, or a boot-time start. |
| 4. #124 | Reproduced on loam with the start command's service stopped. Root cause: on E2B nothing restarts `arugulad --hosted` once it exits. A pause keeps it dead. The platform takes "the provider says running" for "ready". The hub's "woke, but didn't dial in" is told to the opener and to nobody else. | **Changes M258 like so** (q4 below). |

What was run (everything under `results/`):
- **loam, end to end** (`rig.sh`, `loam-claim.sh`): a testnet whose platform made boxes on loam's
  E2B API, enrolled by sandbox id, at a control behind a cloudflared quick tunnel. Then
  [session-proto.sh](session-proto.sh) ran on the box in a pane through the relay
  ([q1-loam.txt](results/q1-loam.txt)), and #124's kill/stop/pause/wake ran
  ([q4-loam.txt](results/q4-loam.txt), [q4-control-log.txt](results/q4-control-log.txt)). The
  template was `packaging/hosted/Dockerfile` plus [template-s42.Dockerfile](template-s42.Dockerfile).
- **The same image in a local container** (Docker, no network): session-proto.sh again
  ([q1-docker.txt](results/q1-docker.txt)), a daemon restart under a session
  ([q1-docker-restart.txt](results/q1-docker-restart.txt)), and the push
  ([push-proto.sh](push-proto.sh), [q2-push-docker.txt](results/q2-push-docker.txt)).
- **E2B** (the lead ran it, with Jake's approval of the key; one throwaway sandbox at a time, from
  E2B's `base`, never enrolled): [e2b-snapshot.sh](e2b-snapshot.sh)
  ([e2b-snapshot-run1.txt](results/e2b-snapshot-run1.txt) found the leak, and
  [e2b-snapshot.txt](results/e2b-snapshot.txt) passes with the fix). The same script on loam's own
  checkpoints: [loam-snapshot.txt](results/loam-snapshot.txt). Every sandbox and snapshot was
  deleted, and the logs list what was left: none.

**Not proven:**
- **A real Claude Code turn as a session user.** It wasn't run (the lead's call: the mechanism is
  proven with loam plus `fake_acp.py`, and step 5's loam run proved a real haiku turn as `user`).
  What's different for Claude is its environment (q1, point 3) and its `~/.claude` in the session
  user's home. To run it, use the loam rig with the project key set (as step 5's
  `cloud key set anthropic`), then, on the box:
  `arugula agent --cwd /home/ses1/work/<ses> --acp "sudo -n -u ses1 -- env -i HOME=/home/ses1 PATH=/usr/local/bin:/usr/bin:/bin ANTHROPIC_API_KEY=<the box's placeholder> NODE_EXTRA_CA_CERTS=/usr/local/share/ca-certificates/e2b-ca.crt /home/user/.local/share/arugula/agents/claude/node_modules/.bin/claude-agent-acp" --model haiku "Reply with S42-OK"`.
  The adapter must be readable by ses1, which it isn't today (q1, point 1). As a generic `--acp`
  block it also skips M185's turn gate, so M255 must open it as kind `claude`.
- **Signed session events.** Every field is there in the block's `state` and `turn_result` calls (q1),
  and run events show the signing and delivery path. But no `arugula session event v1` was built or
  sent: that's M254's wire types.
- **A real push to GitHub,** per the brief. The push went to a local bare repository.
- **#124's OOM itself.** The daemon was killed (`kill -9`) and stopped. No box was run out of
  memory.

The prototype code is all in this directory, and none of it is for main. No crate was changed.

## q1: can a hosted daemon run a session as its own Unix user, in its own worktree?

**Answer: yes.** It works with today's daemon and no code change: the block's command is
`sudo -n -u ses1 -- env -i … <the agent>`, and its `cwd` is the session's checkout. What M255
builds is that same command, made by the daemon, plus the eight points below.

**Evidence** ([q1-loam.txt](results/q1-loam.txt) on a hosted loam box; the same in
[q1-docker.txt](results/q1-docker.txt)):

- The block's process tree: the daemon's shim (as `user`), then sudo, then the agent as ses1.
  Stdio is plain pipes: sudo uses no pty when it has no terminal, so ACP goes straight through.

  ```
  4214  4068 user  4067 /usr/local/bin/arugulad _shim --record …/blocks/4/process -- sudo -n -u ses1 -- env -i HOME=/home/ses1 …
  4215  4214 root  4215 sudo -n -u ses1 -- env -i HOME=/home/ses1 … /usr/bin/python3 /opt/s42/fake_acp.py
  ```
- **A turn** (`hello`), **a permission** (`run …`: `attention: needs_input`, `current_tool.title`,
  and `pending[0]` with its options `allow-once`, `allow-with-updates` and `reject`; `approve` →
  "Ran it."), and **a question** (`ask one`: `asks[0]` with its `questions` and JSON `schema`;
  `answer {"question_0": "Blue"}` → answered).
- **Work that commits, as ses1:** `work README.md NOTES.md` gives `35b95db add NOTES.md` on
  `arugula/ses_s42demo-fix-readme`, with ` M README.md` left uncommitted. But
  `turn_result` came back with only `{"message": "done"}` and **no `work`**: turnwork's git runs as
  `user`, which can't enter the session's 0700 directory, and turnwork fails silently.
- **Stop:** `cancel` gives `{"cancelled": true}`, and `close` leaves no ses1 process
  (`pgrep -u ses1` is empty).
- **A daemon restart** ([q1-docker-restart.txt](results/q1-docker-restart.txt)): the agent dies with
  the daemon, since a hosted box has no FD store. On restart the block starts it again as ses1 and
  loads its session from ses1's home: "remember plum", restart, "recall" → "You said plum."

**Changes M255 like so:**

1. **Session users are the template's**, as `runner` is: `ses1`, `ses2` and `ses3`, one per
   session slot (the cap is 3). Each has no sudo and a home at 0700, and `Defaults>sesN env_reset`.
   They are **not in `user`'s group**, unlike `runner`, so they can't read `/home/user`, setup's
   files there among them (`~/.npmrc` and the like). A session keeps its slot for life: its
   `~/.claude` and ACP session live in that home, and a restart must find them (point 8). This
   means the adapters must move out of `/home/user/.local/share/arugula/agents` to a root-owned
   place everyone can read (`/opt/arugula/agents`, say), or ses users can't run Claude Code.
2. **The checkout is a clone, not a `git worktree`.** A worktree's metadata lives in the main
   clone's `.git/worktrees/`, which only `user` may write: as ses1, `git commit` gives `fatal: not a
   git repository: /home/user/s42-repo/.git/worktrees/s42-wt` (step 1a). Instead, `user` clones the
   workspace's clone (`--no-hardlinks`) to a staging directory, then `sudo chown -R sesN` and
   `sudo mv` it to `/home/sesN/work/<ses>`. Then, as sesN, it sets origin to the GitHub URL (no
   credentials) and switches to `arugula/<ses>-<slug>` (step 1b). A big repo wants `--reference` to the workspace's clone, which the session user can read
   once the objects are made group-readable for a `sessions` group. M255 picks between the two.
3. **No secret on sudo's command line.** Another session user read
   `sudo -n -u ses1 -- env -i S42_TOKEN=… sleep 30` from `/proc/<sudo>/cmdline` (step 4d). On E2B,
   sudo also logs its command lines to the journal (q3). What goes on the command line: `HOME`,
   `PATH`, the provider key's placeholder (`agent::hosted::env`, which isn't a secret) and
   `NODE_EXTRA_CA_CERTS`. Anything secret (an MCP token, if a session ever gets one) goes on the
   agent's stdin first: a tiny `arugulad _session-exec` reads one line of environment, then execs
   the adapter, as runs already pass their login. Keep the block's kind `claude`, so M185's turn gate
   and placeholder still apply. Add the prefix in `spawn` (`agent/mod.rs`, at the
   `link::LocalSpawn`), not as a custom `--acp` command.
4. **All of the daemon's git in a session's checkout runs as its session user**: turnwork,
   `diff`, `status`, the bundle for the push (q2). As `user`, git can't even get in (step 4a). The
   tempting fix is wrong: `sudo git` is refused for "dubious ownership" (step 4b), and with
   `safe.directory` added, the agent's `core.fsmonitor` **ran as root** (step 4b'). As ses1, the
   same planted config runs as ses1 and harms nothing (step 4c). This touches `turnwork.rs` and
   `review::Runner`: a runner "as sesN" wraps git in `sudo -n -u sesN`.
5. **The firewall covers session users.** On loam, from ses1, both the daemon's port 7681 and
   envd's 49983 were open (step 4e): `work::firewall` keeps only `runner` off loopback, and only
   once a run has run (the ruleset was still empty). Extend it to
   `meta skuid { runner, ses1, ses2, ses3 }` and put it in before the first session too. A
   consequence is that sessions get no `arugula` MCP server, because the daemon's port is closed to
   them. That's the safe default. A session-scoped one can come later.
6. **The agent gets no file access through the daemon.** The daemon offers ACP's client
   capabilities as `fs: {readTextFile: false, writeTextFile: false}` and `terminal: false`, and
   answers `fs/read_text_file` as unsupported (`agent/mod.rs:1852`, `agent/tests.rs:174`). That's
   what keeps an agent running as ses1 from reading `user`'s files through the daemon. Make it a
   test that names sessions, so nobody turns it on for them later.
7. **When a session ends, everything its user runs is killed** (`work::kill_all` with sesN), as for
   runs. Stop and close already left nothing here, but a background process would outlive both.
8. **Restarts:** the agent dies with the daemon and comes back as the same user with `session/load`.
   That's why the slot must be kept with the session (point 1). M255's "a restart neither loses
   nor doubles a prompt" is about the queue, and this doesn't change it.
9. **What a signed event carries** is all in the block's `state`, today:
   - `attention` (`working`, `needs_input`, `idle` or `done`);
   - `current_tool.title`;
   - `pending[]`: a permission, with its `options`;
   - `asks[]`: a question, with its `questions` and `schema`;
   - `error` and `recent_turns`;
   - `turn_result` (`message`, then `work` once point 4 is done).

   Shape it like run events (`arugula_e2e::run::Event`): an `arugula session event v1` body with
   the session, seq, state, at, tool title, error and the payload's sha256, signed with the
   machine key. The payload goes beside it, as a run's operation goes beside its hash, and control
   checks the hash. A question's schema runs to a few kB, too big to sign inline.
10. Cosmetic: the block's label says `sudo` (argv[0]). Label sessions from their definition.

## q2: can the `arugula-io` App's token push an `arugula/*` branch, and only that?

**Answer:** it can push, but not "only that" by itself. `gh api /apps/arugula-io` (public) shows:

```
{"actions":"read","checks":"read","contents":"write","issues":"write","members":"read","metadata":"read",
 "pull_requests":"write","statuses":"read","workflows":"write"}
```

- **`contents: write`: yes.** So an installation token can push branches.
- **Refs: no.** `POST /app/installations/{id}/access_tokens` narrows by `repositories` or
  `repository_ids` and by `permissions` only. There is no ref or branch parameter. Tokens last an
  hour, and `DELETE /installation/token` revokes one early. The platform already uses both:
  `forge/app.rs` `mint`, which logs every mint and refuses a token that covers more than the one
  repo, and `revoke_url`, which M219's deploy boxes call.
- **Rulesets** can confine refs, but they aren't ours to rely on. A ruleset targets refs with
  fnmatch patterns ("restrict creations, updates, deletions" on everything but
  `refs/heads/arugula/**`). It applies to everyone except its bypass actors: repository roles
  (write, maintain, admin), teams, Apps or Dependabot. So "the App only on `arugula/**`" means
  every human role and every other bot must be a bypass actor. A repo admin makes that setting, and
  `arugula-io` has no `administration` permission to make it for them, nor should it. It can be
  optional hardening in the docs, never the guarantee.
- **`workflows: write` is the sharp edge.** A token that has it can push a branch that changes
  `.github/workflows/`. An App's push (unlike `GITHUB_TOKEN`'s) triggers `on: push` workflows,
  with the repository's secrets. Mint the push token with `{metadata: read, contents: write}` and
  nothing else. GitHub then refuses a push that touches a workflow file, because the token lacks
  `workflows`. Even so, any `arugula/*` push runs the repo's existing push workflows on code the
  agent wrote, which is no different from a member pushing a branch. aru-115 should say so.
  - **Changed since (Jake, 2026-10-26; aru-115, amended):** the token has `workflows: write`
    too. GitHub compares a new branch's tree with the default branch's, so once main changed a
    workflow file after a session began, the branch's older file read as an edit, and every push
    of it was refused (the v0.63.0 live test). A session's branch can now change CI files, and
    the PR's reviewer checks them.

**How the box enforces it** ([push-proto.sh](push-proto.sh),
[q2-push-docker.txt](results/q2-push-docker.txt); a local bare repo stood in for GitHub, and the
token was fake):

1. The session user writes `git bundle create … refs/heads/arugula/<ses>-<slug>` in its own
   checkout, as itself (q1, point 4).
2. The daemon reads the bundle's bytes (as sesN) into a file of its own, and
   `git bundle list-heads` must name exactly that one ref, or nothing is pushed.
3. The daemon fetches the bundle into a bare repository of its own, which the agent never touches.
   It runs under `env -i`, `GIT_CONFIG_NOSYSTEM=1`, `GIT_CONFIG_GLOBAL=/dev/null` and
   `core.hooksPath=/dev/null`.
4. It pushes `refs/heads/R:refs/heads/R` with `--no-follow-tags`, where R is checked against
   `arugula/<ses>-` first. The token is minted for this push, for this repo, with contents only (and workflows since 2026-10-26: q2's note).
   It reaches git through `credential.helper` reading an environment variable of that one process:
   never a file, never argv. Then the token is revoked.

What the run showed:
- `push main` and `push arugula/ses_other-x` were refused before git ran. The remote got only the
  session's branch.
- The pre-push hook, `core.fsmonitor`, `remote.origin.push = +refs/heads/*:refs/heads/*` and the
  credential helper the agent planted in its checkout did nothing to `user`.
- The token was found nowhere on disk.

**Changes M259 like so:**
- "The box pushes" means the daemon pushes from its own mirror, never from the session's checkout.
  The session's part is a bundle.
- The push token's scope is a new `Scope` in `forge/app.rs` with `{metadata: read, contents:
  write}`, for one repo, minted per push and revoked after, as `DeployScope::Contents` is.
- The ref check sits in the daemon (`work.rs`, the push), and a test pushes `main` and another
  session's branch and is refused. Control should refuse to mint for a session whose box asks for a
  ref other than its own name, so a confused daemon can't ask either.
- **What this doesn't stop:** an agent that gets from ses1 to `user` (who has blanket sudo; "the
  real boundary = uid + user's blanket sudo" is Jake's open decision from step 5). Such an agent
  could use a token in the seconds it lives, on any ref. Only control holding the token and
  checking the ref closes that. One way is a git smart-HTTP proxy in control: it reads the
  receive-pack command list (`old new ref` before the pack), refuses any other ref, and forwards
  with a token the box never sees. That's a later hardening, and it doesn't change the box's side
  above.

## q3: can E2B snapshot a set-up box, and does a box from it start clean?

**Answer: yes on E2B, once the wipe is complete. loam has the same mechanism, but not on the E2B
API the platform calls.**

The test ([e2b-snapshot.mjs](e2b-snapshot.mjs)) used one sandbox at a time:
1. A builder box gets setup-like state: a fake enrolled daemon state directory (`daemon.key`,
   `control.json`, `run-grant-revokes.json` and its `.kept` marker, `hosted-setup.json`,
   `hosted-box`, a run ledger); a secret in `~/.npmrc`, `~/.git-credentials` and `/etc` (written
   through sudo); the secret in a running process's environment; and `~/repo/built.txt`, setup's
   real work. It is snapshotted as it is.
2. child1, made from that snapshot, shows what a snapshot carries. Then the wipe runs, and the real
   `arugulad --hosted` (this branch's static build, pointed at 127.0.0.1:9) is started fresh. Then
   come a full snapshot and a filesystem-only one.
3. A child is made from each of those, and checked.

**E2B** ([e2b-snapshot.txt](results/e2b-snapshot.txt), PASS):
- Snapshots take 0.5–1 s (full) or about 0.2 s (filesystem-only: the team has it). A child starts
  in 0.5–2 s. The source keeps running.
- In every child, `/run/e2b/.E2B_SANDBOX_ID` names the new box. That's what M180's fresh-box check
  keys on.
- **An unwiped snapshot carries everything:** the old `daemon.key` (same hash), `control.json`, the
  kept revokes, setup's state, the old box's note, every secret file, and, in a full snapshot, the
  running process with the secret in its environment. A child's create-time `envs` are its own;
  metadata is the create's.
- **First run** ([e2b-snapshot-run1.txt](results/e2b-snapshot-run1.txt), 3 FAILs): the secret
  survived the wipe in `/var/log/journal/<id>/user-1000.journal`. E2B's base runs systemd and
  journald (PID 1 `/sbin/init`), and journald holds its file open and makes it again. The entry was
  sudo's command-line log (`SYSLOG_IDENTIFIER sudo`, `COMMAND=/usr/bin/sh -c 'printf … <secret> >
  /etc/s42-app.conf'`). It came from the probe's own `sudo grep "$secret"` in run 1, and from the
  deliberate setup step in run 2. A real setup that hands sudo a secret on its command line leaves
  it there just the same.
- **Second run:** with `journalctl --rotate && journalctl --vacuum-files=1` first in the wipe, both
  wiped children were clean: no old daemon state, no secret on disk or in a process, and setup's
  work still there. In the full child, the fresh daemon logged "a new box; starting over with keys
  of its own", noted the new box, reseeded and made its own key (`cddae6…`, the old one `5e21c7…`).
  In the filesystem-only child **nothing started arugulad**: it boots from disk, and E2B's start
  command runs only at template build ("does not re-execute each time you create a sandbox").

**loam** ([loam-snapshot.txt](results/loam-snapshot.txt), PASS; `S42_BACKEND=loam`):
- The E2B API answers 404 for `/sandboxes/{id}/snapshots`, `/snapshots` and `/fork` (loam's
  docs/E2B.md, "Not here").
- loam's own API has the equivalent: `POST /v1/sandboxes/{id}/checkpoints` with `{memory}`, then
  `POST /v1/sandboxes {"from": {sandbox, checkpoint}}`. The same test passes there, the daemon's
  re-key included. loam has no journald.
- Differences that matter: a checkpoint is deleted with its sandbox, so the builder must outlive
  every box made from it. A fork made on loam's API is invisible to its E2B API ("Paused sandbox …
  not found"). So the platform's `E2b` substrate can't use it, and only a loam-native substrate
  could. M263's "loam can't test it" stands.

**Changes M263 like so:**
- **`Substrate::snapshot(id, mode) -> snapshot id`, and `delete_snapshot`.** E2B's create takes
  the snapshot id as its `templateID`, so `create` needs nothing new. A snapshot outlives its box,
  so a rebuild or a spec change deletes the old one. They're templates on the bill.
- **The wipe is a list, run as root on the builder before the snapshot:**
  1. stop arugulad;
  2. remove the whole state directory (not just `PER_BOX`: `control.json`, the kept revokes and
     `work/` are left out of it, and an enrolled builder has all three);
  3. `journalctl --rotate && journalctl --vacuum-files=1`;
  4. delete every file holding one of the workspace's secret values: scan for them, since the
     builder has the values;
  5. kill every process holding one in its environment;
  6. remove shell histories;
  7. start arugulad fresh, so it waits for a box of its own.

  Then scan again and refuse to snapshot on a hit. The scan is M263's "a box started from a
  snapshot carries no old machine key, revokes or secrets" test, run in the box.
- **Full or filesystem-only:** a full snapshot keeps the waiting daemon running, as the template
  does today, and it worked. But it also keeps every other process's memory, and a secret in a heap
  isn't found by an environment scan. So either kill every process that wasn't in the template's
  baseline before a full snapshot, or take filesystem-only and start the daemon at boot. E2B's base
  has systemd, but whether our `ubuntu:24.04` template has it under E2B wasn't checked: `ps -p 1`
  on one throwaway box from `arugula-workspace` answers it. Recommendation: full, with the
  baseline kill. It needs no boot path, and it's what was proven.
- **Templates:** `Defaults !syslog` for sudo, or journald `Storage=volatile`, keeps command lines
  off the disk. That isn't enough alone: `/run` is in a full snapshot's memory. Setup's docs
  should say to pass secrets in the environment, never on a command line.
- The snapshot is per workspace, as planned: it holds setup's work, made with that workspace's
  secrets.

## q4: #124, a box whose daemon died pauses for good

**Reproduced** on loam ([q4-loam.txt](results/q4-loam.txt),
[q4-control-log.txt](results/q4-control-log.txt)), on a hosted box made through the platform:

- **q4a, with a supervisor:** `kill -9` the daemon. loam runs the template's start command as a
  service (`e2b-template-start`, `restarts 0 → 1`, a new pid). The box was reachable through the
  relay again in 7 s. So loam doesn't show #124 at all.
- **q4b, without one,** as on E2B: the service was stopped, and the box paused at the provider.
  The workspace said `paused`.
  - `cloud open --wait` answered in 2 s, and the workspace said **`ready`**.
  - Opening the machine (`ls --host dev`) failed after 16 s: "can't reach dev (the relay:
    websocket handshake interrupted)". Control logged "a hosted workspace's wake answered …
    answer=Awake", then 15 s later "a hosted machine's box woke, but it didn't dial in".
  - 20 s later the workspace still said `ready`, with nothing recorded.
- **q4c:** the service started again. The machine dialed in, and was reachable 7 s later.

**Root cause, in four parts:**
1. **Nothing in an E2B box restarts the daemon.** `arugulad --hosted` is the template's start
   command (`packaging/hosted/template.mjs`). E2B runs it once, at build, and snapshots it running;
   it never runs it again. When it exits (#124's OOM, a crash, a `kill`), nothing restarts it.
   loam supervises it, which is why loam boxes recover.
2. **A pause keeps it dead.** Resume restores memory as it was, with the daemon gone. The hold sees
   a silent daemon as idle (`hosted/hold.rs`), so the box pauses at its idle timeout and stays that
   way.
3. **The platform can't restart it, and doesn't look.** The E2B adapter drops the envd access token
   on purpose (`compute/e2b.rs`: "never kept"), so the platform has no way to run anything in a
   box. Its wake (`hosted/worker.rs` `wake`) sets the workspace `ready` when the provider's resume
   succeeds. It never checks that the daemon dialed in.
4. **The one thing that notices, notices for the opener only.** Control's `wake.rs` waits
   `REDIAL` (15 s) and answers `WakeCode::Silent` ("The box woke, but Arugula in it didn't
   connect") to whoever opened the box. It doesn't tell the platform. The workspace stays `ready`,
   and MCP's `workspace_machine` gives up after 90 s with "isn't up after 90s (ready)".

Unverified: that #124's boxes died of OOM. The daemon has no `oom_score_adj` anywhere
(`grep -rn oom_score crates packaging`: nothing). On a 2 GB box a cargo link is the likelier
victim, but the OOM killer picks by score, and nothing protects the daemon.

**Changes M258 like so:**
- **A supervisor in the box comes first.** Make the template's start command a supervisor, as loam
  already does for us. It can be `arugulad --hosted` as a tiny parent that forks the daemon and
  starts it again on exit, with backoff. Or, if E2B gives our template systemd (see q3), a unit
  with `Restart=always`. Either way set `OOMScoreAdjust=-1000` (or `oom_score_adj` through the
  template's sudo rule) on the parent and the daemon. The parent must be in the snapshot: the start
  command is what E2B snapshots.
- **Then the platform's check, for what the supervisor can't fix:** after a wake, the worker waits
  for the machine to dial in (the hub knows: `HubPort::activity`, or control's `Silent`). If it
  doesn't come within N s, the workspace says why (an `error` such as "its daemon didn't come back
  after the box woke") and pushes its maker, instead of saying `ready`.
- **"The worker restarts it"** needs a way into the box, and today there is none. It's a choice:
  1. **The in-box supervisor only** (recommended). The platform marks the box for rebuild and says
     so. It keeps the envd token unheld.
  2. **Keep the envd access token, sealed, in the platform's database**, so the worker can start
     the daemon through envd's process API. That reverses a deliberate decision: it gives the
     platform a way to run commands in every box.

  M258 as written ("the worker restarts it") assumes option 2. I recommend option 1, with option 2
  only if a supervised daemon is ever seen staying down.
- **The tests:** `wake_tests.rs` and `hold_tests.rs` get a fake substrate whose daemon never dials
  in, and the workspace must end up with the error, not `ready`. They need to see the hub's
  "dialed in" (a fake `HubPort`) as well as the substrate.
- `rebuild` should say plainly what `--force` loses (#124's third point). Saving unpushed work
  without the daemon needs the provider's file API, and so option 2's token. Leave that out of M258.

## For aru-115

aru-115 amends aru-091 (and aru-041, for the push). What this spike says it should contain:

- **What a session is:** an ACP agent block of kind `claude` on the project's hosted box, run as
  one of the box's session users (`ses1`…`ses3`, one per slot, at most three at once). Those users
  have no sudo, no way into `/home/user` or the daemon's state, and no loopback to the daemon or to
  envd. Each session works in a clone of the workspace's repository in its own home, on
  `arugula/<ses>-<slug>`. It is never a `git worktree` of the workspace's clone.
- **Who it acts as:** the asker. Its prompts are credited to their authors, and it runs on the
  project's key through M185's gate (kind `claude`, the placeholder, never the key).
- **What runs as whom:** everything the daemon does in a session's checkout runs as that session's
  user, never as the daemon's user or root. The daemon offers the agent no file or terminal access
  of its own (ACP `fs` and `terminal` stay off).
- **Pushing:** only the daemon pushes, from a repository of its own, a bundle of exactly
  `refs/heads/arugula/<ses>-<slug>`. The token is minted per push for that one repository, with
  `contents: write` and never `workflows`, handed to git through the environment of that one
  process, and revoked after. Any other ref is refused before git runs. Control mints only for the
  session's own ref name. The PR stays a draft the asker sends with their one-use user token
  (aru-041, amended): machines never send, merge or approve.
- **What it doesn't promise:** that a branch push can't run the repository's own `on: push`
  workflows. They run, as they would for a member's push, with the repo's secrets, on code the
  agent wrote. Repository rulesets that confine the App to `arugula/**` are the owner's optional
  hardening, not something Arugula sets. An agent that escapes its session user to `user` could
  misuse a push token while it lives: control holding the token (a receive-pack proxy) is the
  later fix.
- **Secrets:** nothing secret on a command line on a hosted box (sudo logs them, and other users
  can read them in `/proc`).
- **Where `@arugula` dispatches:** on hosted project boxes only, never personal machines (Jake,
  2026-10-25). The session status card is the one card kind allowed in a chat timeline.
