#!/usr/bin/env bash
#
# iTerm2 in a fresh tart VM, driven over AppleScript from ssh (no person,
# no GUI on the host). Called by `testnet/macos/test.sh iterm2 [claim...]`.
#
# M5, `arugula tmux -CC` as iTerm2's tmux:
#   attach  iTerm2 runs `arugula tmux -CC` and opens a native window for
#           the daemon's tab; text written into that iTerm2 session runs
#           in the daemon's pane; what the pane prints shows in iTerm2
#   split   splitting the session in iTerm2 adds a pane to the daemon's tab
#   tab     a tab the daemon opens shows up as an iTerm2 tab
# M32, OSC 52 from the TUI to a real terminal's clipboard:
#   copy    `arugula tui` in iTerm2 copies the line above the prompt in
#           copy mode (Ctrl-] [, k, V, y) and the Mac's clipboard (pbpaste)
#           has it
#
# BREAK=1: iTerm2 runs `arugula ls` (and waits) instead of `tmux -CC`, the session
# checks act on a plain shell window instead of the tmux one, and iTerm2's
# clipboard access is off; every check then fails. BREAK=both runs the
# normal pass, then the BREAK=1 pass in the same VM, with fresh daemons
# and iTerm2 started again: it passes when every check held in the first
# and none in the second.
#
# ARUGULA_MACOS_UP=1: the VM is already up (test.sh tools); use it and
# leave it up.
#
# The VM's sshd is pre-authorized for Apple Events to iTerm2 (the image's
# TCC database already allows it for Safari and System Events; SIP is off
# there). iTerm2 comes from iterm2.com's latest stable zip.
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$HERE/../.." && pwd)"
V="$HERE/vm.sh"
VM="${ARUGULA_MACOS_VM:-arugula-macos}"
BIN="${ARUGULA_MACOS_BIN:-$ROOT/target/debug}"
ALL="attach split tab copy"
claims="${*:-$ALL}"
for c in $claims; do case " $ALL " in *" $c "*) ;; *) echo "unknown claim $c ($ALL)" >&2; exit 2 ;; esac; done
case "${BREAK:-}" in "") passes=normal ;; both) passes="normal break" ;; *) passes="break" ;; esac

v() { "$V" "$1" "$VM" "${@:2}"; }
osa() { v ssh osascript -; }
# Every check that failed, and every one that held, in this pass.
failed=
held=
check() {
  if [ "$3" = "$4" ]; then
    echo "[macos iterm2 $1] ok: $2"; held="$held $1"
  else
    echo "[macos iterm2 $1] FAIL: $2: wanted '$4', got '$3'" >&2; failed=1
  fi
}
want() { case " $claims " in *" $1 "*) return 0 ;; esac; return 1; }

if [ -z "${ARUGULA_MACOS_UP:-}" ]; then
  v down >/dev/null
  v up >/dev/null
  [ -n "${KEEP:-}" ] || trap 'v down >/dev/null' EXIT
fi

v ssh "set -e
  db=~/Library/Application\\ Support/com.apple.TCC/TCC.db
  for c in /usr/libexec/sshd-keygen-wrapper /usr/bin/osascript; do
    sqlite3 \"\$db\" \"insert or replace into access (service,client,client_type,auth_value,auth_reason,auth_version,indirect_object_identifier_type,indirect_object_identifier,flags,last_modified) values ('kTCCServiceAppleEvents','\$c',1,2,0,1,0,'com.googlecode.iterm2',0,cast(strftime('%s','now') as integer))\"
  done
  curl -sSL -o /tmp/iterm.zip https://iterm2.com/downloads/stable/latest
  ditto -x -k /tmp/iterm.zip /Applications
  /System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister -f /Applications/iTerm.app
  defaults write com.googlecode.iterm2 SUEnableAutomaticChecks -bool false
  mkdir -p /tmp/arugula"
v push "$BIN/arugulad" "$BIN/arugula" /tmp/arugula/
v ssh 'chmod 755 /tmp/arugula/*'
I() { v ssh "/tmp/arugula/arugula --socket /tmp/ill-cc/sock $*"; }

