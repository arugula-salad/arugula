#!/usr/bin/env bash
#
# Run claims against a running profile of the test stack.
#
#   testnet/test.sh ssh                 every claim of the ssh profile
#   testnet/test.sh ssh jump bytes      just those
#   BREAK=1 testnet/test.sh ssh jump    must fail
#   testnet/test.sh ssh --break         each claim under BREAK=1; passes only
#                                       if every one of them fails
#   testnet/test.sh ssh --break jump    just those, under BREAK=1
#
# The control profile's stack has every box the ssh profile has, so the ssh
# claims run on it too: CI brings up control once and runs `ssh`, then
# `control`. The control claims that recreate box-bare leave it bare again.
#
# The ssh profile's claims, and how BREAK=1 breaks each one:
#
#   login  The stack's key logs into the bastion, with strict host key
#          checking. Broken: a fresh key the boxes have never seen.
#   jump   box-bare is reached through the bastion with ProxyJump.
#          Broken: the bastion's AllowTcpForwarding is turned off (and back
#          on afterwards).
#   no-route
#          box-bare has no route out; only ssh through the bastion reaches
#          it. Broken: the check runs on the bastion, which has one.
#   bare   box-bare has no arugula: no binary on the login PATH, no state
#          or config dir. Broken: a stub arugula is put in /usr/local/bin
#          (and removed afterwards).
#   bytes  ssh carries a binary stream both ways unchanged: 1 MiB of random
#          bytes through `cat` on box-bare comes back identical. This is
#          what S28's bridge relies on. Broken: a forced tty (-tt).
#   agent  A key in the client's agent shows on box-bare when the agent is
#          forwarded through the bastion, and `git push` from box-bare to
#          the git server (a bare repository over ssh) works with the key
#          only in that agent (M51). Broken: ForwardAgent=no, which fails
#          both.
#   linger On box-systemd, a user turns on lingering for themselves from an
#          ssh login with no sudo (S28's lifetime question). Broken: polkit
#          masked (and unmasked afterwards).
#
# The control profile's claims (they need node, the web client's packages
# and Playwright's browsers (the relay claim's phones), and the CLI built:
# `cargo build -p arugula`, or ARUGULA_CLI):
#
#   signin  A person signs in to control with (the fake) GitHub, from the
#           host, and their first device is trusted on enrollment
#           (web/fixtures/device.ts). Broken: GitHub is down (the fakes
#           stopped, and started afterwards).
#   reach   box-systemd, on the inner network with no route out, reaches
#           control at its address there. Broken: control taken off the
#           inner network (and put back).
#   relay   M52 end to end: on a fresh box-systemd, `arugula --ssh
#           box-systemd join` installs Arugula and starts its daemon, and
#           its code is approved by a headless device; the box is then on
#           the account's device list and online, and with the ssh master
#           closed and the bastion paused, a marker round-trips through a
#           pane over control's relay. A Pixel 7 (Chrome) and an iPhone
#           (WebKit) signed in to control, approved by the device, open the
#           box's pane and type a marker, which the device reads back from
#           the box over the relay. After `docker restart` it comes back to
#           the relay by itself, and the same phones and the device reach
#           the pane again. Broken:
#           polkit masked on the box, so lingering can't be turned on and
#           the daemon doesn't start again after the restart.
#   unreachable
#           A box that can't reach control (box-bare joining the hosted
#           control, with no route out) says so, naming the box, control
#           and `arugula --ssh box web`, and is still reachable over
#           --ssh. Broken: it joins the stack's control, which it can reach.
#   cli     M49 end to end: box-systemd and box-bare join the stack's
#           control (approved by the headless device); box-bare's daemon
#           also listens on the inner network and lists that URL, and
#           box-systemd lists none. On the bastion, which has the CLI and no
#           daemon (so no hosts.json), `arugula login` shows a code the
#           device approves; then `arugula hosts` lists both machines
#           from control, and `--host box-bare` (direct) and `--host
#           box-systemd` (relayed) each `run`, `ls` and `capture`. Then on
#           the relayed box: `events --follow` and `tail --follow` print
#           while they run, and `attach` (in a pty from `ssh -tt`)
#           types into a pane, see the answer, and leave with Ctrl-]. Broken:
#           the CLI isn't logged in, so neither name resolves.
#   team    Another person's machine in a team (#254): an owner signs in,
#           makes a team and joins box-systemd to it; a second person, the
#           CLI's account, asks to join and is let in. `arugula hosts` on
#           the bastion lists box-systemd as the owner's, and `--host
#           box-systemd` captures its pane and attaches to it, typing as a
#           team editor (relayed). Broken: the
#           CLI already pinned a different root for the owner's account
#           (control "changed" it), so the machine is neither listed nor
#           reached.
#   platform
#           Chat and Cloud through control's own site (M140), whichever
#           process answers (ARUGULA_TESTNET_PLATFORM=in or out at `up`): a
#           person turns chat on, makes a team and a channel, posts, reads
#           the message back and hears it on the live stream; the platform
#           answers Cloud's and billing's routes. Broken: the maintenance
#           switch on (`arugula-control maintenance on`, and off
#           afterwards), so every platform route answers 503.
#   platform-page
#           The platform page on its own host (M151), through control's
#           port, whichever process the platform is in: any page path is
#           its index.html and its script its own, under the page's CSP;
#           a platform route answers there (401: nobody's signed in there
#           until M152) and the hub's routes and /control.json are 404s,
#           with no cookie set; control's own site still serves the shell
#           under its own CSP, and its /control.json says where the
#           platform page is. Broken: asking a host control wasn't given
#           (app2.control.test), which gets the shell.
#   platform-restart
#           Only with the platform out of process: a relayed session to a
#           box's pane stays open while the platform's container restarts,
#           a marker round-tripping on the same socket before and after.
#           Broken: control's container restarts instead, which holds the
#           relay.
#   github-webhook
#           Only with the platform out of process (M161): a GitHub webhook
#           signed with the App's secret, posted to control's own site,
#           lands on the platform (202, and the platform logs its delivery);
#           sent again it's a duplicate, and still one after the platform's
#           container restarts (the delivery is in its database, not its
#           memory). Broken: signed with another secret, so it's refused.
#   issues-restart
#           Only with the platform out of process (M206): native issues, their
#           timelines and the thread on one, written through control's own
#           site, read back the same after the platform's container
#           restarts; the next issue and comment are numbered on with no
#           gap, and search finds both. Broken: the container is recreated
#           instead, so its database file is new.
#   runs    Only with the platform out of process on Postgres (M212): runs
#           on a hosted workspace (the real `arugulad --hosted` in
#           hosted-box, enrolled by ticket, under the project's standing
#           grant, started by hand). A run executes once, as `runner`, in
#           its own copy of a repo in /home/user (left as it was), across a
#           platform restart, with events signed by the box; its pane,
#           watched end to end through control's relay, says what it ran;
#           what it left running is killed. `runner` can't read the
#           daemon's key or reach its port (`user` can). Sent again after the
#           box's daemon restarts, it isn't run again. An agent run is
#           Claude Code (a stand-in), headless, its login on stdin. A revoke
#           saying `running stop` stops a run and all of `runner`'s
#           processes. Broken: the box's ledger is wiped first, so it runs
#           twice.
#   platform-fanout
#           Only with two platform machines (ARUGULA_TESTNET_PLATFORM=two,
#           M144): both answer their health check on the database and
#           exactly one leads; a live stream held on each hears a message
#           posted through the other (each called directly, with a principal
#           signed with the stack's key); a page that dropped from one
#           resumes on the other from its Last-Event-ID. Broken: platform2
#           on a file of its own (ARUGULA_TESTNET_PLATFORM2_DB empty), two
#           machines that share no database.
#   platform-handover
#           Only with two platform machines: a Cloud workspace's job is
#           held mid-step by the machine that claimed it (the fake Managoat
#           holds its read), that machine is killed, and the other takes the
#           job once its lease runs out (not before, and within seconds
#           after), and sets the workspace up. Broken: the other machine is
#           paused too, so nothing takes it.
#   hosted-setup
#           Only with ARUGULA_TESTNET_HOSTED=1 at `up` (M183, hosted.yaml): a
#           team's owner delegates its project and, from the CLI on the
#           bastion, makes a hosted workspace with two repos on the git
#           server (over HTTPS) and a setup script, and sets its GIT_TOKEN;
#           the platform makes its box at the fake E2B, and the real
#           `arugulad --hosted` on box-systemd enrolls with the ticket in
#           that box's env. Its setup runs in a pane (session `setup`) the
#           CLI captures, both repos are cloned and the script ran in the
#           first, and `cloud open --wait` ends set up; a spec edit shows
#           "rebuild needed"; work on the box that isn't pushed stops a
#           rebuild, and `--download` saves it, then rebuilds (a new box at
#           the fake, the old one deleted). Broken: the wrong GIT_TOKEN, so
#           the clone fails and the setup says so.
#   agents  Agents in conversations (M198), signed by a joined box's daemon:
#           box-systemd joins a person's account; they give it threads.read
#           and threads.post over their team's project and @mention it in a
#           channel. `arugula mcp` on the box (over ssh, as Claude runs it)
#           finds the mention with read_thread, answers with post_thread,
#           and its capabilities say thread tools are live; the person
#           reads the answer labelled as their agent on box-systemd; after
#           the read grant is revoked, read_thread is refused. Broken: no
#           grant given, so the box's first read is refused.
#
# Needs `testnet/up.sh <profile>` first. Exit codes: 0 every claim held, 1 a
# claim failed or there's no Docker (ARUGULA_SKIP_DOCKER=1 makes that a
# loud skip with exit 0: nothing runs), 2 usage.
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROFILE="${1:-ssh}"; shift || true
# shellcheck source-path=SCRIPTDIR source=env.sh
. "$HERE/env.sh"
CFG="$STATE/ssh_config"
BREAK="${BREAK:-}"
SSH_CLAIMS="login jump no-route bare bytes agent linger"
CONTROL_CLAIMS="signin reach relay unreachable cli team hosted platform platform-page agents"

