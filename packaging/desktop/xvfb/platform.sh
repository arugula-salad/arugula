#!/usr/bin/env bash
# M157 (#1183) on Linux, in the Xvfb container (`just desktop-xvfb
# platform`): the app ($1, a debug build) keeps the platform page's exact
# origin in its window, its window controls work there, and anything that
# only looks like it goes to the browser. With a daemon ($2) and a
# stand-in control (fake-control.py --pages) under Openbox.
#
# Control is http://127.0.0.1:P and its /control.json says the platform
# page is http://127.0.0.2:P: another host, so neither control's origin
# nor the capability every loopback page on 127.0.0.1 gets
# (capabilities/default.json) covers it. Its window permissions are
# M157's grant (allow_control) or nothing. Every page is page.html, which
# reports to the stand-in that it loaded (so a report from a host is that
# page running in the window: ARUGULA_OPEN_LOG stands in for the browser)
# and runs the commands this script gives it.
#
# 1. The window opens on control's page (joined, signed in), which goes
#    to the platform page: it loads in the window, and nothing opened in
#    the browser.
# 2. On the platform page, the Tauri window commands the client's
#    titlebar calls work: toggleMaximize fills the screen and restores
#    it, minimize iconifies it, a drag on the page's top strip moves it
#    (the app's drag script, start_dragging) and a double-click there
#    maximizes it. A target=_blank link to it goes to the window already
#    showing it, with no second window (#1596); one to control's page
#    opens a window of ours, whose close() closes it.
# 3. Lookalikes go to the browser and the window stays on the platform
#    page: another host on its port (127.0.0.3:P), its host on another
#    port (127.0.0.2:Q), https on its host and port, and a target=_blank
#    link to another host.
# 4. A control that says no platform: the same link from control's page
#    goes to the browser.
set -euo pipefail
app=$1
daemon=$2
here=$(cd "$(dirname "$0")" && pwd)
work=$(mktemp -d)
state=$work/state
pages=$work/pages
export HOME=$work/home
mkdir -p "$state" "$HOME" "$pages/cmd"
pids=()
app_pid=
trap 'kill "${pids[@]}" $app_pid 2>/dev/null || true; [ "${ok:-}" = 1 ] || { echo "--- app"; tail -60 "$work/app.log"; echo "--- daemon"; tail -20 "$work/daemon.log"; echo "--- control"; tail -60 "$work/control.log" 2>/dev/null; echo "--- opened"; cat "$work/opened" 2>/dev/null; }' EXIT

