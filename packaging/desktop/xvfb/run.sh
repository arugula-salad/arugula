#!/usr/bin/env bash
# Runs the desktop tests named in its arguments, in the Xvfb container
# (`just desktop-xvfb`): `join`, `app [claim...]` (Nautilus among its
# claims), `stale [claim...]` (#317), `platform` (M157). APP is the app, BIN the directory
# with the static arugulad and arugula, HOST their triple.
set -uo pipefail
here=$(cd "$(dirname "$0")" && pwd)
tests=()
for w in "$@"; do
  case "$w" in
    join | app | stale | platform) tests+=("$w") ;;
    *) [ ${#tests[@]} -gt 0 ] || { echo "no desktop test $w (join, app, stale, platform)" >&2; exit 2; }
       tests[${#tests[@]}-1]+=" $w" ;;
  esac
done
# Each test starts its own Xvfb on :99 and kills it on exit, without waiting
# for it to go. After app (Nautilus and all) it can take a while: the next
# test's Xvfb found :99's lock still held, quit, and then the old one went,
# so that test had no X server (#1076). So :99 is free before each test.
x99_free() {
  local pid
  pid=$(tr -dc 0-9 </tmp/.X99-lock 2>/dev/null) || return 0
  [ -n "$pid" ] && [ -d "/proc/$pid" ] || return 0
  # A zombie still counts as running to Xvfb's lock check: its lock goes.
  [ "$(awk '{print $3}' "/proc/$pid/stat" 2>/dev/null)" = Z ] || return 1
  rm -f /tmp/.X99-lock /tmp/.X11-unix/X99
}
free_x99() {
  local pid
  for _ in $(seq 40); do
    x99_free && return 0
    sleep 0.5
  done
  pid=$(tr -dc 0-9 </tmp/.X99-lock 2>/dev/null) || return 0
  echo "the last test's Xvfb ($pid) still holds :99 after 20s: killing it" >&2
  kill -9 "$pid" 2>/dev/null || true
  sleep 1
  rm -f /tmp/.X99-lock /tmp/.X11-unix/X99
}
failed=()
for t in "${tests[@]}"; do
  set -- $t
  name=$1; shift
  free_x99
  echo "=== $name $*"
  case "$name" in
    join) "$here/test.sh" "$APP" "$BIN/arugulad-$HOST" ;;
    app) "$here/app.sh" "$APP" "$BIN/arugulad-$HOST" "$BIN/arugula-$HOST" "$@" ;;
    stale) "$here/stale.sh" "$APP" "$@" ;;
    platform) "$here/platform.sh" "$APP" "$BIN/arugulad-$HOST" ;;
  esac || failed+=("$name")
done
[ ${#failed[@]} -eq 0 ] || { echo "failed: ${failed[*]}" >&2; exit 1; }