need_docker

case "$PROFILE" in
  ssh) CLAIMS="$SSH_CLAIMS" ;;
  control) CLAIMS="$CONTROL_CLAIMS" ;;
  *) echo "usage: testnet/test.sh ssh|control [--break | claim...]   (ssh: $SSH_CLAIMS; control: $CONTROL_CLAIMS)" >&2; exit 2 ;;
esac
[ -f "$CFG" ] || { echo "no $CFG; run 'just testnet up $PROFILE' first" >&2; exit 1; }
if [ "$PROFILE" = control ]; then
  [ -f "$STATE/control.env" ] || { echo "no $STATE/control.env; run 'just testnet up control' first" >&2; exit 1; }
  # shellcheck source=/dev/null
  . "$STATE/control.env"
  # A platform of its own to restart, only out of process; two of them to
  # share chat and Cloud (M144).
  case "${ARUGULA_TESTNET_PLATFORM:-in}" in
    out) CONTROL_CLAIMS="$CONTROL_CLAIMS platform-restart github-webhook issues-restart runs" ;;
    two) CONTROL_CLAIMS="$CONTROL_CLAIMS platform-restart platform-fanout platform-handover" ;;
  esac
  # Hosted workspaces at the fake E2B (M183).
  [ "${ARUGULA_TESTNET_HOSTED:-}" != 1 ] || CONTROL_CLAIMS="$CONTROL_CLAIMS hosted-setup"
  [ "$PROFILE" != control ] || CLAIMS="$CONTROL_CLAIMS"
fi

if [ "${1:-}" = --break ]; then
  shift
  held=""
  claims="${*:-$CLAIMS}"
  for c in $claims; do
    case " $CLAIMS " in *" $c "*) ;; *) echo "unknown claim '$c' (claims: $CLAIMS)" >&2; exit 2 ;; esac
    if BREAK=1 "$0" "$PROFILE" "$c" >/dev/null 2>&1; then held="$held $c"; else echo "[testnet $PROFILE $c] caught under BREAK=1"; fi
  done
  [ -z "$held" ] || { echo "FAIL: these claims held under BREAK=1, so they can't catch what they check:$held" >&2; exit 1; }
  exit 0
fi

claims="${*:-$CLAIMS}"
WORK="$(mktemp -d)"
# The CLI's ssh masters, in a short directory (a socket path is at most
# 104 bytes on macOS).
RT="/tmp/ilg-$TESTNET-$$"
cleanup() {
  [ -z "${OUR_AGENT:-}" ] || kill "$OUR_AGENT" 2>/dev/null || true
  [ -z "${PAUSED:-}" ] || docker unpause "$TESTNET-bastion" >/dev/null 2>&1 || true
  if [ -d "$RT" ]; then
    for b in box-bare box-systemd; do ssh -F "$CFG" -o ControlPath="$RT/arugula-ssh/%C" -O exit "$b" >/dev/null 2>&1 || true; done
    rm -rf "$RT"
  fi
  rm -rf "$WORK"
}
trap cleanup EXIT

s() { ssh -F "$CFG" "$@"; }
sha() { if command -v sha256sum >/dev/null; then sha256sum | cut -d' ' -f1; else shasum -a 256 | cut -d' ' -f1; fi; }

claim_login() {
  # IdentityFile on the command line adds to the config's rather than
  # replacing it, so the broken run gets a config naming only the stranger.
  local cfg="$CFG"
  if [ -n "$BREAK" ]; then
    ssh-keygen -q -t ed25519 -N '' -f "$WORK/stranger"
    sed "s|IdentityFile .*|IdentityFile \"$WORK/stranger\"|" "$CFG" > "$WORK/ssh_config"
    cfg="$WORK/ssh_config"
  fi
  [ "$(ssh -F "$cfg" bastion hostname)" = bastion ]
}

claim_jump() {
  if [ -n "$BREAK" ]; then
    docker exec "$TESTNET-bastion" sed -i 's/^AllowTcpForwarding yes/AllowTcpForwarding no/' /etc/ssh/sshd_config
    docker exec "$TESTNET-bastion" kill -HUP 1
    sleep 1
    local ok=0; [ "$(s box-bare hostname 2>/dev/null)" = box-bare ] && ok=1
    docker exec "$TESTNET-bastion" sed -i 's/^AllowTcpForwarding no/AllowTcpForwarding yes/' /etc/ssh/sshd_config
    docker exec "$TESTNET-bastion" kill -HUP 1
    sleep 1
    [ "$ok" = 1 ]
  else
    [ "$(s box-bare hostname)" = box-bare ]
  fi
}

claim_no_route() {
  # A default route shows in /proc/net/route as destination 00000000.
  local box=box-bare; [ -n "$BREAK" ] && box=bastion
  ! s "$box" "awk 'NR > 1 && \$2 == \"00000000\"' /proc/net/route | grep -q ."
}

claim_bare() {
  if [ -n "$BREAK" ]; then
    docker exec "$TESTNET-box-bare" sh -c 'printf "#!/bin/sh\n" > /usr/local/bin/arugula && chmod +x /usr/local/bin/arugula'
  fi
  local rc=0
  s box-bare 'bash -lc "! command -v arugula && ! command -v arugulad && [ ! -e ~/.local/state/arugula ] && [ ! -e ~/.config/arugula ]"' >/dev/null || rc=1
  [ -z "$BREAK" ] || docker exec "$TESTNET-box-bare" rm -f /usr/local/bin/arugula
  return "$rc"
}

claim_bytes() {
  local tty="-T"; [ -n "$BREAK" ] && tty="-tt"
  head -c 1048576 /dev/urandom > "$WORK/sent"
  # On a forced tty ssh reads its escapes from the random bytes too, and
  # a newline, ~, ^Z suspended ssh itself with nothing to resume it: the
  # broken run hung for good (three of ten check runs, 2026-10-14). -e none
  # turns the escapes off; the timeout (GNU's sends SIGCONT after SIGTERM)
  # is for whatever else stalls it. 1 MiB takes about a second.
  timeout 60 ssh -F "$CFG" -e none "$tty" box-bare cat < "$WORK/sent" > "$WORK/back" 2>/dev/null || true
  [ "$(sha < "$WORK/sent")" = "$(sha < "$WORK/back")" ]
}

# An agent of our own holding the stack's key, started once.
use_agent() {
  [ -n "${OUR_AGENT:-}" ] && return 0
  eval "$(ssh-agent -s)" >/dev/null
  OUR_AGENT="$SSH_AGENT_PID"
  ssh-add -q "$STATE/id_ed25519"
}

# The forwarded agent: its key shows on the box, and a push from the box
# works with the key only there. One break fails both, so they're one
# claim. Both checks always run: under BREAK=1 the claim holds (isn't
# caught) if either still works, so each one is shown to need the agent.
claim_agent() {
  use_agent
  local want fwd="-o ForwardAgent=yes" branch="claim-$$-$RANDOM" key=0 push=0
  want="$(ssh-keygen -lf "$STATE/id_ed25519.pub" | cut -d' ' -f2)"
  [ -n "$BREAK" ] && fwd="-o ForwardAgent=no"
  # shellcheck disable=SC2086
  s $fwd box-bare ssh-add -l 2>/dev/null | grep -qF "$want" && key=1
  # shellcheck disable=SC2086,SC2016 # options split on purpose; $(...) runs on the box
  s $fwd box-bare 'cd "$(mktemp -d)" && git init -q && git -c user.name=illo -c user.email=illo@box-bare commit -q --allow-empty -m claim && git push -q git@git:/srv/git/repo.git HEAD:refs/heads/'"$branch" 2>/dev/null &&
    docker exec "$TESTNET-git" git --git-dir=/srv/git/repo.git rev-parse -q --verify "refs/heads/$branch" >/dev/null && push=1
  if [ -n "$BREAK" ]; then
    [ "$key" = 1 ] || [ "$push" = 1 ]
  else
    [ "$key" = 1 ] || note "agent: the key isn't in box-bare's agent"
    [ "$push" = 1 ] || note "agent: git push from box-bare didn't reach the git server"
    [ "$key" = 1 ] && [ "$push" = 1 ]
  fi
}

claim_linger() {
  local b="$TESTNET-box-systemd" rc=0
  docker exec "$b" loginctl disable-linger illo
  if [ -n "$BREAK" ]; then docker exec "$b" systemctl mask --now polkit.service >/dev/null 2>&1; fi
  # shellcheck disable=SC2016 # expanded on the box
  s box-systemd 'loginctl enable-linger 2>/dev/null && [ "$(loginctl show-user "$USER" -p Linger --value)" = yes ]' || rc=1
  if [ -n "$BREAK" ]; then docker exec "$b" sh -c 'systemctl unmask polkit.service && systemctl start polkit.service' >/dev/null 2>&1; fi
  return "$rc"
}

