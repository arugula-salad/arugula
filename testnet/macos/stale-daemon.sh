#!/usr/bin/env bash
#
# #317 on macOS, in a fresh tart VM: the app never shows a daemon older
# than 0.19.0 (one that reports no protocol), and never updates it either
# (#392: the daemon updates itself). Its window stays on the setup page,
# which says which version runs and which the app needs.
#
#   testnet/macos/stale-daemon.sh [claim...]   (after `just desktop`)
#   KEEP=1 ...                                 leave the clone running
#
# A released arugulad (OLD, default 0.8.0, the one #317 saw; it must be
# below 0.19.0) is installed as the service with its own `arugulad
# install` (~/.local/bin and ~/Library/LaunchAgents/illogicald.plist, as
# install.sh did; releases before 0.26.0 are illogical's), then the app from $ARUGULA_DMG (default
# dist/arugula-desktop-macos-arm64.dmg) starts:
#   stopped  the service is installed but not running: the app starts it,
#            finds it too old, says so, and OLD still answers
#   running  the same with OLD already running
#
# #661: an illogicald from before the rename (ILL, default 0.21.0, what
# lex00's Mac had), installed as its install script did, in a fresh clone:
#   illogical_running  it runs, with a pane counting: the app says it's
#            illogicald from before the rename and offers the update, and
#            updates nothing unasked; *Update arugulad* (Return, the page's
#            default button, typed through System Events) brings up the
#            app's arugulad, the
#            illogicald agent is gone and the pane is the same process,
#            still counting
#   illogical_stopped  the same with its service stopped: the app starts
#            it, then says so and offers the update
# shellcheck disable=SC2016,SC2329 # strings run in the VM expand there; claims run as claim_$c
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$HERE/../.." && pwd)"
V="$HERE/vm.sh"
VM="${ARUGULA_MACOS_VM:-arugula-macos-l}"
DMG="${ARUGULA_DMG:-$ROOT/dist/arugula-desktop-macos-arm64.dmg}"
OLD="${OLD:-0.8.0}"
ILL="${ILL:-0.21.0}"
# shellcheck source=testnet/macos/need-tart.sh
. "$HERE/need-tart.sh"
[ -f "$DMG" ] || { echo "no .dmg (ARUGULA_DMG, or build one: just desktop)" >&2; exit 2; }

