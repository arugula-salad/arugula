#!/usr/bin/env bash
# A chant-fly plan's ledger across a box's two users (#1985 review), on the
# hosted box's image (testnet/hosted/Dockerfile: `user` runs arugulad,
# `runner` runs runs). What arugulad does after a plan's step, with the same
# commands and the same script (crates/arugulad-work/src/run_file_read.sh):
#
# - the run's home is 0700, `runner`'s: `user` can't open the bundle there,
#   and gets it from `runner` (`sudo -n -u runner sh -c <script>`), only a
#   regular file of runner's own, not a link, at most MAX bytes;
# - a leftover of runner's that left its process group (setsid) is found by
#   `pgrep -u runner`, and `sudo -n -u runner kill -KILL -1` ends it;
# - the push's token sits in a `user` process's environment, which runner
#   can't read through /proc;
# - git pushes with the token as an extraheader from its environment only.
#
#   testnet/hosted/plan-ledger-users.sh       # builds the image, runs, cleans up
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../.." && pwd)
image=arugula-testnet-hosted-box
images=${ARUGULA_IMAGES:-ghcr.io/arugula-salad/ci}
docker build -q --build-arg "ARUGULA_IMAGES=$images" -t "$image" "$here" >/dev/null
name=arugula-plan-ledger-users-$$
# --init: a box has one (systemd), which reaps what the kill ends.
docker run -d --rm --init --name "$name" "$image" >/dev/null
trap 'docker rm -f "$name" >/dev/null 2>&1 || true' EXIT
docker cp "$root/crates/arugulad-work/src/run_file_read.sh" "$name:/opt/run_file_read.sh"
docker exec "$name" chmod 0644 /opt/run_file_read.sh

# As runner, as a plan's step leaves it: its home's work dir 0700, a bundle,
# a link to something of user's, a second name, one too large, and a
# leftover that left its group.
docker exec -u runner "$name" bash -c '
  set -e
  w=/home/runner/work/run_1; mkdir -p -m 700 "$w"; cd "$w"
  printf "a bundle\n" > ledger.bundle
  ln -s /home/user/.bashrc link.bundle
  printf x > two; ln two two-again
  head -c 2048 /dev/zero > big.bundle
  setsid sleep 300 </dev/null >/dev/null 2>&1 &
'

docker exec -u user "$name" bash -c '
  set -u
  fail() { echo "FAIL: $*" >&2; exit 1; }
  ok() { echo "ok: $*"; }
  w=/home/runner/work/run_1
  script=$(cat /opt/run_file_read.sh)
  as_runner() { sudo -n -u runner -- /bin/sh -c "$script" sh "$@"; }

  # arugulad, as user, cannot open the run'"'"'s home itself.
  if cat "$w/ledger.bundle" >/dev/null 2>&1; then fail "user read runner'"'"'s 0700 home directly"; fi
  ok "user cannot open $w/ledger.bundle itself (the blocker the review found)"

  got=$(as_runner "$w/ledger.bundle" 1024) || fail "the bundle did not come over: $?"
  [ "$got" = "a bundle" ] || fail "the bundle came over as [$got]"
  ok "runner hands the bundle over"
  as_runner "$w/link.bundle" 1024 >/dev/null; [ $? = 3 ] || fail "a link was handed over"
  as_runner "$w/two" 1024 >/dev/null; [ $? = 3 ] || fail "a file with two names was handed over"
  as_runner "$w" 1024 >/dev/null; [ $? = 3 ] || fail "a directory was handed over"
  as_runner "$w/big.bundle" 1024 >/dev/null; [ $? = 4 ] || fail "a file over the cap was handed over"
  as_runner "$w/none" 1024 >/dev/null; [ $? = 2 ] || fail "a missing file read"
  ok "a link, a second name, a directory, one too large and a missing one are refused (3, 3, 3, 4, 2)"
  names=$(sudo -n -u runner -- /bin/sh -c "$script" sh -l "$w" | sort | tr "\n" " ")
  [ "$names" = "big.bundle ledger.bundle link.bundle two two-again " ] || fail "listed [$names]"
  ok "runner lists its directory: $names"

  # The survivor check: a leftover that left its group is found, killed,
  # and then none is left.
  pgrep -u runner >/dev/null || fail "pgrep -u runner found no leftover"
  ok "pgrep -u runner finds the setsid leftover: $(pgrep -u runner | tr "\n" " ")"
  sudo -n -u runner -- kill -KILL -- -1 || fail "sudo -u runner kill -KILL -- -1 exited $?"
  ok "sudo -u runner kill -KILL -- -1 exited 0"
  for _ in $(seq 20); do pgrep -u runner >/dev/null || break; sleep 0.05; done
  pgrep -u runner >/dev/null && fail "runner still has processes after the kill"
  [ "$(pgrep -u runner >/dev/null; echo $?)" = 1 ] || fail "pgrep -u runner did not say none (1)"
  ok "after it, pgrep -u runner says none (exit 1)"
  sudo -n -u runner -- kill -KILL -- -1 || fail "with nothing left, the kill exited $?"
  ok "with nothing left, the kill exits 0 too"
  sudo -n -u nosuch -- kill -KILL -- -1 2>/dev/null && fail "a kill sudo refuses exited 0"
  ok "a kill sudo refuses exits non-zero, which fails the plan"

  # The push token in a user process'"'"'s environment, out of runner'"'"'s reach.
  env GIT_CONFIG_VALUE_0="AUTHORIZATION: basic c2VjcmV0" sleep 30 &
  pid=$!
  sleep 0.2
  if sudo -n -u runner -- cat "/proc/$pid/environ" >/dev/null 2>&1; then fail "runner read user'"'"'s environ"; fi
  if sudo -n -u runner -- cat "/proc/$pid/cmdline" | tr "\0" " " | grep -q basic; then fail "the token is on a command line"; fi
  kill $pid
  ok "runner cannot read the environment of user'"'"'s process holding the token"

  # git pushes with the header from its environment only.
  d=$(mktemp -d); cd "$d"
  git init -q --bare origin.git; git init -q -b chant/lifecycle w
  ( cd w && echo x > f && git add f && git -c user.name=t -c user.email=t@t commit -q -m one )
  GIT_CONFIG_NOSYSTEM=1 GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_COUNT=1 \
    GIT_CONFIG_KEY_0=http.https://github.com/.extraheader GIT_CONFIG_VALUE_0="AUTHORIZATION: basic c2VjcmV0" \
    git -C w push -q --porcelain "--force-with-lease=refs/heads/chant/lifecycle:" -- "$d/origin.git" \
    HEAD:refs/heads/chant/lifecycle >/dev/null || fail "the push failed"
  [ -n "$(git --git-dir origin.git rev-parse -q --verify refs/heads/chant/lifecycle)" ] || fail "nothing pushed"
  grep -rqs c2VjcmV0 "$d" && fail "the token reached a file"
  ok "git pushes with the token in its environment, and it is in no file"
'
echo "plan-ledger-users: every check held"