# The control profile's helpers.
ROOT="$(cd "$HERE/.." && pwd)"
CLI="${ARUGULA_CLI:-${CARGO_TARGET_DIR:-$ROOT/target}/debug/arugula}"
note() { echo "[testnet $PROFILE] $*" >&2; }
# The headless approving device (web/fixtures/device-cli.ts), one per run;
# `devs FILE ...` is another person's, kept in FILE.
devs() { local f="$1"; shift; node --experimental-strip-types --no-warnings "$ROOT/web/fixtures/device-cli.ts" --state "$f" "$@"; }
dev() { devs "$WORK/device.json" "$@"; }
# shellcheck disable=SC2086 # CONTROL_VIA is several words
signin() { dev signin --control "$CONTROL_URL" $CONTROL_VIA --login "$1"; }
field() { sed -n "s/.*\"$1\":\"\([^\"]*\)\".*/\1/p"; }
# The CLI on the host, as a person runs it, with the stack's ssh config.
cli() {
  mkdir -p "$RT"
  ARUGULA_SSH="ssh -F $CFG" ARUGULA_SSH_BINARIES="$ARUGULA_TESTNET_BINARIES" ARUGULA_SSH_INSTALL=yes \
    XDG_RUNTIME_DIR="$RT" "$CLI" "$@"
}
# A box as new: recreated, and answering ssh.
fresh() {
  docker compose -f "$HERE/compose.yaml" --profile ssh up -d --force-recreate --wait "$1" >/dev/null 2>&1 || return 1
  for _ in $(seq 1 40); do s "$1" true 2>/dev/null && return 0; sleep 0.5; done
  return 1
}

claim_signin() {
  local rc=0
  if [ -n "$BREAK" ]; then docker stop "$TESTNET-fakes" >/dev/null; fi
  signin "signin-$$" > "$WORK/signin.json" 2>/dev/null || rc=1
  grep -q '"approved":true' "$WORK/signin.json" || rc=1
  if [ -n "$BREAK" ]; then docker start "$TESTNET-fakes" >/dev/null; sleep 1; fi
  return "$rc"
}

claim_reach() {
  local ip="${CONTROL_URL#http://}" rc=0
  ip="${ip%%:*}"
  if [ -n "$BREAK" ]; then docker network disconnect "$TESTNET-inner" "$TESTNET-control"; fi
  s box-systemd bash -s > "$WORK/reach" 2>/dev/null <<SH || rc=1
exec 3<>/dev/tcp/$ip/8080
printf 'GET /control.json HTTP/1.0\r\n\r\n' >&3
head -1 <&3
SH
  grep -q ' 200 ' "$WORK/reach" || rc=1
  if [ -n "$BREAK" ]; then docker network connect --ip "$ip" "$TESTNET-inner" "$TESTNET-control"; fi
  return "$rc"
}

claim_relay() {
  local box=box-systemd fp code="" pid started ok=0
  fresh "$box" || { note "relay: couldn't recreate $box"; return 1; }
  if [ -n "$BREAK" ]; then docker exec "$TESTNET-$box" systemctl mask --now polkit.service >/dev/null 2>&1; fi
  fp="$(signin "relay-$$-$RANDOM" | field fingerprint)"
  [ -n "$fp" ] || { note "relay: no device signed in"; return 1; }
  # One command, as a person types it; its code shows in this terminal.
  cli --ssh "$box" join "$CONTROL_URL" --account "$fp" < /dev/null > "$WORK/join.out" 2>&1 &
  pid=$!
  for _ in $(seq 1 120); do
    code="$(sed -nE 's/.*#join=([A-Z0-9]+-[A-Z0-9]+(\.[A-Z0-9-]+)?).*/\1/p' "$WORK/join.out" | head -1)"
    [ -n "$code" ] && break
    kill -0 "$pid" 2>/dev/null || break
    sleep 0.5
  done
  if [ -z "$code" ]; then note "relay: no join code"; cat "$WORK/join.out" >&2; kill "$pid" 2>/dev/null; return 1; fi
  dev approve "$code" > /dev/null || { kill "$pid" 2>/dev/null; return 1; }
  wait "$pid" || { note "relay: join failed"; cat "$WORK/join.out" >&2; return 1; }
  grep -q "Joined." "$WORK/join.out" || return 1
  dev devices | grep -q "\"kind\":\"daemon\",\"name\":\"$box\"" || { note "relay: $box isn't on the device list"; return 1; }
  dev online "$box" 60 > /dev/null || return 1
  # ssh is out of the picture: the CLI's master closed, the bastion paused.
  ssh -F "$CFG" -o ControlPath="$RT/arugula-ssh/%C" -O exit "$box" >/dev/null 2>&1 || true
  docker pause "$TESTNET-bastion" > /dev/null && PAUSED=1
  dev pane "$box" "M52-RELAY-$$" 30 > /dev/null || { note "relay: no pane over the relay"; return 1; }
  # From phones: a Pixel 7 (Chrome) and an iPhone (WebKit), signed in to
  # this control and approved by the device, type into the box's pane;
  # then it's restarted (it comes back to the relay by itself) and the
  # same phones type into it again (web/fixtures/m52-phones.ts).
  started="$(docker inspect -f '{{.State.StartedAt}}' "$TESTNET-$box")"
  node --experimental-strip-types --no-warnings "$ROOT/web/fixtures/m52-phones.ts" --state "$WORK/device.json" \
    --box "$box" --restart "$TESTNET-$box" --marker "M52-$$" > /dev/null || { note "relay: the phones didn't reach the box's pane"; return 1; }
  [ "$(docker inspect -f '{{.State.StartedAt}}' "$TESTNET-$box")" != "$started" ] || return 1
  for _ in $(seq 1 45); do
    if dev pane "$box" "M52-REBOOT-$$" 10 > /dev/null 2>&1; then ok=1; break; fi
    sleep 2
  done
  docker unpause "$TESTNET-bastion" > /dev/null && PAUSED=
  [ "$ok" = 1 ] || { note "relay: no pane over the relay after the restart"; return 1; }
}

claim_unreachable() {
  local box=box-bare url=https://control.arugula.io pid rc=0
  fresh "$box" || return 1
  [ -z "$BREAK" ] || url="$CONTROL_URL"
  cli --ssh "$box" join "$url" < /dev/null > "$WORK/unreachable.out" 2>&1 &
  pid=$!
  for _ in $(seq 1 60); do kill -0 "$pid" 2>/dev/null || break; sleep 0.5; done
  kill "$pid" 2>/dev/null || true
  wait "$pid" 2>/dev/null && rc=1
  grep -qF "$box can't reach control at $url" "$WORK/unreachable.out" || rc=1
  grep -qF "arugula --ssh $box web" "$WORK/unreachable.out" || rc=1
  cli --ssh "$box" ls > /dev/null || { note "unreachable: $box isn't reachable over --ssh"; rc=1; }
  # Leave it bare for the ssh profile's claims.
  fresh "$box" || true
  return "$rc"
}

# Join a box to the stack's control over ssh, approved by the device
# (signed in already), or by the device in state file $3 into team $4.
# Its daemon is up when this returns.
join_box() {
  local box="$1" fp="$2" state="${3:-$WORK/device.json}" team="${4:-}" code="" pid
  # Empty before the join starts: the background job truncates it only once
  # it runs, and a box joined before (cli, then team) left a used code here.
  : > "$WORK/join-$box.out"
  # shellcheck disable=SC2046 # no --team, or --team TEAM
  cli --ssh "$box" join "$CONTROL_URL" --account "$fp" $([ -z "$team" ] || echo --team "$team") \
    < /dev/null > "$WORK/join-$box.out" 2>&1 &
  pid=$!
  for _ in $(seq 1 120); do
    code="$(sed -nE 's/.*#join=([A-Z0-9]+-[A-Z0-9]+(\.[A-Z0-9-]+)?).*/\1/p' "$WORK/join-$box.out" | head -1)"
    [ -n "$code" ] && break
    kill -0 "$pid" 2>/dev/null || break
    sleep 0.5
  done
  if [ -z "$code" ]; then note "join $box: no code"; cat "$WORK/join-$box.out" >&2; kill "$pid" 2>/dev/null; return 1; fi
  # shellcheck disable=SC2086 # no team, or one
  devs "$state" approve "$code" $team > /dev/null || { kill "$pid" 2>/dev/null; return 1; }
  wait "$pid" || { note "join $box failed"; cat "$WORK/join-$box.out" >&2; return 1; }
  devs "$state" online "$box" 60 > /dev/null
}