fail() { echo "FAIL: $*" >&2; exit 1; }
wait_for() {
  local until=$((SECONDS + $1)); shift
  until "$@"; do
    [ $SECONDS -lt $until ] || return 1
    sleep 0.2
  done
}
# The app's windows (visible), by their title.
windows() { xdotool search --onlyvisible --name '^Arugula$' 2>/dev/null || true; }
window() { windows | head -1; }
has_window() { [ -n "$(window)" ]; }
windows_are() { [ "$(windows | wc -l)" -eq "$1" ]; }
# A report from the page at ORIGIN (and PATH): `reported ORIGIN WHAT [PATH]`.
reported() { grep -qE "^report ${1#*://} what=$2 origin=$1 path=${3:-/}( |\$)" "$work/control.log" 2>/dev/null; }
# Give the page at ORIGIN (and PATH) a command: `tell ORIGIN[PATH] CMD...`.
tell() {
  local at=${1#*://}; shift
  case "$at" in */*) ;; *) at=$at/ ;; esac
  local path=/${at#*/}
  # fake-control.py's name for it: host:port, then the path with / as _.
  local f=$pages/cmd/${at%%/*}${path//\//_}
  echo "$*" >"$f.tmp"
  mv "$f.tmp" "$f"
}
# Whether the log has a line matching REGEX after its first N.
logged_since() { tail -n +$(($1 + 1)) "$work/control.log" | grep -qE "$2"; }
# `call ORIGIN NAME`: the page at ORIGIN calls its window's NAME() and
# says what it did: fails, printing the error, when it threw.
call() {
  local n; n=$(wc -l <"$work/control.log")
  local want="^report ${1#*://} what=call-(ok|err) origin=$1 path=/ cmd=$2( |\$)"
  tell "$1" call "$2"
  wait_for 10 logged_since "$n" "$want" \
    || { echo "the page never said what $2() did"; return 1; }
  local line; line=$(tail -n +$((n + 1)) "$work/control.log" | grep -E "$want" | head -1)
  case "$line" in *what=call-ok*) return 0 ;; *) echo "${line##*err=}"; return 1 ;; esac
}
# `click_link ORIGIN URL`: the page at ORIGIN shows a target=_blank link
# to URL, and the mouse clicks it.
click_link() {
  tell "$1" link "$2"
  wait_for 10 grep -qF "report ${1#*://} what=link origin=$1 path=/ to=$2" "$work/control.log" || fail "the page at $1 showed no link"
  sleep 0.3
  local w; w=$(window)
  xdotool windowactivate --sync "$w" 2>/dev/null || true
  xdotool mousemove --window "$w" 250 250 click 1
}
opened() { grep -qxF "$1" "$work/opened" 2>/dev/null; }
geometry() { eval "$(xdotool getwindowgeometry --shell "$1")"; }
maximized() { geometry "$1"; [ "$WIDTH" -ge 1270 ] && [ "$HEIGHT" -ge 850 ]; }
restored() { geometry "$1"; [ "$WIDTH" -lt 1270 ]; }
iconic() { xprop -id "$1" WM_STATE 2>/dev/null | grep -qi iconic; }
placed() { geometry "$1"; [ "$X" = 100 ] && [ "$Y" = 100 ] && [ "$WIDTH" = 1000 ] && [ "$HEIGHT" = 700 ]; }
. "$here/drag.sh"

Xvfb :99 -screen 0 1280x900x24 -nolisten tcp >/dev/null 2>&1 &
pids+=($!)
export DISPLAY=:99
wait_for 10 xdpyinfo >/dev/null 2>&1 || fail "no X server"
eval "$(dbus-launch --sh-syntax)"
pids+=("$DBUS_SESSION_BUS_PID")
openbox >/dev/null 2>&1 &
pids+=($!)

python3 "$here/fake-control.py" "$work/control-port" "$work/control.log" --pages "$pages" &
pids+=($!)
wait_for 10 test -s "$work/control-port" || fail "the stand-in control didn't start"
port=$(cat "$work/control-port")
other=$(cat "$pages/other-port")
control=http://127.0.0.1:$port
platform=http://127.0.0.2:$port
echo "{\"platform\": \"$platform\", \"block_domain\": \"blocks.localhost\"}" >"$pages/control.json"

RUST_LOG=arugulad=info "$daemon" --listen 127.0.0.1:0 --state-dir "$state" --shell "bash --norc --noprofile" \
  --no-manager-env --tailscale-socket /nonexistent/sock >"$work/daemon.log" 2>&1 &
pids+=($!)
wait_for 20 test -s "$state/listen" || fail "the daemon didn't start"

# Joined to control (ARUGULA_CONTROL, for tests) and signed in there: the
# window opens on control's page.
mkdir -p "$HOME/.config/io.arugula.desktop"
echo "{\"signed_in\": {\"$control\": true}}" >"$HOME/.config/io.arugula.desktop/cloud.json"
start_app() {
  ARUGULA_STATE_DIR=$state ARUGULA_CONTROL=$control ARUGULA_OPEN_LOG=$work/opened ARUGULA_NO_DAEMON_UPGRADE=1 \
    ARUGULA_DESKTOP_SETTINGS=$work/desktop.json \
    WEBKIT_DISABLE_SANDBOX_THIS_IS_DANGEROUS=1 WEBKIT_DISABLE_COMPOSITING_MODE=1 LIBGL_ALWAYS_SOFTWARE=1 \
    "$app" >>"$work/app.log" 2>&1 &
  app_pid=$!
  wait_for 90 has_window || fail "the app's window never showed"
  wait_for 60 reported "$control" loaded || fail "the window never showed control's page"
}
stop_app() {
  kill "$app_pid" 2>/dev/null || true
  wait "$app_pid" 2>/dev/null || true
  app_pid=
}

# 1. Control's page, then the platform page in the window.
start_app
echo "ok: the window shows control's page ($control)"
wait_for 30 grep -qF "platform page is $platform/" "$work/app.log" || fail "the app never read control's platform page"
echo "ok: the app read the platform page's origin from control's /control.json: $platform"
tell "$control" go "$platform/"
wait_for 20 reported "$platform" loaded || fail "the platform page never loaded in the window"
grep -qF "report 127.0.0.2:$port what=loaded origin=$platform path=/ tauri=true" "$work/control.log" \
  || fail "the platform page has no Tauri window API"
! grep -qF "127.0.0.2" "$work/opened" 2>/dev/null || fail "it opened the platform page in the browser too: $(cat "$work/opened")"
echo "ok: control's page went to the platform page ($platform), which loaded in the window; nothing opened in the browser"

# 2. The window's controls, from the platform page.
w=$(window)
xdotool windowsize "$w" 1000 700 windowmove "$w" 100 100
wait_for 5 restored "$w" || fail "the window didn't take 1000x700"
err=$(call "$platform" toggleMaximize) || fail "toggleMaximize() on the platform page: $err"
wait_for 10 maximized "$w" || fail "toggleMaximize() said ok, but the window is ${WIDTH}x$HEIGHT"
echo "ok: on the platform page toggleMaximize() maximized the window (${WIDTH}x$HEIGHT)"
err=$(call "$platform" toggleMaximize) || fail "toggleMaximize() again: $err"
wait_for 10 restored "$w" || fail "a second toggleMaximize() left the window ${WIDTH}x$HEIGHT"
echo "ok: and again restored it (${WIDTH}x$HEIGHT)"
err=$(call "$platform" minimize) || fail "minimize() on the platform page: $err"
wait_for 10 iconic "$w" || fail "minimize() said ok, but: $(xprop -id "$w" WM_STATE | tr '\n' ' ')"
echo "ok: minimize() minimized it"
xdotool windowactivate --sync "$w" 2>/dev/null || xdotool windowmap "$w"
sleep 1
xdotool windowsize "$w" 1000 700 windowmove "$w" 100 100
wait_for 10 placed "$w" || fail "the window didn't take 1000x700 at 100,100 (${WIDTH}x$HEIGHT at $X,$Y)"
drag "$w" $((WIDTH / 2)) 12
dragged || fail "dragging the platform page's top strip left the window at $X,$Y (was $x0,$y0)"
echo "ok: dragging the platform page's top strip moved the window from $x0,$y0 to $X,$Y (start_dragging)$(pressed)"
xdotool mousemove --window "$w" $((WIDTH / 2)) 12 click --repeat 2 --delay 80 1
wait_for 10 maximized "$w" || fail "a double-click on the platform page's strip left the window ${WIDTH}x$HEIGHT"
echo "ok: a double-click on its strip maximized it (internal_toggle_maximize, ${WIDTH}x$HEIGHT)"
call "$platform" toggleMaximize >/dev/null || true

click_link "$platform" "$platform/second"
wait_for 20 reported "$platform" loaded /second || fail "a target=_blank link to the platform page loaded nowhere"
sleep 1
windows_are 1 || fail "a target=_blank link to the platform page made $(windows | wc -l) windows, not the one showing it"
! grep -qF "127.0.0.2" "$work/opened" 2>/dev/null || fail "it opened the platform page in the browser: $(cat "$work/opened")"
echo "ok: a target=_blank link to the platform page went to the window showing it (#1596)"
n=$(wc -l <"$work/control.log")
tell "$platform/second" go "$platform/"
wait_for 20 logged_since "$n" "^report 127.0.0.2:$port what=loaded origin=$platform path=/( |\$)" || fail "the window didn't go back to the platform page"
click_link "$platform" "$control/second"
wait_for 20 reported "$control" loaded /second || fail "a target=_blank link to control's page loaded nowhere"
wait_for 10 windows_are 2 || fail "a target=_blank link to control's page made $(windows | wc -l) windows"
echo "ok: a target=_blank link to control's page opened a window of ours on it"
tell "$control/second" call close
wait_for 10 windows_are 1 || fail "close() on control's second window left $(windows | wc -l) windows"
echo "ok: close() on it closed that window"

# 3. Lookalikes.
for look in "http://127.0.0.3:$port/" "http://127.0.0.2:$other/" "https://127.0.0.2:$port/"; do
  tell "$platform" go "$look"
  wait_for 15 opened "$look" || fail "$look didn't go to the browser"
done
click_link "$platform" "http://127.0.0.3:$port/popup"
wait_for 15 opened "http://127.0.0.3:$port/popup" || fail "a target=_blank link to 127.0.0.3:$port didn't go to the browser"
sleep 1
grep -E "report (127.0.0.3:$port|127.0.0.2:$other) " "$work/control.log" && fail "a lookalike's page loaded in a window"
windows_are 1 || fail "the lookalikes left $(windows | wc -l) windows"
tell "$platform" ping
wait_for 10 reported "$platform" pong || fail "the window isn't on the platform page after the lookalikes"
echo "ok: lookalikes went to the browser (another host on its port, its host on another port, https, and a target=_blank link); the window stays on the platform page"

# 4. A control that says no platform page.
stop_app
echo '{"block_domain": "blocks.localhost"}' >"$pages/control.json"
: >"$work/control.log"
: >"$work/opened"
start_app
wait_for 30 grep -q "^GET /control.json host=127.0.0.1:$port" "$work/control.log" || fail "the app didn't read control's /control.json"
tell "$control" go "$platform/"
wait_for 15 opened "$platform/" || fail "with no platform page said, $platform/ didn't go to the browser"
sleep 1
! reported "$platform" loaded || fail "with no platform page said, $platform loaded in the window"
tell "$control" ping
wait_for 10 reported "$control" pong || fail "the window left control's page"
echo "ok: with no platform in /control.json, the same link from control's page went to the browser"
ok=1
