# CI optimization research (#1634)

Gathered 2026-10-28 for #1634, part of #1662. Numbers come from GitHub
Actions' API for arugula-salad/arugula unless a line says otherwise.

## 1. chant `build: newest` with bisect

Tried locally on branch `chant-build-newest-trial` (not pushed). The
behaviour below is from chant 0.124.54's own source
(`src/workspace/ci-builds.ts`, `ci-green.ts`, `ci-green-workflow.ts`):

- `build: every|newest` is per phase, read by chant 0.124.33 and newer.
  Under `newest`, a commit a newer run superseded counts as unbuilt, and so
  does a cancelled run (not red). The tick reruns a failed run once; a
  retry that hasn't started within an hour counts as the failure. When the
  tip is red, the tick tags `ci/build/<sha>` and dispatches the phase's
  workflow there.
- The change: `chant.workspace.json` (`minReader` 0.108.0 to 0.124.33,
  `"build": "newest"` on every phase), `chant-ci-green.yml` (the pin,
  `ci/build/**` in its workflow_run branches, `actions: write`), and
  `chant ci workflow` rewrites check.yml's and windows*.yml's concurrency to
  one group per ref with `cancel-in-progress: false`: one run at a time,
  the newest push waiting, the ones between dropped. 5 files, +40 −21.
- It has to be every phase, not only the heavy ones: a phase's setting
  covers the whole workflow it's in, lint and claims share check.yml, and
  lint's `suite-changed` is in windows.yml, so windows goes `newest` too
  (chant refuses a workflow that mixes them). The `macos*` pattern has to
  narrow to `macos (*)`, or `chant ci workflow` also rewrites release.yml's
  and app-release.yml's `macos` jobs to run on every push.

**ci/green and releases.** Green tags land only on commits where every
required phase ran; superseded commits get none. `just release` without a
commit still takes the newest green one; `just release <commit>` on a
superseded commit is refused (unless `ARUGULA_RELEASE_SKIP_GREEN=1`). So a
release can only start from a built commit, and after a burst the last
commit is built, so the newest merges are tested before a release.

**Bisect time.** Binary while more than four commits are in the range,
then a linear walk back from the oldest red one; one open request per
phase, each step waiting for its build. A step is a full run plus the
tick's latency (workflow_run, with the 15-minute cron as a backstop), and a
red step is retried once first. Main's check runs took 16 min median and
31 min at most over the last 60. A 10-commit burst: about two halvings and
up to four linear steps, so 1 to 1.5 h typically and 2.5 to 3 h at worst.
Requests expire after 6 h; a newer green tip stops the bisect.

**What has to change first.**

1. *(Blocking.)* check.yml's `changes` job, windows.yml's `suite-changed`
   and windows-desktop.yml's `app-changed` diff against
   `github.event.before`, the previous push. Under `newest`, a Rust commit
   superseded by a docs-only tip is never tested: the tip's run skips the
   suite, skipped counts as passed, and the tip goes green. They need to
   diff against the last built (or green) commit. (From reading the code;
   not seen in a run.)
2. The first real tick would rerun the 68 failed jobs a dry run
   (`ci tick --dry-run --forge github`) lists for the last 24 h.