# The CLI on the bastion, logged in to the stack's control as the device's
# account (fingerprint $1), unless BREAK=1 and $2 is "skip-on-break".
bastion_login() {
  local fp="$1" code pid
  s bastion 'mkdir -p ~/.local/bin && cat > ~/.local/bin/arugula && chmod 755 ~/.local/bin/arugula' \
    < "$ARUGULA_TESTNET_BINARIES/arugula" || return 1
  [ -z "$BREAK" ] || [ "${2:-}" != skip-on-break ] || return 0
  s bastion ".local/bin/arugula login $CONTROL_URL --name bastion --account $fp" < /dev/null > "$WORK/login.out" 2>&1 &
  pid=$!
  code=""
  for _ in $(seq 1 60); do
    code="$(sed -nE 's/.*#join=([A-Z0-9]+-[A-Z0-9]+(\.[A-Z0-9-]+)?).*/\1/p' "$WORK/login.out" | head -1)"
    [ -n "$code" ] && break
    kill -0 "$pid" 2>/dev/null || break
    sleep 0.5
  done
  [ -n "$code" ] || { note "arugula login showed no code"; cat "$WORK/login.out" >&2; return 1; }
  dev approve "$code" > /dev/null || { kill "$pid" 2>/dev/null; return 1; }
  if ! wait "$pid" || ! grep -q "Logged in." "$WORK/login.out"; then
    note "login failed"; cat "$WORK/login.out" >&2; return 1
  fi
  dev devices | grep -q '"kind":"cli","name":"bastion"' || { note "the CLI isn't on the device list"; return 1; }
}

# `arugula --host $1 attach $2` on the bastion, in a pty (ssh -tt), typed
# into as a person would: Enter (the pane's command may have ended), a
# command, then Ctrl-] to leave. Holds when the command's answer came back
# and attach exited 0.
pty_attach() {
  local box="$1" pane="$2" mark="ATTACH-$1-$$" rc=0
  # shellcheck disable=SC2016 # $((...)) is for the pane's shell
  { sleep 4; printf '\r'; sleep 2; printf 'echo %s-$((6*7))\r' "$mark"; sleep 4; printf '\035'; sleep 2; } |
    s -tt bastion "stty cols 100 rows 30; .local/bin/arugula --host $box attach $pane" > "$WORK/attach-$box.out" 2>&1 || rc=1
  grep -q "$mark-42" "$WORK/attach-$box.out" || { note "attach on $box never showed $mark-42 (exit $rc)"; tail -c 600 "$WORK/attach-$box.out" >&2; return 1; }
  [ "$rc" = 0 ] || { note "attach on $box didn't exit 0 after Ctrl-]"; return 1; }
}

# `events --follow` and `tail --follow` on the bastion print while they
# run (a streamed answer through control), before they're stopped.
follows() {
  s bastion bash -s -- "$1" > "$WORK/follow-$1.out" 2>&1 <<'SH'
b="$1" i=.local/bin/arugula
timeout 30 $i --host "$b" events --follow --type bell > /tmp/ev.out 2>&1 &
sleep 3
$i --host "$b" run -- "sleep 1; printf '\a'" > /dev/null
p="$($i --host "$b" --json run -- 'for n in 1 2 3; do sleep 1; echo FOLLOW-$((40+n)); done; sleep 60' | sed -n 's/.*"pane": *\([0-9]*\).*/\1/p')"
timeout 30 $i --host "$b" tail --follow "$p" > /tmp/tail.out 2>&1 &
ok=""
for _ in $(seq 1 40); do
  grep -q '"bell"' /tmp/ev.out && grep -q 'FOLLOW-43' /tmp/tail.out && { ok=1; break; }
  sleep 0.5
done
# Still running: the answers have no end, and came anyway.
jobs -r | grep -c timeout
kill %1 %2 2>/dev/null
echo "events: $(tail -c 200 /tmp/ev.out)"
echo "tail: $(tail -c 200 /tmp/tail.out)"
rm -f /tmp/ev.out /tmp/tail.out
[ -n "$ok" ]
SH
}

claim_cli() {
  local fp code pid b out pane ok
  # The bastion is the CLI's machine: on the inner network, with no daemon.
  # Leave it as it was, whatever happens.
  # shellcheck disable=SC2329,SC2317 # run by the trap below
  bastion_tidy() { s bastion 'rm -rf ~/.local/bin/arugula ~/.config/arugula' >/dev/null 2>&1 || true; }
  bastion_tidy
  if ! fresh box-bare || ! fresh box-systemd; then note "cli: couldn't recreate the boxes"; return 1; fi
  fp="$(signin "cli-$$-$RANDOM" | field fingerprint)"
  [ -n "$fp" ] || { note "cli: no device signed in"; return 1; }
  join_box box-systemd "$fp" || return 1
  join_box box-bare "$fp" || return 1
  # box-bare: reachable on the inner network too, and says so to control.
  # shellcheck disable=SC2016 # expanded on the box
  s box-bare 'pkill -x arugulad; for _ in 1 2 3 4 5 6 7 8 9 10; do pgrep -x arugulad >/dev/null || break; sleep 0.5; done
    ARUGULA_DIRECT_URL=http://box-bare:7681 nohup setsid ~/.local/bin/arugulad --keep-panes --listen 0.0.0.0:7681 \
      </dev/null >"$HOME/.local/state/arugulad.log" 2>&1 &' || return 1
  for _ in $(seq 1 60); do
    dev online box-bare 2 2>/dev/null | grep -q 'box-bare:7681' && break
    sleep 1
  done
  dev online box-bare 2 | grep -q 'box-bare:7681' || { note "cli: box-bare doesn't list its direct URL"; return 1; }
  # The CLI on the bastion, as the boxes got theirs.
  bastion_login "$fp" skip-on-break || { bastion_tidy; return 1; }
  ok=1
  s bastion '.local/bin/arugula hosts' > "$WORK/hosts.out" 2>&1 || ok=0
  grep -q '^box-bare .*direct http://box-bare:7681.*(control: ' "$WORK/hosts.out" || ok=0
  grep -q '^box-systemd .*relayed.*(control: ' "$WORK/hosts.out" || ok=0
  [ "$ok" = 1 ] || { note "cli: arugula hosts on the bastion:"; cat "$WORK/hosts.out" >&2; }
  for b in box-bare:direct box-systemd:relayed; do
    local how="${b#*:}"; b="${b%%:*}"
    # shellcheck disable=SC2016 # $((...)) is for the pane's shell
    out="$(s bastion "ARUGULA_VERBOSE=1 .local/bin/arugula --host $b --json run -- 'echo M49-$b-\$((6*7))'" 2>&1)" || { note "cli: run on $b: $out"; ok=0; continue; }
    grep -q "$b: $how" <<< "$out" || { note "cli: $b wasn't reached $how: $out"; ok=0; }
    pane="$(sed -n 's/.*"pane": *\([0-9]*\).*/\1/p' <<< "$out" | head -1)"
    [ -n "$pane" ] || { note "cli: no pane from $b: $out"; ok=0; continue; }
    s bastion ".local/bin/arugula --host $b ls" | grep -q "^%$pane " || { note "cli: ls on $b doesn't show %$pane"; ok=0; }
    local seen=0
    for _ in $(seq 1 30); do
      if s bastion ".local/bin/arugula --host $b capture $pane" 2>/dev/null | grep -q "M49-$b-42"; then seen=1; break; fi
      sleep 0.5
    done
    [ "$seen" = 1 ] || { note "cli: capture on $b never showed M49-$b-42"; ok=0; continue; }
    # The relayed box: streams and attach (#254).
    [ "$b" = box-systemd ] || continue
    follows "$b" || { note "cli: events/tail --follow on $b:"; cat "$WORK/follow-$b.out" >&2; ok=0; }
    pty_attach "$b" "$pane" || ok=0
  done
  bastion_tidy
  # Leave box-bare bare for the ssh profile's claims.
  fresh box-bare || true
  [ "$ok" = 1 ]
}