claims=("$@")
[ ${#claims[@]} -gt 0 ] || claims=(stopped running illogical_running illogical_stopped)
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
# A released daemon from before the rename (#505: its tarball and binaries
# keep illogical's names), checked against its release's SHA256SUMS, into
# $work/DIR.
fetch() {
  local tgz=illogical-$1-aarch64-apple-darwin.tar.gz
  curl -fsSL -o "$work/$tgz" "https://github.com/arugula-salad/arugula/releases/download/v$1/$tgz"
  curl -fsSL -o "$work/$1.sums" "https://github.com/arugula-salad/arugula/releases/download/v$1/SHA256SUMS"
  (cd "$work" && grep " $tgz\$" "$1.sums" | shasum -a 256 -c - >/dev/null) || { echo "$tgz doesn't match SHA256SUMS" >&2; exit 1; }
  mkdir "$work/$2"
  tar -xzf "$work/$tgz" -C "$work/$2" --strip-components 1
}
fetch "$OLD" old
fetch "$ILL" ill

# A fresh clone with the app in /Applications and both old daemons in /tmp.
fresh() {
  v down >/dev/null
  v up >/dev/null
  v push "$DMG" /tmp/arugula.dmg
  vs 'set -e; hdiutil attach -nobrowse -quiet -mountpoint /tmp/d /tmp/arugula.dmg; cp -R /tmp/d/Arugula.app /Applications/; hdiutil detach -quiet /tmp/d'
  vs 'mkdir -p /tmp/old /tmp/ill'
  v push "$work/old/illogicald" "$work/old/illogical" /tmp/old/
  v push "$work/ill/illogicald" "$work/ill/illogical" /tmp/ill/
  vs 'xattr -dr com.apple.quarantine /tmp/old /tmp/ill 2>/dev/null; chmod +x /tmp/old/* /tmp/ill/*; true'
}
fresh

# With the local token, which 0.21.0 asks for (0.8.0 has none), from either state directory (after
# the takeover arugula is a link to illogical: one file, read once).
answers() { vs 'curl -s -H "Authorization: Bearer $(cat ~/.local/state/arugula/local-token 2>/dev/null || cat ~/.local/state/illogical/local-token 2>/dev/null)" http://127.0.0.1:7681/api/host' | python3 -c 'import json, sys; print(json.load(sys.stdin).get("version", ""))' 2>/dev/null || true; }
# What 7681 says, raw, for a failure's message.
raw() { vs 'curl -s -i -m 5 -H "Authorization: Bearer $(cat ~/.local/state/arugula/local-token 2>/dev/null || cat ~/.local/state/illogical/local-token 2>/dev/null)" http://127.0.0.1:7681/api/host | head -1; ls ~/.local/state 2>&1 | tr "\n" " "' 2>&1 || true; }
answers_old() { [ "$(answers)" = "$OLD" ]; }
nothing_answers() { [ -z "$(answers)" ]; }
said_why() { vs "grep -q 'arugulad here is $OLD; this app needs 0.19.0 or newer' /tmp/app.log"; }
quit_app() { vs 'osascript -e "quit app \"arugula\"" 2>/dev/null; sleep 2; pkill -x arugula-desktop; : >/tmp/app.log; true'; }
start_app() { vs '(nohup /Applications/Arugula.app/Contents/MacOS/arugula-desktop >>/tmp/app.log 2>&1 &)'; }
# The older daemon as the service, as install.sh leaves it.
install_old() {
  vs '/tmp/old/illogicald install >/dev/null'
  wait_for 30 answers_old || fail "$1" "$OLD didn't answer after its install ($(answers))"
}
setup_page() {
  if wait_for 60 said_why; then
    pass "$1" "the setup page says $OLD runs and 0.19.0 is needed"
  else
    fail "$1" "the app never said $OLD is too old: $(vs 'grep arugula /tmp/app.log | tail -3')"
  fi
  # Long enough for an update, if anything started one.
  sleep 20
  if answers_old; then
    pass "$1" "$OLD still answers: the app didn't update it"
  else
    fail "$1" "the daemon became '$(answers)'; the app must leave it alone (#392)"
  fi
}

claim_stopped() {
  quit_app
  install_old stopped
  vs 'launchctl bootout gui/$(id -u)/illogicald 2>/dev/null; true'
  wait_for 15 nothing_answers || fail stopped "$OLD still answers after launchctl bootout"
  start_app
  wait_for 60 answers_old || fail stopped "the app didn't start the stopped $OLD service"
  setup_page stopped
}

claim_running() {
  quit_app
  install_old running
  start_app
  setup_page running
}

answers_ill() { [ "$(answers)" = "$ILL" ]; }
said_ill() { vs "grep -q 'The daemon here is illogicald $ILL, from before Arugula' /tmp/app.log"; }
bundled_version() { vs '/Applications/Arugula.app/Contents/MacOS/arugulad --version' | awk '{ print $NF }'; }
pane_pid() { vs "pgrep -f '[>] /tmp/count; sleep'" | head -1 || true; }
count() { vs 'cat /tmp/count' 2>/dev/null || echo 0; }
# illogicald as its install script left it, from a fresh clone.
install_ill() {
  fresh
  vs '/tmp/ill/illogicald install >/dev/null'
  wait_for 30 answers_ill || fail "$1" "illogicald $ILL didn't answer after its install ($(raw))"
}
# The setup page offers the update and nothing changes until it's pressed.
offered() {
  if wait_for 60 said_ill; then
    pass "$1" "the app says illogicald $ILL is from before the rename"
  else
    fail "$1" "the app never said so: $(vs 'grep arugula /tmp/app.log | tail -3')"
    return 1
  fi
  sleep 10
  if answers_ill && vs 'launchctl print gui/$(id -u)/illogicald >/dev/null 2>&1 && ! test -e ~/Library/LaunchAgents/arugulad.plist'; then
    pass "$1" "illogicald $ILL still answers under its own agent: nothing updated unasked"
  else
    fail "$1" "the daemon became '$(answers)' unasked ($(raw))"
  fi
}
# *Update arugulad*: the app's own arugulad takes over.
update() {
  local new; new=$(bundled_version)
  local how
  how=$(vs 'osascript -e "tell application \"System Events\" to set frontmost of process \"arugula-desktop\" to true" -e "delay 1" -e "tell application \"System Events\" to key code 36"' 2>&1) \
    || { fail "$1" "pressing Return for Update arugulad: $how"; return 1; }
  if wait_for 120 eval '[ "$(answers)" = "$new" ]'; then
    pass "$1" "Update arugulad brought up arugulad $new"
  else
    fail "$1" "after Update arugulad the daemon is '$(answers)', not $new: $(vs 'grep arugula /tmp/app.log | tail -3')"
    return 1
  fi
  if vs 'launchctl print gui/$(id -u)/illogicald >/dev/null 2>&1'; then
    fail "$1" "the illogicald agent is still loaded"
  else
    pass "$1" "the illogicald agent is gone"
  fi
}

claim_illogical_running() {
  install_ill illogical_running
  vs "\$HOME/.local/bin/illogical run --session work \"i=0; while :; do i=\\\$((i+1)); echo \\\$i > /tmp/count; sleep 0.2; done\"" >/dev/null
  wait_for 10 vs 'test -s /tmp/count' || true
  local pane0; pane0=$(pane_pid)
  start_app
  offered illogical_running || return 0
  update illogical_running || return 0
  local pane1 c0; pane1=$(pane_pid); c0=$(count); sleep 2
  if [ -n "$pane0" ] && [ "$pane0" = "$pane1" ] && [ "$(count)" -gt "$c0" ]; then
    pass illogical_running "the pane is the same process ($pane0), still counting"
  else
    fail illogical_running "the pane was ${pane0:-none}, now ${pane1:-none}, count $c0 then $(count)"
  fi
}

claim_illogical_stopped() {
  install_ill illogical_stopped
  vs 'launchctl bootout gui/$(id -u)/illogicald 2>/dev/null; true'
  wait_for 15 nothing_answers || fail illogical_stopped "illogicald still answers after launchctl bootout"
  start_app
  offered illogical_stopped || return 0
  update illogical_stopped || true
}

for c in "${claims[@]}"; do
  "claim_$c"
done
exit $failed
