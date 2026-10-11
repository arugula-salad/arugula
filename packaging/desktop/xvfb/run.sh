#!/usr/bin/env bash
# Runs the desktop tests named in its arguments, in the Xvfb container
# (`just desktop-xvfb`): `join`, `app [claim...]` (Nautilus among its
# claims), `stale [claim...]` (#317). APP is the app, BIN the directory
# with the static arugulad and arugula, HOST their triple.
set -uo pipefail
here=$(cd "$(dirname "$0")" && pwd)
tests=()
for w in "$@"; do
  case "$w" in
    join | app | stale) tests+=("$w") ;;
    *) [ ${#tests[@]} -gt 0 ] || { echo "no desktop test $w (join, app, stale)" >&2; exit 2; }
       tests[${#tests[@]}-1]+=" $w" ;;
  esac
done
failed=()
for t in "${tests[@]}"; do
  set -- $t
  name=$1; shift
  echo "=== $name $*"
  case "$name" in
    join) "$here/test.sh" "$APP" "$BIN/arugulad-$HOST" ;;
    app) "$here/app.sh" "$APP" "$BIN/arugulad-$HOST" "$BIN/arugula-$HOST" "$@" ;;
    stale) "$here/stale.sh" "$APP" "$@" ;;
  esac || failed+=("$name")
done
[ ${#failed[@]} -eq 0 ] || { echo "failed: ${failed[*]}" >&2; exit 1; }