claim_team() {
  local fp ofp oacct team code ok=1 out="" pane seen=0
  # shellcheck disable=SC2329,SC2317 # run below
  bastion_tidy() { s bastion 'rm -rf ~/.local/bin/arugula ~/.config/arugula' >/dev/null 2>&1 || true; }
  bastion_tidy
  fresh box-systemd || { note "team: couldn't recreate box-systemd"; return 1; }
  # The owner: a team of their own, and box-systemd joined to it.
  # shellcheck disable=SC2086 # CONTROL_VIA is several words
  out="$(devs "$WORK/owner.json" signin --control "$CONTROL_URL" $CONTROL_VIA --login "owner$$")"
  ofp="$(field fingerprint <<< "$out")"; oacct="$(field account <<< "$out")"
  [ -n "$ofp" ] || { note "team: the owner didn't sign in"; return 1; }
  team="$(devs "$WORK/owner.json" team-create "the team" | field team)"
  [ -n "$team" ] || { note "team: no team"; return 1; }
  join_box box-systemd "$ofp" "$WORK/owner.json" "$team" || return 1
  # The CLI's person: signed in, the CLI logged in, then let into the team.
  fp="$(signin "member$$" | field fingerprint)"
  [ -n "$fp" ] || { note "team: no device signed in"; return 1; }
  bastion_login "$fp" || { bastion_tidy; return 1; }
  code="$(devs "$WORK/owner.json" team-invite "$team" | field code)"
  dev team-accept "$team" "$code" > /dev/null || { note "team: couldn't ask to join"; bastion_tidy; return 1; }
  devs "$WORK/owner.json" team-admit "$team" | grep -q '"admitted":1' || { note "team: not let in"; bastion_tidy; return 1; }
  if [ -n "$BREAK" ]; then
    # Control "changes" the owner's root after the CLI first saw it.
    s bastion "sed -i 's/^{/{\n  \"pins\": {\"$oacct\": \"0000000000000000\"},/' ~/.config/arugula/cli-control.json"
  fi
  s bastion '.local/bin/arugula hosts' > "$WORK/hosts.out" 2>&1 || ok=0
  grep -q "^box-systemd .*relayed.*(control: .*, owner$$'s)" "$WORK/hosts.out" ||
    { note "team: arugula hosts doesn't list box-systemd as owner$$'s:"; cat "$WORK/hosts.out" >&2; ok=0; }
  # The machine's first pane (making panes stays the machine owner's). The
  # owner doesn't type in it: the first to type drives a pane.
  pane="$(devs "$WORK/owner.json" panes box-systemd | sed -n 's/.*"panes":\[\([0-9]*\).*/\1/p')"
  [ -n "$pane" ] || { note "team: box-systemd has no pane"; bastion_tidy; return 1; }
  # The machine's daemon takes the new member's devices when control
  # nudges it; give it a moment.
  for _ in $(seq 1 30); do
    out="$(s bastion "ARUGULA_VERBOSE=1 .local/bin/arugula --host box-systemd capture $pane" 2>&1)" && { seen=1; break; }
    sleep 1
  done
  [ "$seen" = 1 ] || { note "team: capture of %$pane: $out"; bastion_tidy; return 1; }
  grep -q "box-systemd: relayed" <<< "$out" || { note "team: box-systemd wasn't relayed: $out"; ok=0; }
  # A team editor types into it, and sees the answer there and in a capture.
  pty_attach box-systemd "$pane" || ok=0
  s bastion ".local/bin/arugula --host box-systemd capture $pane" 2>/dev/null | grep -q "ATTACH-box-systemd-$$-42" ||
    { note "team: a capture after attach doesn't show what was typed"; ok=0; }
  bastion_tidy
  [ "$ok" = 1 ]
}

# M178: an owner's device delegates the team's project; control enrolls a
# hosted box under it (testnet/hosted-box.ts stands in for `arugulad
# --hosted`, M180), with the enrollment the platform would record
# (`arugula-control hosted-enrollment`); a teammate accepts it and is routed
# to it, an outsider refuses it and isn't; revoking stops the next
# enrollment. BREAK: the teammate (an editor) delegates instead, and is
# refused.
hosted_box() {
  # shellcheck disable=SC2086 # CONTROL_VIA is several words
  node --experimental-strip-types --no-warnings "$HERE/hosted-box.ts" --control "$CONTROL_URL" $CONTROL_VIA "$@"
}
enrollment() {
  docker exec "$TESTNET-control" /opt/arugula/arugula-control hosted-enrollment --db /var/lib/arugula-control/control.db "$@"
}
claim_hosted() {
  local ok=1 team code proj tk id pid who
  for who in howner hmate hout; do
    # shellcheck disable=SC2086 # CONTROL_VIA is several words
    devs "$WORK/$who.json" signin --control "$CONTROL_URL" $CONTROL_VIA --login "$who$$" > /dev/null ||
      { note "hosted: $who didn't sign in"; return 1; }
  done
  team="$(devs "$WORK/howner.json" team-create "hosted team" | field team)"
  code="$(devs "$WORK/howner.json" team-invite "$team" | field code)"
  devs "$WORK/hmate.json" team-accept "$team" "$code" > /dev/null || { note "hosted: couldn't ask to join"; return 1; }
  devs "$WORK/howner.json" team-admit "$team" | grep -q '"admitted":1' || { note "hosted: not let in"; return 1; }
  if [ -n "$BREAK" ]; then who=hmate; else who=howner; fi
  proj="$(devs "$WORK/$who.json" delegate "$team" | field project)"
  [ -n "$proj" ] || { note "hosted: $who couldn't delegate the project"; return 1; }
  tk="$(enrollment --project "$proj" --owner "team:$team" --workspace "hw_testnet$$" --name hosted-box | field ticket)"
  [ -n "$tk" ] || { note "hosted: no enrollment"; return 1; }
  hosted_box --ticket "$tk" --hold 40 > "$WORK/hosted-box.out" 2>&1 &
  pid=$!
  for _ in $(seq 1 40); do grep -q '"dialed":true' "$WORK/hosted-box.out" && break; sleep 0.5; done
  id="$(field device < "$WORK/hosted-box.out")"
  grep -q '"dialed":true' "$WORK/hosted-box.out" || { note "hosted: the box didn't enroll and dial:"; cat "$WORK/hosted-box.out" >&2; kill "$pid"; return 1; }
  # The teammate: listed, accepted for the project, routed.
  devs "$WORK/hmate.json" hosted-entry "$id" > "$WORK/hosted-entry.json" || { note "hosted: the teammate isn't told of it"; ok=0; }
  devs "$WORK/hmate.json" hosted-accept "$WORK/hosted-entry.json" | grep -q '"ok":true' || { note "hosted: the teammate refuses it"; ok=0; }
  devs "$WORK/hmate.json" routed "$id" | grep -q '"routed":true' || { note "hosted: the teammate isn't routed to it"; ok=0; }
  # The outsider: not listed, refuses it when shown it, not routed.
  if devs "$WORK/hout.json" hosted-entry "$id" > /dev/null 2>&1; then note "hosted: an outsider is told of it"; ok=0; fi
  devs "$WORK/hout.json" hosted-accept "$WORK/hosted-entry.json" | grep -q '"why":"not_in_project"' || { note "hosted: an outsider accepts it"; ok=0; }
  devs "$WORK/hout.json" routed "$id" | grep -q '"routed":false' || { note "hosted: an outsider is routed to it"; ok=0; }
  # Revoking stops the next enrollment.
  devs "$WORK/howner.json" undelegate "$team" | grep -q '"revoked":1' || { note "hosted: nothing to revoke"; ok=0; }
  enrollment --project "$proj" --owner "team:$team" --workspace "hw_testnet2$$" | grep -q '"kind":"no_delegation"' ||
    { note "hosted: an enrollment after revoking"; ok=0; }
  # Fail closed: the ticket again is refused, and the machine that used it
  # goes too.
  hosted_box --ticket "$tk" | grep -q '"status":409' || { note "hosted: a ticket worked twice"; ok=0; }
  if devs "$WORK/hmate.json" hosted-entry "$id" > /dev/null 2>&1; then note "hosted: a contested machine stays"; ok=0; fi
  kill "$pid" 2>/dev/null
  [ "$ok" = 1 ]
}