# One pass of the claims, BREAK=1's when $1 is break.
run() {
  local brk=; [ "$1" = normal ] || brk=1
  local clip=true; [ -z "$brk" ] || clip=false
  # Two daemons: one for iTerm2's tmux, one with a single pane for the TUI.
  # Each pass starts both afresh, so a break pass can't find what the
  # normal pass left: the old ones' end closes their tmux and TUI windows.
  # iTerm2 stays up (killed and opened again, it hangs on what it
  # restores), and reads its clipboard setting at each OSC 52.
  # shellcheck disable=SC2016 # expands there
  v ssh 'pkill -f "^/tmp/arugula/arugulad"
    for _ in $(seq 1 50); do pgrep -f "^/tmp/arugula/arugulad" >/dev/null || break; sleep 0.2; done
    pkill -9 -f "^/tmp/arugula/arugulad"; rm -rf /tmp/ill-cc /tmp/ill-tui
    defaults write com.googlecode.iterm2 AllowClipboardAccess -bool '"$clip"'
    for d in cc tui; do
      (PS1="$ " nohup /tmp/arugula/arugulad --listen 127.0.0.1:0 --state-dir /tmp/ill-$d --shell "bash --norc --noprofile" >/tmp/illd-$d.log 2>&1 &)
    done
    for _ in $(seq 1 50); do [ -S /tmp/ill-cc/sock ] && [ -S /tmp/ill-tui/sock ] && break; sleep 0.2; done
    pgrep -x iTerm2 >/dev/null || open -a /Applications/iTerm.app
    for _ in $(seq 1 50); do osascript -e "tell application \"iTerm\" to count windows" >/dev/null 2>&1 && break; sleep 0.5; done'
  tmux_windows() { echo 'tell application "iTerm" to count (windows whose name starts with "↣ tmux")' | osa; }
  for _ in $(seq 1 20); do [ "$(tmux_windows)" = 0 ] && break; sleep 0.5; done

  local cc="/tmp/arugula/arugula --socket /tmp/ill-cc/sock tmux -CC"
  # A command that ends at once gets iTerm2's modal "ended very soon"
  # alert, which blocks every AppleScript after it: ls, then wait.
  [ -z "$brk" ] || cc="/bin/sh -c '/tmp/arugula/arugula --socket /tmp/ill-cc/sock ls; exec sleep 3600'"
  # A plain shell window (BREAK=1 acts on it), then the tmux gateway.
  local plain
  plain=$(echo 'tell application "iTerm" to id of (create window with default profile)' | osa)
  osa <<EOF >/dev/null
tell application "iTerm"
  create window with default profile command "$cc"
end tell
EOF
  # The window iTerm2 opens for the daemon's tab is named "↣ tmux ...".
  local target='(first window whose name starts with "↣ tmux")'
  [ -z "$brk" ] || target="(window id $plain)"
  for _ in $(seq 1 40); do [ "$(tmux_windows)" -ge 1 ] 2>/dev/null && break; sleep 0.5; done

  if want attach; then
    check attach "a native window for the daemon's tab" "$(tmux_windows)" 1
    echo "tell application \"iTerm\" to tell current session of $target to write text \"echo ITERM-\$((6*7))\"" | osa >/dev/null || true
    sleep 2
    check attach "typed in iTerm2, ran in the pane" "$(I capture %1 | grep -c '^ITERM-42$' || true)" 1
    I send %1 "'echo FROM-DAEMON-\$((7*7))
'" >/dev/null || true
    sleep 2
    check attach "the pane's output in iTerm2" "$(echo "tell application \"iTerm\" to get contents of current session of $target" | osa | grep -c '^FROM-DAEMON-49' || true)" 1
  fi

  if want split; then
    local before
    before=$(I ls | wc -l | tr -d ' ')
    echo "tell application \"iTerm\" to tell current session of $target to split vertically with default profile" | osa >/dev/null || true
    sleep 3
    check split "a split in iTerm2 is a pane in the tab" "$(I ls | grep -c ' @1 ' || true)" "$((before + 1))"
  fi

  if want tab; then
    tabs() { echo "tell application \"iTerm\" to count tabs of $target" | osa; }
    local before
    before=$(tabs)
    I run -- bash --norc --noprofile >/dev/null
    sleep 3
    check tab "the daemon's new tab is an iTerm2 tab" "$(tabs)" "$((before + 1))"
  fi

  if want copy; then
    v ssh 'echo nothing-yet | pbcopy'
    local tui
    tui=$(echo 'tell application "iTerm" to id of (create window with default profile command "env TERM=xterm-256color /tmp/arugula/arugula --socket /tmp/ill-tui/sock tui")' | osa || true)
    tui="(window id $tui)"
    sleep 4
    osa <<EOF >/dev/null || true
tell application "iTerm"
  tell current session of $tui
    write text "echo TUI-COPY-\$((8*8))"
    delay 1
    write text (ASCII character 29) newline NO
    write text "[" newline NO
    delay 0.5
    write text "k" newline NO
    write text "V" newline NO
    write text "y" newline NO
  end tell
end tell
EOF
    sleep 2
    check copy "copy mode's line on the Mac's clipboard" "$(v ssh pbpaste || true)" TUI-COPY-64
  fi
}

status=0
for p in $passes; do
  failed=""; held=""
  [ "$passes" = normal ] || echo "[macos iterm2] the $p pass"
  run "$p"
  if [ "$p" = normal ] || [ "$passes" = break ]; then
    # BREAK=1 alone, as the testnet's: every check must say FAIL.
    [ -z "$failed" ] || status=1
  elif [ -n "$held" ]; then
    echo "[macos iterm2 break] FAIL: held with BREAK=1:$held" >&2; status=1
  else
    echo "[macos iterm2 break] ok: every check failed with BREAK=1"
  fi
done
exit $status
