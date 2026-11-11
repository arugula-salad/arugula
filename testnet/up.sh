#!/usr/bin/env bash
#
# Bring up one profile of the test stack.
#
#   testnet/up.sh ssh       bastion, box-bare, box-systemd and git (see README.md)
#   testnet/up.sh tailnet   headscale, ts-box and ts-client (S28's comparison)
#
# Makes the stack's keys in testnet/.state (.state-<name> for another
# COMPOSE_PROJECT_NAME) (a client key and one host key per
# box) and writes testnet/.state/ssh_config, which reaches every box by name
# with strict host key checking:
#
#   ssh -F testnet/.state/ssh_config box-bare
#
# Re-running it is safe: existing keys are kept and the containers are
# recreated only if their config changed.
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROFILE="${1:-ssh}"
# shellcheck source-path=SCRIPTDIR source=env.sh
. "$HERE/env.sh"
PORT="${ARUGULA_TESTNET_SSH_PORT:-22922}"

log() { echo "[testnet up $PROFILE] $*" >&2; }
die() { log "FAIL: $*"; exit 1; }

need_docker

files="-f $HERE/compose.yaml"
case "$PROFILE" in
  ssh) boxes="bastion box-bare box-systemd git"; profiles="--profile ssh" ;;
  tailnet) boxes="ts-box ts-client"; profiles="--profile tailnet" ;;
  *) echo "usage: testnet/up.sh ssh|tailnet" >&2; exit 2 ;;
esac

mkdir -p "$STATE"
[ -f "$STATE/id_ed25519" ] || ssh-keygen -q -t ed25519 -N '' -C arugula-testnet -f "$STATE/id_ed25519"
for b in $boxes; do
  [ -f "$STATE/${b}_host_ed25519" ] || ssh-keygen -q -t ed25519 -N '' -C "$b" -f "$STATE/${b}_host_ed25519"
done
# The git server's HTTPS (M183): a self-signed certificate for `git`, which
# the boxes are told to trust (GIT_SSL_CAINFO), and a password for its basic
# auth, user x-access-token, as a forge takes a token.
if [ ! -f "$STATE/git_https.crt" ]; then
  openssl req -x509 -newkey ec -pkeyopt ec_paramgen_curve:prime256v1 -nodes -days 3650 -subj /CN=git \
    -addext subjectAltName=DNS:git -keyout "$STATE/git_https.key" -out "$STATE/git_https.crt" 2>/dev/null
fi
if [ ! -f "$STATE/git_https_password" ]; then
  openssl rand -hex 16 > "$STATE/git_https_password"
  printf 'x-access-token:%s\n' "$(openssl passwd -apr1 "$(cat "$STATE/git_https_password")")" > "$STATE/git_htpasswd"
fi
# Every profile's hosts, so bringing up one profile doesn't drop another's.
: > "$STATE/known_hosts.new"
for k in "$STATE"/*_host_ed25519.pub; do
  b="$(basename "$k" _host_ed25519.pub)"
  echo "$b $(cut -d' ' -f1,2 "$k")" >> "$STATE/known_hosts.new"
done
mv "$STATE/known_hosts.new" "$STATE/known_hosts"

cat > "$STATE/ssh_config" <<CFG
# Written by testnet/up.sh. Every box by name, keys only, strict host keys.
Host bastion
  HostName 127.0.0.1
  Port $PORT
  HostKeyAlias bastion

Host box-bare
  HostName box-bare
  ProxyJump bastion

Host box-systemd
  HostName box-systemd
  ProxyJump bastion

Host *
  User illo
  IdentityFile "$STATE/id_ed25519"
  IdentitiesOnly yes
  UserKnownHostsFile "$STATE/known_hosts"
  StrictHostKeyChecking yes
  BatchMode yes
  ConnectTimeout 10
CFG

if [ "$PROFILE" = tailnet ]; then
  # headscale first, for a reusable, ephemeral key the nodes join with.
  docker compose -f "$HERE/compose.yaml" --profile tailnet up -d --wait headscale >&2
  hs() { docker exec "$TESTNET-headscale" headscale "$@"; }
  for _ in $(seq 1 40); do hs users list >/dev/null 2>&1 && break; sleep 0.5; done
  hs users list -o json | grep -q '"name": *"illo"' || hs users create illo >/dev/null
  uid="$(hs users list -o json | tr -d ' \t\n' | sed 's/.*"id":\([0-9]*\),"name":"illo".*/\1/')"
  hs preauthkeys create --user "$uid" --reusable --ephemeral --expiration 24h > "$STATE/tailnet-authkey.new"
  tail -n1 "$STATE/tailnet-authkey.new" > "$STATE/tailnet-authkey" && rm "$STATE/tailnet-authkey.new"