# M212: runs on a hosted workspace. The box is `hosted-box` running the
# real `arugulad --hosted`, enrolled by a ticket under the account's
# delegation; the platform's row for its workspace is written as M179's
# worker would once the box enrolled (no compute provider here). Runs start
# by hand (M214) under the project's standing grant (M211). Needs the
# platform out of process on Postgres (`ARUGULA_TESTNET_PLATFORM=out
# ARUGULA_TESTNET_POSTGRES=1 just testnet up control`). Checked:
# - a run executes once, as `runner`, in its own copy of a repo in
#   /home/user (which it doesn't change), and outlives a platform restart;
#   its events are signed by the box; its pane, watched end to end through
#   control's relay, says what it ran; what it left running is killed when
#   it ends;
# - `runner` can't read the daemon's key, and can't reach the daemon's port
#   on loopback (the box's iptables rules), which `user` can;
# - sent again, after the box's daemon restarts too, it isn't run again;
# - a revoke saying `running stop` stops a run going, and kills everything
#   of `runner`'s;
# - an agent run is Claude Code (a stand-in here), headless, with the
#   instruction, its login on its standard input and never on a command
#   line.
# BREAK: the box's ledger is wiped before the run comes again, so it runs
# twice.
pq() { docker exec "$TESTNET-postgres" psql -U postgres -d platform -tAc "$1"; }
hbox() { docker exec -u user -e HOME=/home/user "$TESTNET-hosted-box" "$@"; }
hroot() { docker exec "$TESTNET-hosted-box" "$@"; }
hosted_daemon() {
  docker exec -d -u user -e HOME=/home/user -e ARUGULA_HOSTED_TICKET="$1" -e ARUGULA_CONTROL_URL="$CONTROL_URL" \
    -e ARUGULA_WORK_PULL_MS=1000 -e RUST_LOG=info -e ANTHROPIC_API_KEY=sk-testnet-m212 "$TESTNET-hosted-box" \
    sh -c 'exec /opt/arugula/arugulad --hosted >> /home/user/arugulad.log 2>&1'
}
run_state() { pq "SELECT state FROM runs WHERE id = '$1'"; }
wait_state() {
  local run="$1" want="$2" secs="${3:-60}"
  for _ in $(seq 1 $((secs * 2))); do [ "$(run_state "$run")" = "$want" ] && return 0; sleep 0.5; done
  note "runs: $run is $(run_state "$run"), not $want"; return 1
}
runner_procs() { hroot sh -c 'pgrep -u runner | wc -l'; }
claim_runs() {
  local ok=1 acct proj ws tk dev run pane marker="/tmp/m212-ran-$$" n port
  pq "SELECT 1" > /dev/null 2>&1 || { note "runs: the platform isn't on the stack's Postgres (ARUGULA_TESTNET_POSTGRES=1)"; return 1; }
  hroot pkill -f 'arugulad --hosted' 2>/dev/null || true
  hroot sh -c 'rm -rf /home/user/.local /home/user/arugulad.log /home/user/app /home/runner/work /tmp/m212-*'
  # The workspace's repo, as M183's setup leaves it: `user`'s, default umask.
  hbox sh -c 'git init -q /home/user/app && cd /home/user/app && echo one > file &&
    git -c user.email=t@t -c user.name=t add file && git -c user.email=t@t -c user.name=t commit -qm one'
  # A Claude Code that says what it was asked, and whether its login came.
  # shellcheck disable=SC2016 # the stand-in's own words
  printf '#!/bin/sh\necho "$* key:${ANTHROPIC_API_KEY:-none}" > /tmp/m212-claude\n' > "$WORK/claude"
  docker cp "$WORK/claude" "$TESTNET-hosted-box:/usr/local/bin/claude" && hroot chmod 755 /usr/local/bin/claude
  # shellcheck disable=SC2086 # CONTROL_VIA is several words
  acct="$(devs "$WORK/rowner.json" signin --control "$CONTROL_URL" $CONTROL_VIA --login "rowner$$" | field account)"
  [ -n "$acct" ] || { note "runs: the owner didn't sign in"; return 1; }
  proj="$(devs "$WORK/rowner.json" delegate | field project)"
  [ -n "$proj" ] || { note "runs: no delegation"; return 1; }
  ws="hw_$(od -An -N16 -tx1 /dev/urandom | tr -d ' \n')"
  tk="$(enrollment --project "$proj" --owner "account:$acct" --workspace "$ws" --name hosted-box | field ticket)"
  [ -n "$tk" ] || { note "runs: no enrollment"; return 1; }
  hosted_daemon "$tk"
  for _ in $(seq 1 60); do
    dev="$(hbox sh -c 'cat ~/.local/state/arugula/control.json 2>/dev/null' |
      python3 -c 'import json, sys; print(json.load(sys.stdin)["hosted"]["cert"]["device"])' 2>/dev/null)"
    [ -n "$dev" ] && break; sleep 0.5
  done
  [ -n "$dev" ] || { note "runs: the box didn't enroll:"; hbox tail -20 /home/user/arugulad.log >&2; return 1; }
  # As M179's worker records it once the hub says the box enrolled.
  pq "INSERT INTO hosted_workspaces (id, project_id, name, spec, spec_hash, state, machine, created_by, created_at, updated_at)
      VALUES ('$ws', '$proj', 'runs', '{}', '', 'ready', '$dev', '$acct', 0, 0)" > /dev/null
  devs "$WORK/rowner.json" grant-sign | grep -q '"grant"' || { note "runs: the owner couldn't sign the standing grant"; return 1; }

  # A run in its own copy of the repo, still going when the platform
  # restarts, leaving a process behind.
  run="$(devs "$WORK/rowner.json" run-start "$ws" --dir /home/user/app -- sh -c \
    "echo ran >> $marker; id -un > $marker.who; pwd > $marker.pwd; echo two > file; (sleep 600 &); sleep 12; echo done" | field run)"
  [ -n "$run" ] || { note "runs: no run started"; return 1; }
  wait_state "$run" running 30 || { hbox tail -30 /home/user/arugulad.log >&2; return 1; }
  docker restart "$TESTNET-platform" > /dev/null
  wait_state "$run" succeeded 60 || { ok=0; hbox tail -30 /home/user/arugulad.log >&2; }
  pq "SELECT seq, from_state, to_state, daemon IS NOT NULL FROM run_events WHERE run_id = '$run' ORDER BY seq" > "$WORK/runs-events"
  [ "$(tr '\n' ' ' < "$WORK/runs-events")" = "1||pending|f 2|pending|running|t 3|running|succeeded|t " ] ||
    { note "runs: its events:"; cat "$WORK/runs-events" >&2; ok=0; }
  [ "$(hroot cat "$marker.who")" = runner ] || { note "runs: it ran as $(hroot cat "$marker.who")"; ok=0; }
  [ "$(hroot cat "$marker.pwd")" = "/home/runner/work/$run/repo" ] || { note "runs: it ran in $(hroot cat "$marker.pwd")"; ok=0; }
  [ "$(hbox cat /home/user/app/file)" = one ] || { note "runs: it changed user's repo"; ok=0; }
  [ "$(hroot cat "/home/runner/work/$run/repo/file")" = two ] || { note "runs: its copy isn't its own"; ok=0; }
  [ "$(runner_procs)" = 0 ] || { note "runs: runner still runs $(runner_procs) processes after the run"; ok=0; }
  # Its pane, watched end to end through control's relay, says what ran.
  pane="$(pq "SELECT detail FROM run_events WHERE run_id = '$run' AND seq = 2" | sed -n 's/.*"pane":\([0-9]*\).*/\1/p')"
  devs "$WORK/rowner.json" watch "$dev" "$pane" "arugula run $run" 20 > /dev/null || { note "runs: its pane isn't watchable"; ok=0; }
  # As `runner`: not the daemon's key, nor its port; `user` reaches the port.
  hroot sudo -u runner sh -c 'cat /home/user/.local/state/arugula/daemon.key' > /dev/null 2>&1 &&
    { note "runs: runner reads the daemon's key"; ok=0; }
  port="$(hbox cat /home/user/.local/state/arugula/listen | sed 's/.*://')"
  hroot sudo -u runner bash -c "exec 3<>/dev/tcp/127.0.0.1/$port" 2>/dev/null &&
    { note "runs: runner reaches the daemon's port $port"; ok=0; }
  hbox bash -c "exec 3<>/dev/tcp/127.0.0.1/$port" || { note "runs: user can't reach the daemon's port $port"; ok=0; }

  # The platform sends it again (its run set back, its job due), and the
  # daemon restarts in between: it isn't run again.
  pq "UPDATE runs SET state = 'pending', last_seq = 1 WHERE id = '$run'" > /dev/null
  hroot pkill -f 'arugulad --hosted'
  [ -z "$BREAK" ] || hbox rm -rf /home/user/.local/state/arugula/work
  pq "UPDATE platform_jobs SET state = 'pending', next_at = 0, lease_until = 0 WHERE id = '$run'" > /dev/null
  hosted_daemon "$tk"
  # Ten pulls or so, each handing it out again.
  sleep 12
  n="$(hroot sh -c "wc -l < $marker")"
  [ "$n" = 1 ] || { note "runs: it ran $n times"; ok=0; }
  [ "$(run_state "$run")" = pending ] || { note "runs: the replay moved: $(run_state "$run")"; ok=0; }
  pq "UPDATE runs SET state = 'succeeded', last_seq = 3 WHERE id = '$run'" > /dev/null
  [ -z "$BREAK" ] || { hroot pkill -f 'arugulad --hosted'; return 1; }

  # An agent run: Claude Code, headless, its login on its standard input.
  run="$(devs "$WORK/rowner.json" agent-run "$ws" "fix the build" | field run)"
  wait_state "$run" succeeded 60 || { ok=0; hbox tail -30 /home/user/arugulad.log >&2; }
  [ "$(hroot cat /tmp/m212-claude)" = "-p fix the build --output-format text --dangerously-skip-permissions key:sk-testnet-m212" ] ||
    { note "runs: the agent got: $(hroot cat /tmp/m212-claude)"; ok=0; }
  hroot sh -c 'ls /home/user/.local/state/arugula/work/*.login' > /dev/null 2>&1 && { note "runs: an agent's login file stayed"; ok=0; }

  # A revoke that says `running stop` stops a run going, and what it left.
  run="$(devs "$WORK/rowner.json" run-start "$ws" -- sh -c "(sleep 600 &); sleep 300" | field run)"
  wait_state "$run" running 30 || ok=0
  devs "$WORK/rowner.json" grant-revoke "" stop > /dev/null || { note "runs: couldn't revoke"; ok=0; }
  wait_state "$run" failed 30 || ok=0
  pq "SELECT detail FROM run_events WHERE run_id = '$run' ORDER BY seq DESC LIMIT 1" | grep -q "grant was revoked" ||
    { note "runs: it failed with $(pq "SELECT detail FROM run_events WHERE run_id = '$run' ORDER BY seq DESC LIMIT 1")"; ok=0; }
  [ "$(runner_procs)" = 0 ] || { note "runs: runner still runs $(runner_procs) processes after the stop"; ok=0; }
  hroot pkill -f 'arugulad --hosted' 2>/dev/null || true
  [ "$ok" = 1 ]
}

# Control's platform, through control's own site (M140).
plat() { node --experimental-strip-types --no-warnings "$HERE/platform.ts" --state "$WORK/device.json" "$@"; }
maintenance() { docker exec "$TESTNET-control" /opt/arugula/arugula-control maintenance "$@" --db /var/lib/arugula-control/control.db; }

claim_platform() {
  local rc=0
  signin "platform-$$-$RANDOM" > /dev/null || { note "platform: no device signed in"; return 1; }
  if [ -n "$BREAK" ]; then maintenance on --retry-after 30 > /dev/null; sleep 2; fi
  plat chat > "$WORK/platform.json" || rc=1
  if [ -n "$BREAK" ]; then maintenance off > /dev/null; sleep 2; fi
  [ "$rc" = 0 ] || { note "platform: chat and Cloud through control:"; cat "$WORK/platform.json" >&2; }
  return "$rc"
}

