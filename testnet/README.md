# testnet: a local stack of real services

Part A of #200: network shapes that one host's loopback can't give a test. A
box behind a bastion, a network where only ssh gets through, a box with no
Arugula on it. One Compose file, one profile per scenario, borrowing
INTENTIUS/terragucci's `stack/`.

```sh
just testnet up ssh            # build the image, make keys, start the boxes
just testnet test ssh          # every claim
just testnet test ssh jump     # one claim
just testnet break ssh         # each claim under BREAK=1; all must fail
just testnet break ssh jump    # one claim under BREAK=1
just testnet down              # remove containers, networks and .state
ssh -F testnet/.state/ssh_config box-bare   # or bastion, box-systemd

just testnet up control        # the ssh profile, plus arugula-control and its fakes
just testnet test ssh          # the ssh claims run on control's stack too
just testnet test control relay # M52 end to end
just testnet test control cli  # M49: the CLI logged in, reaching a direct and a relayed box
just testnet test control team # #254: the CLI reaching a team machine of another account
just testnet test control platform # M140: chat and Cloud through control's own site
just testnet test control platform-page # M151: the platform page on app.control.test
just testnet test control agents # M198: a box's agent reads and posts in a channel over MCP, granted
ARUGULA_TESTNET_PLATFORM=out just testnet up control  # the platform out of process (M140)
just testnet test control platform-restart  # out of process: a relayed session outlives a platform restart
just testnet test control issues-restart    # out of process: native issues and their threads outlive a platform restart
just testnet test control github-webhook    # out of process: the GitHub App's webhook lands on the platform (M161)
ARUGULA_TESTNET_PLATFORM=two just testnet up control  # two platform machines on Postgres (M144)
just testnet test control platform-fanout platform-handover  # chat and Cloud across the two
just testnet measure control   # M140: platform routes' latency, in process vs out
testnet/window.sh              # M141: the first window's runbook, rehearsed, both rollbacks too
testnet/window2.sh             # M145: SQLite to Postgres, two machines, the nightly dump and drill, rollbacks
just testnet up tailnet        # headscale and two Tailscale nodes
just testnet measure tailnet   # S28: ssh against the tailnet path
testnet/selfupdate.sh OLD NEW  # #391: the daemon updates itself (two `just static` builds)
```

