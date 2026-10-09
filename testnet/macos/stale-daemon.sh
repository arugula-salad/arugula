#!/usr/bin/env bash
#
# #661 on macOS, in a fresh tart VM: a daemon older than the arugulad the
# app carries still works, so the app says so and offers its update, and
# changes nothing until asked (#392: the daemon updates itself).
#
#   testnet/macos/stale-daemon.sh [claim...]   (after `just desktop`)
#   KEEP=1 ...                                 leave the clone running
#
# A released arugulad (OLD, default 0.26.1: the oldest whose own update
# finds the renamed repository's releases, #525) is installed as the
# service with its own `arugulad install` (~/.local/bin and
# ~/Library/LaunchAgents/arugulad.plist, as install.sh does), in a fresh
# clone, then the app from $ARUGULA_DMG (default
# dist/arugula-desktop-macos-arm64.dmg) starts:
#   running  it runs, with a pane counting: the app says it's older than
#            its own and offers the update, and updates nothing unasked;
#            *Update arugulad* (Return, the page's default button, typed
#            through System Events) updates it under its own agent to a
#            newer release, and the pane is the same process, still
#            counting
#   stopped  the same with its service stopped: the app starts it, then
#            says so and offers the update
#
# A daemon too old for the app (#317, below 0.19.0) is only illogicald,
# which nothing runs since #534: `just desktop-xvfb stale` keeps that
# check, with a stand-in installed as arugulad.
# shellcheck disable=SC2016,SC2329 # strings run in the VM expand there; claims run as claim_$c
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$HERE/../.." && pwd)"
V="$HERE/vm.sh"
VM="${ARUGULA_MACOS_VM:-arugula-macos-l}"
DMG="${ARUGULA_DMG:-$ROOT/dist/arugula-desktop-macos-arm64.dmg}"
OLD="${OLD:-0.26.1}"
# shellcheck source=testnet/macos/need-tart.sh
. "$HERE/need-tart.sh"
[ -f "$DMG" ] || { echo "no .dmg (ARUGULA_DMG, or build one: just desktop)" >&2; exit 2; }