# The platform page on its own host (M151). Every answer there has the
# page's headers and no Set-Cookie.
claim_agents() {
  local box=box-systemd fp
  fresh "$box" || { note "agents: couldn't recreate $box"; return 1; }
  fp="$(signin "agents-$$-$RANDOM" | field fingerprint)"
  [ -n "$fp" ] || { note "agents: no device signed in"; return 1; }
  join_box "$box" "$fp" || return 1
  plat agents "$box" "$CFG" ${BREAK:+--break} > "$WORK/agents.json" || { note "agents: through MCP on $box:"; return 1; }
}

claim_platform_page() {
  local host="${PLATFORM_HOST:-app.control.test}" ctl="${CONTROL_URL#http://}" port="$ARUGULA_TESTNET_CONTROL_PORT" rc=0 js p
  local csp="default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data: https:; connect-src 'self'; frame-src 'none'; frame-ancestors 'none'; base-uri 'none'; form-action 'self'; object-src 'none'"
  [ -z "$BREAK" ] || host=app2.control.test
  # GET $2 with Host $1: its status, its headers in $WORK/h, its body in $WORK/b.
  pp_at() { curl -sS -o "$WORK/b" -D "$WORK/h" -H "Host: $1" -w '%{http_code}' "http://127.0.0.1:$port$2"; }
  pp_hdr() { sed -n "s/^$1: //Ip" "$WORK/h" | tr -d '\r'; }
  pp_platforms() {
    [ "$(pp_hdr content-security-policy)" = "$csp" ] && [ "$(pp_hdr x-frame-options)" = DENY ] &&
      [ "$(pp_hdr cross-origin-opener-policy)" = same-origin ] && ! grep -qi '^set-cookie:' "$WORK/h"
  }
  # Status $1 for path $2 on the platform's host, with its headers.
  pp_answers() { [ "$(pp_at "$host" "$2")" = "$1" ] && pp_platforms; }
  pp_page() { pp_answers 200 "$1" && grep -q 'id="root"' "$WORK/b"; }
  pp_script() { [ -n "$1" ] && pp_answers 200 "$1" && [ "$(pp_hdr content-type)" = text/javascript ]; }
  # Control's own site, as it was: the shell, under control's CSP.
  pp_shell() { [ "$(pp_at "$ctl" /)" = 200 ] && pp_hdr content-security-policy | grep -q "connect-src 'self' wss:" && ! grep -q "$1" "$WORK/b"; }
  pp_says() { [ "$(pp_at "$ctl" /control.json)" = 200 ] && grep -q "\"platform\":\"http://${PLATFORM_HOST:-app.control.test}:8080\"" "$WORK/b"; }
  for p in / /p/prj_8f2k-testnet/chat/ch_3j9-general /settings; do
    pp_page "$p" || { note "platform-page: $host$p isn't the platform page: $(head -c 300 "$WORK/h")"; rc=1; }
  done
  js="$(sed -n 's/.*<script type="module" crossorigin src="\([^"]*\)".*/\1/p' "$WORK/b" | head -1)"
  pp_script "$js" || { note "platform-page: its script (${js:-none}) isn't served"; rc=1; }
  pp_answers 404 /assets/gone.js || { note "platform-page: a missing asset isn't a 404"; rc=1; }
  pp_answers 401 /api/projects || { note "platform-page: /api/projects isn't the platform's 401"; rc=1; }
  for p in /api/me /api/devices /control.json /auth/github; do
    pp_answers 404 "$p" || { note "platform-page: $p answers on $host"; rc=1; }
  done
  pp_shell "${js:-no script}" || { note "platform-page: control's own page changed"; rc=1; }
  pp_says || { note "platform-page: /control.json doesn't say where the platform page is: $(cat "$WORK/b")"; rc=1; }
  [ "$rc" != 0 ] || note "platform-page: $host serves the platform page ($js), and control.json says so"
  return "$rc"
}

claim_platform_restart() {
  local box=box-systemd fp restart="$TESTNET-platform"
  [ -z "$BREAK" ] || restart="$TESTNET-control"
  fresh "$box" || { note "platform-restart: couldn't recreate $box"; return 1; }
  fp="$(signin "hold-$$-$RANDOM" | field fingerprint)"
  [ -n "$fp" ] || { note "platform-restart: no device signed in"; return 1; }
  join_box "$box" "$fp" || return 1
  plat hold "$box" "$restart" > "$WORK/hold.json" || { note "platform-restart: the relayed session didn't hold"; return 1; }
  note "platform-restart: $(cat "$WORK/hold.json")"
}

# A pull_request webhook for a repository nobody watches, signed with
# `secret`, to control's site: the status, the answer in $WORK/hook.json.
hook() {
  local body sig
  body='{"action":"opened","repository":{"full_name":"testnet/nobody"},"pull_request":{"number":1}}'
  sig="sha256=$(printf '%s' "$body" | openssl dgst -sha256 -hmac "$2" -r | cut -d' ' -f1)"
  curl -s -o "$WORK/hook.json" -w '%{http_code}' -X POST "http://127.0.0.1:$ARUGULA_TESTNET_CONTROL_PORT/github/webhook" \
    -H 'content-type: application/json' -H 'x-github-event: pull_request' -H "x-github-delivery: $1" \
    -H "x-hub-signature-256: $sig" --data-binary "$body"
}

# Whether the last hook was taken as one already seen.
duplicate() { if [ "$1" = 200 ]; then grep -q '"duplicate":true' "$WORK/hook.json"; else return 1; fi; }

claim_github_webhook() {
  local d="testnet-$$-$RANDOM" secret=whsec-github-testnet st=""
  [ -z "$BREAK" ] || secret=not-the-apps-secret
  st="$(hook "$d" "$secret")"
  [ "$st" = 202 ] || { note "github-webhook: $st $(cat "$WORK/hook.json")"; return 1; }
  # The log is coloured: the delivery id and the line's words, not `delivery=`.
  docker logs "$TESTNET-platform" 2>&1 | grep "webhook handed to the hub" | grep -q "$d" ||
    { note "github-webhook: the platform didn't log delivery $d"; return 1; }
  st="$(hook "$d" "$secret")"
  duplicate "$st" || { note "github-webhook: sent again: $st $(cat "$WORK/hook.json")"; return 1; }
  docker restart "$TESTNET-platform" >/dev/null
  # Until the platform answers again, control's gateway says 503.
  for _ in $(seq 1 60); do
    st="$(hook "$d" "$secret")"
    case "$st" in 502|503) sleep 0.5 ;; *) break ;; esac
  done
  duplicate "$st" || { note "github-webhook: after the platform's restart: $st $(cat "$WORK/hook.json")"; return 1; }
  note "github-webhook: $d landed on the platform, and was known again after its restart"
}

claim_issues_restart() {
  signin "issues-$$-$RANDOM" > /dev/null || { note "issues-restart: no device signed in"; return 1; }
  plat issues-write > "$WORK/issues.json" || { note "issues-restart: couldn't write issues"; return 1; }
  if [ -n "$BREAK" ]; then
    # shellcheck disable=SC2086 # the files and profiles up.sh used
    docker compose $ARUGULA_TESTNET_COMPOSE up -d --force-recreate --no-deps --wait platform >/dev/null 2>&1
  else
    docker restart "$TESTNET-platform" >/dev/null
  fi
  plat issues-read "$WORK/issues.json" > "$WORK/issues-read.json" ||
    { note "issues-restart: what was written didn't read back after the restart"; return 1; }
  note "issues-restart: $(cat "$WORK/issues-read.json")"
}

# platform2 recreated, on Postgres or (BREAK=1) a file of its own.
platform2() {
  # shellcheck disable=SC2086 # the files and profiles up.sh used
  ARUGULA_TESTNET_PLATFORM2_DB="$1" docker compose $ARUGULA_TESTNET_COMPOSE up -d --force-recreate --no-deps --wait \
    platform2 managoat-platform2 >/dev/null 2>&1
}

claim_platform_fanout() {
  local rc=0
  signin "fanout-$$-$RANDOM" > /dev/null || { note "platform-fanout: no device signed in"; return 1; }
  [ -z "$BREAK" ] || platform2 ""
  plat fanout "$ARUGULA_TESTNET_PLATFORM_PORT" "$ARUGULA_TESTNET_PLATFORM2_PORT" > "$WORK/fanout.json" || rc=1
  [ -z "$BREAK" ] || platform2 postgres://postgres:testnet@postgres:5432/platform
  if [ "$rc" = 0 ]; then note "platform-fanout: $(cat "$WORK/fanout.json")"; fi
  return "$rc"
}

claim_platform_handover() {
  signin "handover-$$-$RANDOM" > /dev/null || { note "platform-handover: no device signed in"; return 1; }
  plat handover "$ARUGULA_TESTNET_MANAGOAT_PORT" "$TESTNET" ${BREAK:+--break} > "$WORK/handover.json" || return 1
  note "platform-handover: $(cat "$WORK/handover.json")"
}