The Rust tests that drive Arugula against the stack,
`crates/daemon/tests/integration/ssh.rs` (M51, box-bare and git) and
`crates/daemon/tests/integration/reboot.rs` (#26, box-systemd), bring the `ssh` profile
up when it isn't. Both recreate the boxes they use and need `just static
<arch>` for the box's binaries.

Docker is required: without it every script and test here fails, saying
so. Only `ARUGULA_SKIP_DOCKER=1` skips, and then each prints that it did
not run. CI never sets it. The claims expect fresh boxes: after installing anything on one,
`just testnet down` and `up` again (`bare` fails otherwise, as it should).

## Profiles

| Profile | Services | Status | For |
|---|---|---|---|
| `ssh` | `bastion`, `box-bare`, `box-systemd`, `git` | validated | S28, M51, M52's install step, #26 |
| `control` | `ssh`'s, and `control`, `fakes`, `postgres`; `platform` too with `ARUGULA_TESTNET_PLATFORM=out`, and `platform2` with `two` | validated | M52's join, the relay, a box with no way out, the platform out of process (M140), on Postgres (M143) and on two machines (M144) |
| `tailnet` | `headscale`, `ts-box`, `ts-client` | validated | S28's tailnet comparison |

The relay is part of `control`, and Forgejo and GitLab have a stack of
their own in [`forges/`](forges/README.md).

### `ssh`

- `bastion`: Debian with sshd only. It's the one thing published to the
  host, on `127.0.0.1:22922` (`ARUGULA_TESTNET_SSH_PORT`), and it forwards
  TCP for ProxyJump.
- `box-bare`: the same image with no Arugula and nothing set up for it,
  on an internal network with no route out. It's reached only through the
  bastion, so anything installed on it has to arrive over ssh.
- `box-systemd`: box-bare with systemd as PID 1, logind and polkit, also on
  the internal network. It runs privileged with its own cgroup namespace
  (Docker Desktop on macOS runs it too), for lingering and user services.
  Its journal is on disk, so `docker restart` keeps it (#26's reboot test).
- `git`: a git server on the internal network, bare repositories over ssh
  like a forge's: `git@git:/srv/git/repo.git`. The user `git` has
  `git-shell` and the stack's key. The boxes trust its host key, so a
  `git push` from a box needs only the client's agent forwarded (M51).

The boxes have one user, `illo`, who logs in with the stack's key only. Agent and
TCP forwarding are on. `up.sh` writes `testnet/.state/`: the client key,
a host key per box, `known_hosts`, and an `ssh_config` that reaches each box
by name with strict host key checking and `BatchMode`.

### `tailnet`

- `headscale`: a control server for the stack's own tailnet, plain http on
  the inner network. `up.sh` makes a user, `illo`, and a reusable,
  ephemeral preauthorized key (`tailnet-authkey` in the state directory)
  that both nodes join with.
- `ts-box`: a box with sshd and Tailscale in userspace mode, as a sandbox
  runs it: its netstack forwards tailnet connections to the daemon's port
  on loopback, and the daemon asks tailscaled who is connecting.
- `ts-client`: the client, with Tailscale on a real `tailscale0` interface
  (`NET_ADMIN` and `/dev/net/tun`), so programs on it dial the tailnet.

The nodes share a network and connect directly (`tailscale ping` says
"direct"). headscale requires a DERP map, so its embedded DERP server is on,
unused. `measure-tailnet.sh` installs Arugula on ts-box over ssh from
ts-client, then times the same requests over both paths (S28's numbers are
in `spikes/s28-ssh/README.md`). Tailscale SSH isn't here: its check mode
needs a login at an identity provider, which a test can't do.

### `control`

Everything in `ssh`, and:

- `control`: this tree's `arugula-control`, its relay included, from the
  static build (`just static aarch64` on Apple silicon, `just static` on
  x86_64; `just testnet up control` runs it), mounted rather than built
  into an image. It's on both networks. On the inner one it has a fixed
  address, `10.229.80.10` (`ARUGULA_TESTNET_INNER_NET` changes the first
  three octets), and that address is its public URL,
  `http://10.229.80.10:8080`: the boxes reach it by dialing out, as joined
  machines do, and a private address lets a daemon use plain http. The host
  reaches it on `127.0.0.1:22980` (`ARUGULA_TESTNET_CONTROL_PORT`).
  Its ssh jump host for guests (M65) is published on `127.0.0.1:22982`
  (`ARUGULA_TESTNET_GUEST_SSH_PORT`), the address invites name.
- `fakes`: `web/fixtures/fakes.ts` in Node, the same fakes `just
  control-smoke` uses: GitHub sign-in (published on `127.0.0.1:22981`,
  `ARUGULA_TESTNET_FAKES_PORT`, for the browser's redirect), Stripe and a
  Web Push endpoint. Only control talks to the last two.
- `postgres`: Postgres 17, as hosted control's Fly Managed Postgres (M143),
  reached only by control, its data in memory. Control's platform uses it
  when the stack comes up with `ARUGULA_TESTNET_POSTGRES=1` (`just testnet
  up control` with it set), and keeps to control's own file otherwise.

- `platform` (only with `ARUGULA_TESTNET_PLATFORM=out` at `up`): this
  tree's `arugula-platform` from the same static build, on the outer
  network with nothing published, and control started with
  `--platform http://platform:7700` as its gateway (M140). Control's
  internal API (`HubPort` over HTTP) listens on `:7691` in both ways, and
  the two share the stack's own `ARUGULA_PLATFORM_KEY` (in
  `compose.yaml`). The platform's database is in its container's own
  filesystem, so it outlives `docker restart`. Without the variable the
  platform runs inside control, as it always did: CI's run is that way.
- `platform2` (only with `ARUGULA_TESTNET_PLATFORM=two` at `up`, which adds
  [`two.yaml`](two.yaml) and puts both on `postgres`): a second
  `arugula-platform`, as hosted control runs two from M145 (M144). Control
  reaches both as `platforms`, a name Docker's DNS answers with both
  addresses, as Fly's `.internal` does. Each is published on the host
  (`127.0.0.1:22983` and `22984`, `ARUGULA_TESTNET_PLATFORM_PORT` and
  `ARUGULA_TESTNET_PLATFORM2_PORT`) only so the claims can call one or the
  other directly, with a principal signed with the stack's key. Cloud is
  on in both, with the fake Managoat (`fakes-serve.ts --managoat`, through
  a loopback sidecar each, as in `window.yaml`), whose control port is
  published too (`22985`, `ARUGULA_TESTNET_MANAGOAT_PORT`): the handover
  claim holds a request there and sees which machine sent it.

A person on the host is `web/fixtures/device-cli.ts`, the headless
approving device ([docs/testing.md](../docs/testing.md#a-device-that-approves-things)): it signs in, enrolls, approves join
codes and opens panes through the relay. `up.sh` writes
`.state/control.env` with control's URL and the `--via` mappings it needs.
The claims run the CLI from this tree (`cargo build -p arugula`, which
`just testnet test control` does) and need `node`.

## Claims

A claim checks one property of a running profile. `BREAK=1` breaks that
property, and the claim then has to fail; `just testnet break` checks that
every claim does, which shows each one can catch what it's about.
[docs/testing.md](../docs/testing.md#the-ssh-tracks-tests) says which
promise of the SSH track each claim and test guards, and what a failure
means.

| Claim | Checks | Broken by |
|---|---|---|
| `login` | the stack's key logs into the bastion, host key checked | a fresh key the boxes don't know |
| `jump` | box-bare answers through the bastion with ProxyJump | the bastion's `AllowTcpForwarding` off |
| `no-route` | box-bare has no default route | checking the bastion instead |
| `bare` | no Arugula on box-bare's login PATH, no state or config dir | a stub `/usr/local/bin/arugula` |
| `bytes` | 1 MiB of random bytes through `cat` on box-bare come back identical | a forced tty (`-tt`) |
| `agent` | a key in the client's agent shows on box-bare when forwarded, and `git push` from box-bare to `git` works with the key only in the forwarded agent | `ForwardAgent=no`, which must fail both |
| `linger` | on box-systemd, `loginctl enable-linger` works from an ssh login with no sudo | polkit masked |

The `control` profile's:

| Claim | Checks | Broken by |
|---|---|---|
| `signin` | a person signs in from the host with (the fake) GitHub; their first device is trusted on enrollment | GitHub down (`fakes` stopped) |
| `reach` | box-systemd, with no route out, reaches control at its inner address | control taken off the inner network |
| `relay` | M52 end to end: on a fresh box-systemd, `arugula --ssh box-systemd join` installs, starts the daemon and shows a code; the device approves it; the box is on the account's device list and online; with the ssh master closed and the bastion paused, a marker round-trips through a pane over the relay; a Pixel 7 (Chrome) and an iPhone (WebKit) signed in to control and approved by the device type a marker into the box's pane, read back by the device (`web/fixtures/m52-phones.ts`); after `docker restart` the box is back on the relay, and the same phones and the device reach the pane again | polkit masked, so no lingering: the daemon doesn't come back after the restart |
| `unreachable` | box-bare joining the hosted control (no route out) is told it can't reach control, with `arugula --ssh box-bare web` as the way in, and `--ssh` still works | joining the stack's control, which it can reach |
| `cli` | M49 end to end: box-systemd and box-bare join the stack's control (the device approves both); box-bare's daemon also listens on the inner network and lists `http://box-bare:7681`, box-systemd lists no URL. On the bastion (the CLI copied there, no daemon, so no `hosts.json`), `arugula login` shows a code and the device approves it; `arugula hosts` lists both from control, marked; `--host box-bare` (direct) and `--host box-systemd` (relayed) each `run`, `ls` and `capture`; then on box-systemd `events --follow` and `tail --follow` print while running, and `attach`, in a pty from `ssh -tt`, types a command, see its answer and leave with Ctrl-] | the CLI isn't logged in: neither name resolves |
| `team` | another account's machine (#254): an owner (a second headless device) makes a team and joins box-systemd to it; the CLI's account asks to join and is let in; `arugula hosts` on the bastion lists box-systemd as the owner's, and `--host box-systemd` captures its pane and attaches to it as a team editor, relayed | the CLI pinned a different root for the owner's account first, so control's chain for it is refused |
| `platform` | chat and Cloud through control's own site, whichever process answers (M140): a person turns chat on, makes a team and a channel, posts, reads it back and hears its event on the live stream; the platform answers Cloud's and billing's routes (`testnet/platform.ts chat`) | the maintenance switch on (`arugula-control maintenance on`), so every platform route answers 503 |
| `platform-page` | the platform page on its own host (M151), `app.control.test` through control's port, in process or out: page paths are its `index.html` and its script its own, under its CSP with no cookie set; `/api/projects` answers there (401 until M152) and the hub's routes and `/control.json` are 404s; control's own site still serves the shell under its own CSP, and its `/control.json` says `platform` | asking for `app2.control.test`, a host control wasn't given: the shell answers |
| `platform-restart` | only out of process: a relayed session to box-systemd's pane stays open while the `platform` container restarts, a marker round-tripping on the same socket before and after (`testnet/platform.ts hold`) | control's container restarted instead, which holds the relay |
| `github-webhook` | only out of process (M161): a GitHub webhook signed with the App's secret, posted to control's site, lands on the platform (202, its delivery in the platform's log), is a duplicate when sent again, and still one after the `platform` container restarts | signed with another secret, so it's refused |
| `issues-restart` | only out of process (M206): two native issues (one closed) and comments in the thread on one, written through control's site, read back the same after the `platform` container restarts (issues, timelines, thread, read mark); the next issue and comment are numbered on with no gap, and search finds both (`testnet/platform.ts issues-write`, `issues-read`) | the `platform` container recreated instead, so its database file is new |
| `platform-fanout` | only with two machines (M144): both answer their health check on the database and exactly one leads; a live stream held on each hears a message posted through the other; a page that dropped from one resumes on the other from its `Last-Event-ID`, without a resync (`testnet/platform.ts fanout`) | `platform2` on a file of its own (`ARUGULA_TESTNET_PLATFORM2_DB` empty): two machines that share no database |
| `platform-handover` | only with two machines: the fake Managoat holds a Cloud workspace's read, so the machine that claimed its job holds it mid-step; that machine is killed; the other takes the job once its lease (120 s) runs out, not before and within seconds after, and sets the workspace up; the killed one is started again (`testnet/platform.ts handover`) | the other machine paused too, so nothing takes the job |

`just testnet measure control [samples]` (`measure-control.sh`) times a
few routes from the host as a signed-in person (`GET /api/me` for the
hub's floor, then `GET /api/chat`, a room's messages,
`GET /api/cloud/workspaces` and a `POST` marking a room read), 300 times
each after a warm-up, in whichever way the stack runs the platform. Each
run is kept in `testnet/.measure-<stack>/` (which `down` leaves); with
both ways measured it compares their p95s and fails over M140's 20 ms
gate.

## M141's window, rehearsed

`testnet/window.sh` runs the first window's runbook (docs/control-ops.md,
"The first window") against the `control` profile, with `window.yaml`
added to compose.yaml: control and the platform keep their databases in
the state directory (bind mounts, as the host's user, so they outlive the
recreations that stand in for Fly secret changes), Cloud is on in both
with a fake Managoat (`fakes-serve.ts --managoat`, reached through a
loopback sidecar since Cloud takes plain http only on loopback), and the
platform starts held (`ARUGULA_PLATFORM_PAUSED=1`). In order:

1. a person with two channels of messages, a read mark, a cloud secret, a
   Cloud workspace that names it and a tab with a turn (`window.ts seed`);
2. the notice (`arugula-control notice post`), as the page gets it;
3. a trial copy carried to the platform, whose secrets open there;
4. the window: maintenance on with `--pause`, `platform copy` on control,
   `docker cp` for `fly ssh sftp`, `copy` on the platform, the three
   checksums equal (both copies and control's tables after), the secrets
   open; control flipped to the platform, the platform let go,
   maintenance off;
5. post-checks (`window.ts check`, the `platform` claim);
6. the rollback within the window: the platform held, control back in
   process on its own copies, unchanged by the time out of process;
7. the window again, over the platform's earlier copy (`--replace`), then
   new work: a message, a turn, and a workspace whose secret the platform
   decrypted for Managoat, the value seeded before the window
   (`fakes-serve.ts` tells its SHA-256 on 9005);
8. the rollback after the window: the platform's file copied back over
   control's copies, control in process, the new work there.

It stops at the first thing that doesn't hold. The transcript is in
`.state/window/transcript.txt`; the stack stays up for a look, and
`just testnet down` removes it (and the state directory with the
databases). With another stack running, give it its own
`COMPOSE_PROJECT_NAME` and ports (Conventions, below).

## M145's window, rehearsed

`testnet/window2.sh` runs the second window's runbook (docs/control-ops.md,
"The second window (M145)") with `window.yaml` and `window2.yaml` added to
compose.yaml. It starts where production is after M141: control as the
gateway, the platform out of process on its own file. The platform's
database is a variable (`ARUGULA_TESTNET_PLATFORM_DATABASE_URL`: empty for
the file, the `postgres` service's URL after the flip), so recreating the
container stands in for setting a Fly secret. `platform2` is the second
machine, and both answer to `platforms`, the stand-in for Flycast. `s3`
(rclone's S3 server) stands in for R2, and `dump` is the nightly dump's
image (packaging/platform-dump) around this tree's static binary. In
order:

1. a person's chat and Cloud, written through the gateway (`window.ts seed`);
2. the notice;
3. pre-checks: `migrations` on the file, a timed trial copy into Postgres
   (the URL handed over as `@/tmp/pg.url`, as `fly ssh sftp put` will),
   and its secrets open;
4. the window: maintenance on; the platform paused; portable checksums of
   the file (`before.txt`); `copy --replace` into Postgres; Postgres's
   checksums and the file's both equal `before.txt`; secrets open. Then
   the flip onto Postgres (still paused), a rollback to the untouched file,
   the flip again, the unpause, and maintenance off;
5. post-checks, then new work on Postgres, including a secret decrypted
   for Managoat;
6. two machines: `platform2` up, exactly one leading, and the hub switched
   to `platforms`. Both serve: a stream and posts through the gateway, the
   `platform-fanout` claim, and each machine stopped in turn while the
   gateway keeps answering;
7. the first nightly dump (`docker compose run dump`) and the restore
   drill (`scripts/platform-restore-drill`) against it. A record that lies
   about one table must fail the drill, and so must a dump with the wrong
   database password. Then the job's own retention: old dumps seeded at 35,
   40, 45 and 60 days, and the next run keeps the newest 3 and deletes the
   rest older than 30 days; a run whose listing is refused (a `curl` in
   front of the real one) still exits 0 with its dump in;
8. rollback after people wrote on Postgres: one machine, the hub back on
   `platform`, Postgres copied back over the file (checked against a
   second copy), the platform back on its file, and the new work there.

As with `window.sh`, it stops at the first thing that doesn't hold. The
transcript is in `.state/window/transcript.txt`. Run it with its own
`COMPOSE_PROJECT_NAME` and ports, adding `ARUGULA_TESTNET_PLATFORM_PORT`,
`ARUGULA_TESTNET_PLATFORM2_PORT`, `ARUGULA_TESTNET_MANAGOAT_PORT` and
`ARUGULA_TESTNET_S3_PORT`, and run `just testnet down` afterwards.

## Conventions

- Containers, networks and the images are named `arugula-testnet*`; `down`
  removes those and `testnet/.state`, nothing else.
- `COMPOSE_PROJECT_NAME` renames a stack, so two can run side by side (one
  per worktree): `COMPOSE_PROJECT_NAME=illo-a2` gives containers and images
  `illo-a2-*`, networks `illo-a2` and `illo-a2-inner`, and state in
  `testnet/.state-illo-a2`. Give it its own `ARUGULA_TESTNET_SSH_PORT`
  and `ARUGULA_TESTNET_INNER_NET` (the inner network's subnet is fixed),
  and for `control` its own `ARUGULA_TESTNET_CONTROL_PORT`,
  `ARUGULA_TESTNET_FAKES_PORT` and `ARUGULA_TESTNET_GUEST_SSH_PORT`. The tests read the same variables.
- Host ports are off the defaults and each can be overridden with an
  `ARUGULA_TESTNET_*_PORT` variable.
- The scripts run on Linux and macOS (bash 3.2) and pass `shellcheck`.
- CI (`.github/workflows/check.yml`) brings up the `control` profile once
  on the Linux runner and runs `test ssh`, `test control`, `break ssh` and
  `break control` on it: control's stack has all of ssh's boxes, and the
  control claims that recreate box-bare leave it bare. Both runners run
  `ssh.rs` and `reboot.rs`, so both need Docker.

## Not yet

- Claims for the tailnet profile; its check is the measurement, which fails
  when either path doesn't reach the daemon.

The `ssh` profile's boxes never have Arugula built in: tests put it there
over ssh from the client (`just static`), which is what `bare` guards.
