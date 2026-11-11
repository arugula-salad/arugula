# Arugula control

Control is the hosted service at control.arugula.io and app.arugula.io:
accounts, devices, the relay that reaches your machines from anywhere,
teams, projects, chat, boards, issues, hosted workspaces and billing. Its
code is closed source and lives in its own repository; versions up to the
split are in this repository's history, under MIT OR Apache-2.0.

Using control (signing in, devices, joining machines, teams) is on
**[docs.arugula.io/control](https://docs.arugula.io/control/)**. What
crosses the line between a daemon and control (the wire, the native API) is
the published contract in [`contract/`](../contract/), and the end-to-end
encryption between your devices and your machines is in
[control-e2e.md](control-e2e.md). This page lists what the CLI and a
machine's agents do with control.

## Grants: what a machine's agents may do

A machine's agents act for its owner only through grants the owner gives,
each over an account or a project, and each revocable (a revoke refuses the
next call; work already under way carries on). Every joined machine holds
`cloud.drive` over its account, so its agents can drive the owner's Cloud
workspaces; the rest are given by hand:

```
arugula grants                                          # every machine's grants, live and revoked
arugula grants revoke geek                              # geek's agents can't drive your Cloud workspaces
arugula grants restore geek                             # they can again (a new grant)
arugula grants add geek threads.read threads.post --project acme
arugula grants add geek threads.read --dms
arugula grants add geek runs.read runs.start --project acme
arugula grants add geek runs.command --project acme
arugula grants add geek sessions.start sessions.reply --project acme
arugula grants add geek deploys.plan --project acme
arugula grants revoke geek threads.post --project acme
arugula grants ls --project acme                        # who can do what in acme
```

An agent a guest stands behind (one started in a session a guest edits) is
refused every tool that acts as the owner, by one check before any tool
runs (`crates/arugulad-mcp/src/guests.rs`).

## Projects and linked repos

```
arugula project ls                        # your personal project's linked repos
arugula project link acme/app             # --project <id or name> for another
arugula project unlink acme/app
```

A linked repo's PRs and issues read and act through control, as you:

```
arugula pr view acme/app#7                # also `issue view`
arugula pr list acme/app --state open     # or --project; also `issue list`
arugula pr checks acme/app#7
arugula pr review acme/app#7 approve      # sends as you; --draft keeps a draft
arugula pr comment acme/app#7 "lgtm"      # likewise `issue comment acme/app#2 ...`
arugula issue create acme/app -t "Title" -b "Body"
arugula draft ls                          # your waiting drafts; or OWNER/REPO[#N], --project
arugula draft show dft_…                  # discard, send
arugula forge inbox                       # what wants you; --all with what's done
arugula forge done acme/app#7             # seen likewise; --reason, --undo
```

## Hosted workspaces

A hosted workspace is `arugulad` in a rented sandbox (packaging/hosted,
`arugulad --hosted`), joined to a project; its box is built from the E2B
template of the daemon release it runs (release.yml's `template` job).

```
arugula cloud create dev --repo https://github.com/acme/app --setup setup.sh
arugula cloud ls                          # every project's; --project <id or name>
arugula cloud open dev --wait             # wakes a paused box, says its machine and its setup
arugula cloud edit dev --setup setup.sh   # rebuild needed
arugula cloud edit dev --finish "just check"   # no rebuild: the next turn runs it
arugula cloud edit dev --size large       # rebuild needed: 8 vCPU, 8 GiB
echo "$TOKEN" | arugula cloud secret set dev GIT_TOKEN
arugula cloud rebuild dev --download dev.tgz   # a new box from the spec
arugula cloud delete dev
arugula cloud key set anthropic --project acme   # reads the key (or token) from stdin
arugula cloud key ls --project acme
arugula cloud key clear anthropic --project acme
arugula cloud preview public dev 5173     # prints its public address
arugula cloud preview ls dev
arugula cloud preview renew dev 5173      # 7 more days
arugula cloud preview off dev 5173
```

Inside a box, `arugula-tools` puts tools where every session user finds
them (packaging/hosted/tools):

```
arugula-tools link zig "$(mise which zig)"    # a binary: a link in bin
arugula-tools rustup                          # rustup's proxies: wrappers that set RUSTUP_HOME
arugula-tools wrap NAME TARGET VAR=VALUE      # any tool that needs a variable set
arugula-tools env 'CARGO_TARGET_DIR=~/target' # every session user's environment
arugula-tools as ses1 cargo --version         # as a session sees it
```

## Runs

```
arugula runs spec add nightly --workspace dev --dir /work/app -- just e2e
arugula runs schedule add nightly --cron '0 3 * * *' --tz America/Chicago
arugula runs rule add nightly --on check-failed --branch main
arugula runs start --spec nightly            # by hand; or a command after --
arugula runs spec ls                         # with their schedules and rules
arugula runs list --state failed             # newest first
arugula runs show run_…                      # its timeline and artifacts
arugula runs logs run_… --follow             # its output, until it ends
```

## Boards

A board runs a project's tickets: rows grouped by phase, each row's lane
following its work (an agent session on the issue, a PR naming it), and a
goal that makes it an epic. Agents read and write them with the `board_read`
and `board_write` MCP tools; a PR that names a native issue (`Arugula #N`,
`Closes #N`) attaches to its row and closes the issue on merge.