# M183: a hosted workspace's setup, end to end (the header says what).
e2b_boxes() {
  docker exec "$TESTNET-fakes" node -e "fetch('http://127.0.0.1:9006/_testnet/sandboxes').then((r) => r.json()).then((j) => console.log(JSON.stringify(j)))"
}
# The fake's boxes for project $1, oldest first: `live <id> <ticket>` for each
# it holds, `deleted <id>` for each deleted.
e2b_project() {
  docker exec "$TESTNET-fakes" node -e "fetch('http://127.0.0.1:9006/_testnet/sandboxes').then((r) => r.json()).then((j) => {
    for (const b of j.boxes) if (b.metadata.arugula_project === '$1') { console.log('live', b.sandboxID, b.envVars.ARUGULA_HOSTED_TICKET ?? ''); }
    for (const id of j.deleted) if ((j.made ?? {})[id] === '$1') console.log('deleted', id);
  })"
}
git_repo() {
  docker exec "$TESTNET-git" sh -ec "d=/srv/git/$1.git; rm -rf \$d; git init -q --bare -b main \$d; w=\$(mktemp -d)
    git -C \$w init -q -b main; echo $1 > \$w/README; git -C \$w add README
    git -C \$w -c user.name=testnet -c user.email=testnet@git commit -q -m first; git -C \$w push -q \$d main
    chown -R git:git \$d; rm -rf \$w"
}
claim_hosted_setup() {
  local ok=1 box=box-systemd fp team proj tk first got pw out pane rc
  bastion_tidy() { s bastion 'rm -rf ~/.local/bin/arugula ~/.config/arugula /tmp/dev.tgz' >/dev/null 2>&1 || true; }
  bastion_tidy
  fresh "$box" || { note "hosted-setup: couldn't recreate $box"; return 1; }
  git_repo app && git_repo lib || { note "hosted-setup: no repos on the git server"; return 1; }
  fp="$(signin "hsetup-$$-$RANDOM" | field fingerprint)"
  [ -n "$fp" ] || { note "hosted-setup: no device signed in"; return 1; }
  team="$(dev team-create "setup team" | field team)"
  proj="$(dev delegate "$team" | field project)"
  [ -n "$proj" ] || { note "hosted-setup: couldn't delegate the team's project"; return 1; }
  bastion_login "$fp" || { bastion_tidy; return 1; }
  bcli() { s bastion ".local/bin/arugula $*"; }
  # shellcheck disable=SC2016 # run by the setup, on the box
  printf '%s\n' 'test -f ../lib/README' 'echo "built $(cat README)" > built.txt' |
    bcli cloud create dev --project "$proj" --repo https://git/app.git --repo https://git/lib.git --setup - > "$WORK/hs-create" ||
    { note "hosted-setup: no workspace made:"; cat "$WORK/hs-create" >&2; bastion_tidy; return 1; }
  pw="$(cat "$STATE/git_https_password")"
  [ -z "$BREAK" ] || pw="not-the-password"
  printf '%s\n' "$pw" | bcli cloud secret set dev GIT_TOKEN --project "$proj" > /dev/null ||
    { note "hosted-setup: couldn't set GIT_TOKEN"; ok=0; }
  # The box the platform made at the fake E2B, and the ticket in its env.
  for _ in $(seq 1 30); do
    tk="$(e2b_project "$proj" | awk '$1 == "live" { print $3 }' | tail -1)"
    [ -n "$tk" ] && break
    sleep 1
  done
  [ -n "$tk" ] || { note "hosted-setup: the platform made no box at the fake E2B: $(e2b_boxes)"; bastion_tidy; return 1; }
  first="$(e2b_project "$proj" | awk '$1 == "live" { print $2 }' | tail -1)"
  # The real hosted daemon on box-systemd, with that box's env.
  docker cp "$ARUGULA_TESTNET_BINARIES/arugulad" "$TESTNET-$box:/usr/local/bin/arugulad" &&
    docker cp "$STATE/git_https.crt" "$TESTNET-$box:/home/illo/git-ca.crt" &&
    docker exec "$TESTNET-$box" sh -c 'chmod 755 /usr/local/bin/arugulad && chown illo /home/illo/git-ca.crt' ||
    { note "hosted-setup: couldn't install arugulad on $box"; bastion_tidy; return 1; }
  s "$box" 'loginctl enable-linger; echo sbx-testnet > ~/e2b-id' > /dev/null
  s "$box" systemd-run --user --unit=arugulad-hosted -E "ARUGULA_HOSTED_TICKET=$tk" -E "ARUGULA_CONTROL_URL=$CONTROL_URL" \
    -E ARUGULA_E2B_ID_FILE=/home/illo/e2b-id -E GIT_SSL_CAINFO=/home/illo/git-ca.crt \
    /usr/local/bin/arugulad --hosted --listen 127.0.0.1:7690 --state-dir /home/illo/hosted-state > /dev/null 2>&1 ||
    { note "hosted-setup: arugulad --hosted didn't start"; bastion_tidy; return 1; }
  # Enrolled, set up: `open --wait` follows the setup to its end.
  rc=0
  bcli cloud open dev --project "$proj" --wait > "$WORK/hs-open" 2>&1 || rc=$?
  if [ "$rc" != 0 ] || ! grep -q "  set up" "$WORK/hs-open"; then
    note "hosted-setup: not set up (exit $rc):"; cat "$WORK/hs-open" >&2
    s "$box" journalctl --user -u arugulad-hosted --no-pager -o cat 2>/dev/null | tail -20 >&2
    s "$box" 'systemctl --user stop arugulad-hosted' > /dev/null 2>&1; bastion_tidy; return 1
  fi
  # Its pane, as anyone on the project reaches it: in session `setup`.
  pane="$(bcli --host dev --json ls 2>/dev/null | tr -d ' \n' | sed -n 's/.*"id":\([0-9]*\),[^}]*"session_name":"setup".*/\1/p' | head -1)"
  [ -n "$pane" ] || pane="$(bcli --host dev ls 2>/dev/null | awk '/setup/ { sub("%", "", $1); print $1; exit }')"
  bcli --host dev capture "$pane" 2>/dev/null > "$WORK/hs-pane"
  grep -q "==> clone app" "$WORK/hs-pane" && grep -q "==> clone lib" "$WORK/hs-pane" && grep -q "Set up." "$WORK/hs-pane" ||
    { note "hosted-setup: the setup pane (%$pane) doesn't show it:"; cat "$WORK/hs-pane" >&2; ok=0; }
  [ "$(s "$box" cat app/built.txt 2>/dev/null)" = "built app" ] || { note "hosted-setup: the script didn't run in app"; ok=0; }
  s "$box" 'git -C app config --get-regexp "^http" ; git -C app remote get-url origin' 2>/dev/null | grep -qx https://git/app.git ||
    { note "hosted-setup: the clone kept something besides its plain URL"; ok=0; }
  # A spec edit: rebuild needed.
  printf 'echo again\n' | bcli cloud edit dev --project "$proj" --setup - > /dev/null || { note "hosted-setup: couldn't edit the spec"; ok=0; }
  bcli cloud ls --project "$proj" | grep -q "rebuild needed" || { note "hosted-setup: no rebuild needed"; ok=0; }
  # Work that isn't pushed stops a rebuild; --download saves it first.
  s "$box" 'echo wip > app/wip.txt'
  if bcli cloud rebuild dev --project "$proj" > "$WORK/hs-rebuild" 2>&1; then note "hosted-setup: a rebuild didn't warn"; ok=0; fi
  # The setup's built.txt and the work: neither is pushed.
  grep -q "app: 2 changed file" "$WORK/hs-rebuild" || { note "hosted-setup: the warning:"; cat "$WORK/hs-rebuild" >&2; ok=0; }
  bcli cloud rebuild dev --project "$proj" --download /tmp/dev.tgz > "$WORK/hs-rebuild" 2>&1 ||
    { note "hosted-setup: rebuild --download:"; cat "$WORK/hs-rebuild" >&2; ok=0; }
  s bastion 'tar -tzf /tmp/dev.tgz' 2>/dev/null | grep -qx app/wip.txt || { note "hosted-setup: the download hasn't the work"; ok=0; }
  for _ in $(seq 1 20); do
    got="$(e2b_project "$proj")"
    grep -qx "deleted $first" <<< "$got" && awk '$1 == "live" { print $2 }' <<< "$got" | grep -qvx "$first" && break
    sleep 1
  done
  grep -qx "deleted $first" <<< "$got" && awk '$1 == "live" { print $2 }' <<< "$got" | grep -qvx "$first" ||
    { note "hosted-setup: no new box at the fake E2B: $got"; ok=0; }
  s "$box" 'systemctl --user stop arugulad-hosted' > /dev/null 2>&1
  bastion_tidy
  [ "$ok" = 1 ]
}

failed=""
for c in $claims; do
  case " $CLAIMS " in *" $c "*) ;; *) echo "unknown claim '$c' (claims: $CLAIMS)" >&2; exit 2 ;; esac
  if "claim_${c//-/_}"; then echo "[testnet $PROFILE $c] PASS${BREAK:+ (BREAK=1: not caught)}"; else echo "[testnet $PROFILE $c] FAIL${BREAK:+ (BREAK=1: caught)}"; failed="$failed $c"; fi
  # A claim that failed with the bastion paused (relay's) left it so: every
  # claim after it would wait out its ssh timeouts through a dead hop.
  if [ -n "${PAUSED:-}" ]; then docker unpause "$TESTNET-bastion" >/dev/null 2>&1 || true; PAUSED=; fi
done
[ -z "$failed" ] || exit 1