fi

# The images built (or pulled) and the containers made, first: on a cold
# Docker (a CI pod's own) that's minutes of apt-get, which is work, not
# waiting, so no deadline. Then started, and ready within WAIT seconds:
# every service with a healthcheck passing it (compose.yaml's x-ready), so
# a box answers ssh, not just run.
wait="${ARUGULA_TESTNET_WAIT:-180}"
# shellcheck disable=SC2086 # several words: the files and profiles
docker compose $files $profiles up --build --no-start >&2
start() {
  # shellcheck disable=SC2086
  if ! docker compose $files $profiles up -d --wait --wait-timeout "$wait" >&2; then
    # shellcheck disable=SC2086
    docker compose $files $profiles ps -a >&2 || true
    # shellcheck disable=SC2086
    docker compose $files $profiles logs --tail=40 >&2 || true
    die "the stack wasn't ready in ${wait}s (ARUGULA_TESTNET_WAIT)"
  fi
}
start

# Healthy means each service answers itself (its check dials 127.0.0.1),
# not that the host forwards between them: with bridged traffic going
# through the host's iptables (bridge-nf-call-iptables, as on a k3s node),
# each network needs the ACCEPT rules Docker adds when it makes it. On geek,
# k3s's network policy controller (kube-router) replaces the whole filter
# table from a copy it saved a moment earlier, up to several times a
# second while pods come and go, and so erases rules Docker added in
# between: a network made then drops every packet between its containers,
# for its whole life (#282). So: TCP across each network, from the
# bastion, and if one drops it, the stack's networks made again, once
# (the rules come with them).
blocked() {
  local to
  case "$PROFILE" in
    ssh) to="box-bare:22" ;;
    *) return 1 ;;
  esac
  for t in $to; do
    docker exec "$TESTNET-bastion" timeout 5 bash -c "exec 3<>/dev/tcp/${t%:*}/${t#*:}" 2>/dev/null || echo "$t"
  done
}
lost="$(blocked)" || true
if [ -n "$lost" ]; then
  log "the bastion can't reach $lost, with every service healthy: the host drops traffic between the stack's containers"
  log "(Docker's rules for a new network were lost, as when k3s's kube-router rewrites the filter table: #282); making the networks again"
  # shellcheck disable=SC2086
  docker compose $files $profiles down >&2 || true
  start
  lost="$(blocked)" || true
  [ -z "$lost" ] || die "the bastion still can't reach $lost, with every service healthy: the host's firewall (iptables FORWARD) drops traffic between the stack's containers (#282)"
fi

if [ "$PROFILE" = tailnet ]; then
  for _ in $(seq 1 60); do
    if docker exec "$TESTNET-ts-client" tailscale ping -c 1 ts-box >/dev/null 2>&1; then
      log "up: docker exec $TESTNET-ts-client tailscale status"
      exit 0
    fi
    sleep 0.5
  done
  docker compose -f "$HERE/compose.yaml" --profile tailnet logs --tail=40 >&2 || true
  die "ts-client can't reach ts-box over the tailnet"
fi

# The bastion and box-bare are healthy (sshd answers in each), so this is
# the path from here: the published port, the jump, the keys.
if ssh -F "$STATE/ssh_config" box-bare true 2>/dev/null; then
  log "up: ssh -F $STATE/ssh_config box-bare"
  exit 0
fi
ssh -F "$STATE/ssh_config" -v box-bare true || true
# shellcheck disable=SC2086
docker compose $files $profiles logs --tail=40 >&2 || true
die "box-bare did not answer through the bastion"