claims=("$@")
[ ${#claims[@]} -gt 0 ] || claims=(running stopped)
failed=0
v() { "$V" "$1" "$VM" "${@:2}"; }
vs() { v ssh "$@"; }
pass() { echo "[macos stale $1] ok${2:+: $2}"; }
fail() { echo "[macos stale $1] FAIL: $2" >&2; failed=1; }
wait_for() {
  local until=$((SECONDS + $1)); shift
  until "$@"; do
    [ "$SECONDS" -lt "$until" ] || return 1
    sleep 1
  done
}

work=$(mktemp -d)
trap 'rm -rf "$work"; [ -n "${KEEP:-}" ] || v down >/dev/null' EXIT
# A released daemon, checked against its release's SHA256SUMS, into
# $work/DIR.
fetch() {
  local tgz=arugula-$1-aarch64-apple-darwin.tar.gz
  curl -fsSL -o "$work/$tgz" "https://github.com/arugula-salad/arugula/releases/download/v$1/$tgz"
  curl -fsSL -o "$work/$1.sums" "https://github.com/arugula-salad/arugula/releases/download/v$1/SHA256SUMS"
  (cd "$work" && grep " $tgz\$" "$1.sums" | shasum -a 256 -c - >/dev/null) || { echo "$tgz doesn't match SHA256SUMS" >&2; exit 1; }
  mkdir "$work/$2"
  tar -xzf "$work/$tgz" -C "$work/$2" --strip-components 1
}
fetch "$OLD" old

# A fresh clone with the app in /Applications and the old daemon in /tmp.
fresh() {
  v down >/dev/null
  v up >/dev/null
  v push "$DMG" /tmp/arugula.dmg
  vs 'set -e; hdiutil attach -nobrowse -quiet -mountpoint /tmp/d /tmp/arugula.dmg; cp -R /tmp/d/Arugula.app /Applications/; hdiutil detach -quiet /tmp/d'
  vs 'mkdir -p /tmp/old'
  v push "$work/old/arugulad" "$work/old/arugula" /tmp/old/
  vs 'xattr -dr com.apple.quarantine /tmp/old 2>/dev/null; chmod +x /tmp/old/*; true'
}

# With the local token.
answers() { vs 'curl -s -H "Authorization: Bearer $(cat ~/.local/state/arugula/local-token 2>/dev/null)" http://127.0.0.1:7681/api/host' | python3 -c 'import json, sys; print(json.load(sys.stdin).get("version", ""))' 2>/dev/null || true; }
# What 7681 says, raw, for a failure's message.
raw() { vs 'curl -s -i -m 5 -H "Authorization: Bearer $(cat ~/.local/state/arugula/local-token 2>/dev/null)" http://127.0.0.1:7681/api/host | head -1; ls ~/.local/state 2>&1 | tr "\n" " "' 2>&1 || true; }
answers_old() { [ "$(answers)" = "$OLD" ]; }
nothing_answers() { [ -z "$(answers)" ]; }
# A version newer than OLD (`sort -V` puts OLD first).
newer() { local now; now=$(answers); [ -n "$now" ] && [ "$now" != "$OLD" ] && [ "$(printf '%s\n%s\n' "$OLD" "$now" | sort -V | head -1)" = "$OLD" ]; }
said_older() { vs "grep -q 'arugulad here is $OLD, older than the' /tmp/app.log"; }
start_app() { vs '(nohup /Applications/Arugula.app/Contents/MacOS/arugula-desktop >>/tmp/app.log 2>&1 &)'; }
pane_pid() { vs "pgrep -f '[>] /tmp/count; sleep'" | head -1 || true; }
count() { vs 'cat /tmp/count' 2>/dev/null || echo 0; }
# The older daemon as install.sh leaves it, in a fresh clone.
install_old() {
  fresh
  vs '/tmp/old/arugulad install >/dev/null'
  wait_for 30 answers_old || fail "$1" "arugulad $OLD didn't answer after its install ($(raw))"
}
# The setup page offers the update and nothing changes until it's pressed.
offered() {
  if wait_for 60 said_older; then
    pass "$1" "the app says arugulad $OLD is older than its own"
  else
    fail "$1" "the app never said so: $(vs 'grep arugula /tmp/app.log | tail -3')"
    return 1
  fi
  sleep 10
  if answers_old; then
    pass "$1" "arugulad $OLD still answers: nothing updated unasked"
  else
    fail "$1" "the daemon became '$(answers)' unasked ($(raw))"
  fi
}
# *Update arugulad*: the daemon's own update, under its own agent.
update() {
  local how
  how=$(vs 'osascript -e "tell application \"System Events\" to set frontmost of process \"arugula-desktop\" to true" -e "delay 1" -e "tell application \"System Events\" to key code 36"' 2>&1) \
    || { fail "$1" "pressing Return for Update arugulad: $how"; return 1; }
  if wait_for 180 newer; then
    pass "$1" "Update arugulad brought up arugulad $(answers)"
  else
    fail "$1" "after Update arugulad the daemon is '$(answers)', not newer than $OLD: $(vs 'grep arugula /tmp/app.log | tail -3')"
    return 1
  fi
  if vs 'launchctl print gui/$(id -u)/arugulad >/dev/null 2>&1'; then
    pass "$1" "it runs under its own agent"
  else
    fail "$1" "the arugulad agent isn't loaded"
  fi
}

claim_running() {
  install_old running
  vs "\$HOME/.local/bin/arugula run --session work \"i=0; while :; do i=\\\$((i+1)); echo \\\$i > /tmp/count; sleep 0.2; done\"" >/dev/null
  wait_for 10 vs 'test -s /tmp/count' || true
  local pane0; pane0=$(pane_pid)
  start_app
  offered running || return 0
  update running || return 0
  local pane1 c0; pane1=$(pane_pid); c0=$(count); sleep 2
  if [ -n "$pane0" ] && [ "$pane0" = "$pane1" ] && [ "$(count)" -gt "$c0" ]; then
    pass running "the pane is the same process ($pane0), still counting"
  else
    fail running "the pane was ${pane0:-none}, now ${pane1:-none}, count $c0 then $(count)"
  fi
}

claim_stopped() {
  install_old stopped
  vs 'launchctl bootout gui/$(id -u)/arugulad 2>/dev/null; true'
  wait_for 15 nothing_answers || fail stopped "arugulad still answers after launchctl bootout"
  start_app
  wait_for 60 answers_old || fail stopped "the app didn't start the stopped $OLD service"
  offered stopped || return 0
  update stopped || true
}

for c in "${claims[@]}"; do
  "claim_$c"
done
exit $failed