3. `minReader` 0.124.33 makes 0.108 readers fail (`reader-too-old`),
   including `docs/development.md`'s `npx @intentius/chant@0.108.0 ci
   last-green`.
4. The tick writes `_ci/<sha>.json` records to a `chant/lifecycle` branch
   (new on the remote) unless `CHANT_NO_RECORD` is set.
5. Unverified: whether reruns of old main runs and a waiting tip build in
   the same `refs/heads/main` group cancel each other, and whether
   workflow_run's `ci/build/**` filter matches a dispatch at a tag (if not,
   bisect steps wait for the cron).

`just decisions` and the records are unaffected.

## 2. The runner pool

**What happened.** Runs 40596637218, 40596657675, 40596713504 (main) and
40596785864 (a branch) started between 22:40:46 and 22:41:59Z. From 22:43
to 22:52, 22 to 23 `arugula-amd64` pods ran, all heavy (build, static,
clippy, core, test, test-2: six per run). Every lost job's annotation is
"The self-hosted runner lost communication with the server", with its step
left `in_progress`; GitHub fails a job about 10 minutes after its runner
goes quiet:

| Failed by GitHub | Quiet from about | Jobs (run) |
|---|---|---|
| 23:02:17 | 22:52 | static, test (7675) |
| 23:03:28 | 22:53 | e2e (1) (7675) |
| 23:04:36 | 22:54 | core (3504) |
| 23:14:05 to 23:19:34 | 23:04 to 23:09 | test-2, test, static, clippy (7218); test-2, build, static (5864); build, test (3504); test-2, clippy (7675) |

Steps crawled before that (`just static` ran 30+ minutes; 2.5 in the 23:21
reruns, which all passed), two runners reported cancelling steps after
GitHub had failed their jobs, and jobs waiting only on `build` (done
22:49) didn't start until 23:12 with 12 to 16 pods up: pods were dying
between 23:04 and 23:12.

**The pool** (home-cloud `platform/arc/runner-scale-set-arugula-amd64.yaml`):
maxRunners 24, minRunners 0; the runner requests 1 CPU / 3Gi at the time
(2 CPU since home-cloud 7c2c5867, 2026-10-28 01:48Z), limit 16Gi, no CPU
limit; dind 0.5 CPU / 512Mi, limit 4Gi; PriorityClass `ci-runner` (-100,
evicted first); spread across nodes only since 7c2c5867. The amd64 nodes
leave about 55 GiB (geek) and 30 GiB (basil) for runners, about 85 GiB.

| | Heavy pods | Requested | Used (about 5 GB each) |
|---|---|---|---|
| 2026-10-27 peak | 23 | 80.5 Gi (fits) | about 115 GB |
| Four runs of today's check.yml | 28 | over 85 GiB | 140 to 195 GB |
| Two runs | 14 | | 70 to 98 GB |

**Cause** (confidence medium-high that it's overload; low on which of
eviction, the OOM killer or starvation): requests describe a typical job,
not a cargo build (3 to 7 GB, about six threads), so the scheduler packed
23 builds onto two nodes, about 130 rustc threads on 32-thread nodes and
more memory than they have. The low-priority pods went. maxRunners only
queues. kubectl here doesn't reach the home cluster, so no pod or node
events were read.

Lost jobs over two weeks: 49 in 515 runs. The other clusters: 2026-10-23
(seven test jobs, a pod's own OOM at the old 10Gi limit, raised to 16Gi
that day), 2026-10-24 (four), and 2026-10-28 02:08 (all seven jobs of
40618088185 at ten minutes, probably the rollout of home-cloud 7c2c5867).

**Proposal.** Four at once can't be sized for (140 to 195 GB against about
85 GiB); two is the most that fits.

- Ours: cap heavy work at two runs without dropping commits. A concurrency
  group can't (GitHub keeps one pending run per group and cancels the
  rest, what #524 removed); a cheap `gate` job on `ubuntu-latest` that the
  heavy jobs need can, waiting while two older main runs still have a
  heavy job running. `build: newest` (section 1) also does it, one run at
  a time per ref, if that lands first.
- Ours: docs/development.md says "2.5 CPUs and 5 GB … up to eight run at
  once"; the pool is 24 runners at 2.5 CPU / 3.5Gi, and `scripts/ci-slot`
  limits nothing across pods (each has its own filesystem).
- home-cloud's, as findings: a runner memory request of about 6Gi makes the
  scheduler stop near 13 to 14 heavy pods (85 / 6.5) and leave the rest
  Pending, not killed; or, with requests as they are, maxRunners about 14.
  Keep 7c2c5867's CPU request and spread. Disk isn't involved.

## 3. Full checks on risky PRs

Today a pull request runs only binaries.yml (`binaries`, and `quick`:
check.yml's lint, gitleaks and THIRD_PARTY.md) on GitHub's runners; the
home cloud's pods run check.yml only on main's pushes and by hand
(`gh workflow run check.yml --ref BRANCH`). #1616 broke `desktop-xvfb`, and
nothing ran it before the merge.

Proposal: a pull_request job in binaries.yml (GitHub's runners, so the
pool isn't touched) that runs `just desktop-xvfb` when the PR changes
`crates/desktop/**`, `packaging/desktop/**` or the xvfb scripts, and a
`full-check` label that dispatches check.yml on the PR's branch. #1661 did
the dispatch by hand (run 40632717590); a label makes it one click.
Every PR doesn't need the whole of check.yml: its e2e and test jobs are
where the pool's load is (section 2).

## 4. Hearing about red sooner

`ci-red` runs hourly and opens "main is red" only after
`ARUGULA_RED_HOURS` (6, the default; no repository variable is set) with
no green commit. On 2026-10-27 that was hours of red nobody saw.

Proposal: keep the 6 h for "nothing has gone green", and add a second
trigger: the first time a required phase fails on main's newest commit
after its one retry (the same rule chant 0.124's tick uses), comment on or
open the issue at once, naming the job and the commit. With `newest` and
bisect (section 1) that comment can name the bad commit once the bisect
finds it. Lowering the variable alone would also page for flakes that the
retry clears.

## 5. Release guard

`scripts/release bump` takes the newest green commit and only prints
"N newer commit(s) on main aren't green yet". On 2026-10-27 that would
have released #1615 without the two features the release was for.

Proposal: when it skips commits, list them (`%h %s`) and stop, unless
`--older` (go on from the green commit) or an explicit COMMIT is given.
`--dry-run` lists them too. Under `newest` (section 1) the list is shorter,
but not empty: merges since the last built tip.

## 6. Wall time

65 full runs on main (attempt 1 only). Era A: before 8455c81b (#259:
tests while static builds, Postgres in its own job, seven e2e shards).
Era B: from it to a25139c3, which put the e2e shards on GitHub's runners.
Era C: since.

| Era | Runs | Median | p90 | Green median / p90 |
|---|---|---|---|---|
| A | 44 | 16.9 | 38.7 | 14.2 / 19.3 |
| B | 9 | 38.6 | 44.2 | 25.8 (one green) |
| C | 12 | 15.5 | 19.9 | 12.9 / 15.0 |

Era B is a burst (seven merges in 17 minutes, 3 to 5 minute waits for a
pod) and Postgres with fsync on (test-postgres 23 to 28 minutes, 5.3 since
0fb9cef6); it says little about the new graph.

Era C, medians in minutes:

| Job | Starts | Takes | Ends |
|---|---|---|---|
| build | 0.6 | 3.7 | 4.3 |
| static | 0.8 | 5.7 | 6.5 |
| test-2 / test | 0.7 | 9.4 / 9.7 | 10.0 / 10.4 |
| desktop-xvfb | 6.7 | 4.1 | 10.7 |
| testnet | 7.2 | 2.9 | 11.5 |
| e2e, 7 shards | 4.3 | 6.5 to 7.6 | 11.2 to 12.3 |

The critical path is changes, build, the slowest e2e shard: about 12.3
minutes, so the median is already under 15; the p90 isn't. Then testnet
(11.5, through static) and test (10.4).

- test vs test-2: nextest's `count:N/2` deals tests out by index, and only
  part 1 runs the doctests (8 s) and control-smoke (44 s). Part 1 also drew
  the slower tests: 826 vs 511 test-seconds in the server workspace, 480
  vs 349 in the root. test runs about 1.3 minutes longer than test-2.
- testnet and desktop-xvfb do need static's musl binaries (the Docker
  boxes; Tauri's sidecars in a glibc 2.35 container, where build's 2.39
  binaries don't run). They could start on `changes` and wait with
  `scripts/ci-wait-job static`, as test does: about 0.75 minutes for
  testnet, 1.2 for desktop-xvfb (its Tauri build works against stand-in
  sidecars, as `just desktop-check`'s do).
- sccache hits 93 to 100% everywhere (core 70 to 100%). The rest of
  test-build's 3 minutes is linking bin, test and proc-macro crates.
- Slowest tests: `agent_screens::claude_code_working_blocked_and_idle_from_its_screen`
  27.8 s, platform `agent_mcp_tests::a_token_opens_no_other_route` 26.9 s,
  `agent_screens::quiet_changes_nothing_for_an_agent_with_rules` 25.6 s,
  `hosted_sessions::a_hosted_machine_runs_sessions` 16.2 s.

Estimates for the critical path: an eighth shard or refreshed
`durations.json` about 0.7 minutes; build's setup (1.4 minutes of caches,
which every shard waits on) down to 0.5, about 0.9; control-smoke and the
doctests moved to part 2 or a time-based partition, test down 0.8 to 1.3;
testnet and desktop-xvfb on `changes`, above. Together, a median critical
path of about 10.5 to 11 minutes, with room under 15 for the p90 as long as
pod waits stay near 0.5 minutes (section 2).

## 7. Flake

`reboot::a_restarted_box_brings_its_daemon_back_with_no_login_and_every_pane_by_policy`
was quarantined on 2026-10-27 by #1640 (`.config/quarantine.toml`, until
2026-11-10, with its tracking issue), with `ssh::…` and
`guest_ssh::…`. quarantine-nightly.yml runs them alone every night.

Other flakes red on main on 2026-10-27 and 28, filed while working #1662:
#1672 (workspace.spec attention timing), #1673 (control.spec Getting
started), #1674 (the journeys' allow-app step, three journeys). #1525's
cause was fixed by #1660; #1515's is in its own PR.

## Follow-ups

Labelled `release-health-10-28`, in the order they're worth doing:

| Issue | What | Section |
|---|---|---|
| #1675 | chant `build: newest` with bisect, after the changed-files jobs diff against the last built commit | 1 |
| #1676 | At most two heavy check runs at once (moot if #1675 lands first), the pool's docs, home-cloud's requests | 2 |
| #1678 | ci-red says so at once when a required job fails on main's tip | 4 |
| #1679 | `just release` lists the merges it would leave out, and stops | 5 |
| #1677 | desktop-xvfb on PRs that touch the app; a `full-check` label | 3 |
| #1680 | Rebalance test and test-2, testnet and desktop-xvfb on `changes`, shorter e2e shards | 6 |

#1675, #1676, #1677 and #1680 change workflows every merge or PR runs, so
each needs lex00's go-ahead before it lands.
