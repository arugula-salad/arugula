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

just testnet up tailnet        # headscale and two Tailscale nodes
just testnet measure tailnet   # S28: ssh against the tailnet path
testnet/selfupdate.sh OLD NEW  # #391: the daemon updates itself (two `just static` builds)
```

The Rust tests that drive Arugula against the stack,
`crates/arugulad/tests/integration/ssh.rs` (M51, box-bare and git),
`crates/arugulad/tests/integration/reboot.rs` (#26, box-systemd) and
bring their profile up when it isn't ready. Both recreate the boxes they use and
need `just static <arch>` for the box's binaries. `just test-run` brings
the stack up before the suite (`testnet/suite-up.sh`), so a cold image
build (minutes of apt-get on a CI pod's own Docker) isn't charged to the
first test's 5-minute deadline.

Up means ready, not started: the boxes and `git` have
healthchecks (`compose.yaml`'s `x-ready`: sshd's banner, box-systemd's boot
finished), so `up --wait`
returns when they answer. `up.sh` builds and makes the containers first,
with no deadline, then waits up to `ARUGULA_TESTNET_WAIT` seconds (180)
for them to be healthy; a test's recreate waits up to two minutes.

Docker is required: without it every script and test here fails, saying
so. Only `ARUGULA_SKIP_DOCKER=1` skips, and then each prints that it did
not run. CI never sets it. The claims expect fresh boxes: after installing anything on one,
`just testnet down` and `up` again (`bare` fails otherwise, as it should).

## Profiles

| Profile | Services | Status | For |
|---|---|---|---|
| `ssh` | `bastion`, `box-bare`, `box-systemd`, `git` | validated | S28, M51, M52's install step, #26 |
| `tailnet` | `headscale`, `ts-box`, `ts-client` | validated | S28's tailnet comparison |

Control's profile, and its claims, left with control for its own
repository, where its testnet/compose.yaml goes over this one: it uses this
stack's boxes, networks and `.state`, so change them with that in mind.
Forgejo and GitLab have a stack of their own in
[`forges/`](forges/README.md).

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
  The same repositories are cloned over HTTPS, `https://git/repo.git`
  (git-http-backend behind nginx, clones only), with basic auth: the user
  `x-access-token` and the password in `.state/git_https_password`, as a
  forge takes a token. Its certificate is `.state/git_https.crt`, self-signed
  for `git` (`GIT_SSL_CAINFO` on a box). `up.sh` makes all three.

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

## Conventions

- Containers, networks and the images are named `arugula-testnet*`; `down`
  removes those and `testnet/.state`, nothing else.
- `COMPOSE_PROJECT_NAME` renames a stack, so two can run side by side (one
  per worktree): `COMPOSE_PROJECT_NAME=illo-a2` gives containers and images
  `illo-a2-*`, networks `illo-a2` and `illo-a2-inner`, and state in
  `testnet/.state-illo-a2`. Give it its own `ARUGULA_TESTNET_SSH_PORT`
  and `ARUGULA_TESTNET_INNER_NET` (the inner network's subnet is fixed). The tests read the same variables.
- Host ports are off the defaults and each can be overridden with an
  `ARUGULA_TESTNET_*_PORT` variable.
- The scripts run on Linux and macOS (bash 3.2) and pass `shellcheck`.
- Healthy isn't connected. On a host that sends bridged traffic through
  iptables and has another program replacing the filter table (k3s's
  network policy controller, on geek), the rules Docker adds for a new
  network can be erased a moment later: its containers are each healthy
  and can't reach one another (#282). `up.sh` connects across each
  network from the bastion and, if one drops the connection, makes the
  stack's networks again once; then it fails saying so.
- CI brings up the `ssh` profile (rare-nightly.yml's testnet, and
  testnet-break.yml under `BREAK=1`). Both Linux and macOS run `ssh.rs`
  and `reboot.rs`, so both need Docker.

## Not yet

- Claims for the tailnet profile; its check is the measurement, which fails
  when either path doesn't reach the daemon.

The `ssh` profile's boxes never have Arugula built in: tests put it there
over ssh from the client (`just static`), which is what `bare` guards.

## A chant-fly plan's ledger across a box's users

`testnet/hosted/plan-ledger-users.sh` runs what arugulad does after a
chant-fly plan's step (#1985) on the hosted box's image, as its two users,
with no control stack: `user` (arugulad's) can't open the run's home, and
gets the bundle from `runner` through `crates/arugulad-work/src/run_file_read.sh`
(only a regular file of runner's own, not a link, under the cap); a
leftover of runner's that left its process group is found by `pgrep -u
runner` and ended by `sudo -n -u runner kill -KILL -- -1`; runner can't read
the environment of `user`'s process that holds the push's token; and git
pushes with the token from its environment only. It needs Docker, and
builds the image itself.
