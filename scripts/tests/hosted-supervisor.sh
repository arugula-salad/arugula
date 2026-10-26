#!/usr/bin/env bash
# Tests the hosted template's supervision of arugulad (M258, Arugula #124;
# packaging/hosted): the unit restarts it and protects it from the OOM
# killer, the template enables the unit and starts the supervisor, and the
# supervisor itself (`start`, what runs without systemd) starts a killed
# daemon again with a backoff, waits on one that already answers rather
# than doubling it, and stops the daemon when it's stopped.
#
# The supervisor runs here against a stand-in daemon, with a `sudo` that
# always fails (so it never touches this machine's systemd or OOM scores)
# and a stand-in ready check.
#
#   scripts/tests/hosted-supervisor.sh
set -euo pipefail

root=$(cd "$(dirname "$0")/../.." && pwd)
here=$root/packaging/hosted
work=$(mktemp -d)
sup=
# shellcheck disable=SC2329 # run by the trap
cleanup() {
  [ -z "$sup" ] || kill -9 "$sup" 2>/dev/null || true
  pkill -9 -f "$work/arugulad" 2>/dev/null || true
  rm -rf "$work"
}
trap cleanup EXIT
fail=0
bad() { printf 'FAIL  %s\n' "$*"; fail=1; }
ok() { printf 'ok    %s\n' "$*"; }
has() { grep -qxF -- "$2" "$1" || { bad "$(basename "$1") has no line: $2"; return 1; }; }

# The unit.
unit=$here/arugulad.service
for line in 'Type=simple' 'User=user' 'ExecStart=/usr/local/bin/arugulad --hosted' 'Restart=always' \
  'StartLimitIntervalSec=0' 'KillMode=process' 'OOMScoreAdjust=-1000' 'WantedBy=multi-user.target'; do
  has "$unit" "$line" || true
done
[ "$fail" = 0 ] && ok "arugulad.service restarts the daemon, never gives up, and protects it from the OOM killer"

# The template: the unit enabled, the supervisor the start command.
f0=$fail
has "$here/Dockerfile" 'COPY start /usr/local/bin/arugula-hosted-start' || true
has "$here/Dockerfile" 'COPY arugulad.service /etc/systemd/system/arugulad.service' || true
grep -qF 'ln -sf /etc/systemd/system/arugulad.service /etc/systemd/system/multi-user.target.wants/arugulad.service' "$here/Dockerfile" \
  || bad "the Dockerfile doesn't enable arugulad.service"
grep -qF "const START = 'arugula-hosted-start'" "$here/template.mjs" || bad "template.mjs doesn't start the supervisor"
grep -qF '"cmd": ["/usr/local/bin/arugula-hosted-start"]' "$here/arugulad.json" || bad "loam's arugulad.json doesn't start the supervisor"
# shellcheck disable=SC2016 # the script's own text
grep -qF '"$here/start" "$here/arugulad.service"' "$root/scripts/ci-hosted-template" || bad "ci-hosted-template leaves start or arugulad.service out of the context"
# shellcheck disable=SC2016 # the script's own text
grep -qF 'echo -1000 | sudo -n tee "/proc/$$/oom_score_adj"' "$here/start" || bad "start doesn't lower its oom_score_adj"
[ "$fail" = "$f0" ] && ok "the template enables the unit and starts the supervisor"

# The supervisor, against a stand-in.
bin=$work/bin
mkdir -p "$bin"
printf '#!/bin/sh\nexit 1\n' > "$bin/sudo"
printf '#!/bin/sh\n[ -e %q ]\n' "$work/answering" > "$bin/arugula-hosted-ready"
cat > "$work/arugulad" <<EOF
#!/bin/sh
echo "\$\$ \$*" >> "$work/starts"
exec sleep 300
EOF
chmod +x "$bin/sudo" "$bin/arugula-hosted-ready" "$work/arugulad"
: > "$work/starts"
# Wait up to $2 tenths of a second for `starts` to have $1 lines.
starts() {
  for _ in $(seq "$2"); do
    [ "$(wc -l < "$work/starts")" -ge "$1" ] && return 0
    sleep 0.1
  done
  return 1
}
last() { tail -1 "$work/starts" | cut -d' ' -f1; }

ARUGULAD=$work/arugulad PATH=$bin:$PATH sh "$here/start" 2> "$work/log" &
sup=$!
if starts 1 30 && grep -q -- '--hosted' "$work/starts"; then ok "starts arugulad --hosted"; else bad "never started the daemon"; fi
if grep -q "can't lower oom_score_adj" "$work/log"; then ok "says so when it can't lower the OOM score"; else
  bad "said nothing of the OOM score: $(cat "$work/log")"
fi

kill -9 "$(last)"
if starts 2 25; then ok "starts a killed daemon again within a second or so"; else bad "a killed daemon wasn't started again"; fi
kill -9 "$(last)"
if starts 3 35 && grep -q 'again in 2 s' "$work/log"; then ok "backs off while it keeps exiting"; else bad "no backoff: $(cat "$work/log")"; fi

# One already answers (left by an earlier supervisor): not doubled.
touch "$work/answering"
kill -9 "$(last)"
sleep 3
if [ "$(wc -l < "$work/starts")" = 3 ]; then ok "waits on a daemon that already answers"; else bad "started a second daemon"; fi
rm "$work/answering"
if starts 4 80; then ok "starts one once nothing answers"; else bad "never started one after the other went"; fi

daemon=$(last)
kill -TERM "$sup"
for _ in $(seq 30); do kill -0 "$sup" 2>/dev/null || break; sleep 0.1; done
if kill -0 "$sup" 2>/dev/null; then bad "TERM didn't stop the supervisor"; else ok "TERM stops the supervisor"; fi
if kill -0 "$daemon" 2>/dev/null; then bad "the daemon outlived its supervisor"; else ok "and its daemon"; fi
sup=

exit "$fail"
