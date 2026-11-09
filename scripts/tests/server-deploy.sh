#!/usr/bin/env bash
# What scripts/ci-server-deploy keeps of the deploy scripts it replaced
# (#1983): the platform's Postgres-secret and private-address preflight, an
# older release leaving a newer one alone, the hub's control.json check, and
# chant's exit code. Against a fake chant and a fake flyctl.
#
#   scripts/tests/server-deploy.sh
set -euo pipefail

src=$(cd "$(dirname "$0")/../.." && pwd)
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
fail=0
bad() { printf 'FAIL  %s\n' "$*"; fail=1; }
ok() { printf 'ok    %s\n' "$*"; }

mkdir "$tmp/bin"
# flyctl: the app's secrets' names and its addresses, from $FAKE.
cat >"$tmp/bin/flyctl" <<'FLY'
#!/usr/bin/env bash
case "$1 $2" in
  "secrets list") [ -z "${FAKE_SECRETS_FAIL:-}" ] || exit 1; cat "$FAKE/secrets" ;;
  "ips list") cat "$FAKE/ips" ;;
  *) echo "fake flyctl: $*" >&2; exit 1 ;;
esac
FLY
# chant: `release list` from $FAKE/releases; `run` records its arguments and
# exits $FAKE_RUN_EXIT.
cat >"$tmp/bin/chant" <<'CHANT'
#!/usr/bin/env bash
case "$1" in
  release) cat "$FAKE/releases" ;;
  run) echo "run ${*:2}" >>"$FAKE/calls"; exit "${FAKE_RUN_EXIT:-0}" ;;
  *) echo "fake chant: $*" >&2; exit 1 ;;
esac
CHANT
cat >"$tmp/bin/curl" <<'CURL'
#!/usr/bin/env bash
cat "$FAKE/control.json"
CURL
chmod +x "$tmp/bin/flyctl" "$tmp/bin/chant" "$tmp/bin/curl"
export PATH=$tmp/bin:$PATH FAKE=$tmp FLY_API_TOKEN=token ARUGULA_SERVER_CHANT=chant \
  ARUGULA_DEPLOY_CHECK_TRIES=1 ARUGULA_DEPLOY_CHECK_WAIT=0
unset GITHUB_STEP_SUMMARY
cd "$tmp"

# releases LABEL COMPONENTS: chant's release list, LABEL running COMPONENTS in prod.
releases() {
  jq -n --arg l "$1" --arg c "$2" '{releases: [{label: $l, environments: [{env: "prod", components: ($c | split(","))}]}]}' >"$tmp/releases"
}
reset() {
  echo '[{"Name": "ARUGULA_PLATFORM_DATABASE_URL"}, {"Name": "OTHER"}]' >"$tmp/secrets"
  echo '[{"Type": "private_v6", "Address": "fdaa::1"}]' >"$tmp/ips"
  echo '{"control": true}' >"$tmp/control.json"
  releases server-1 control,platform
  : >"$tmp/calls"
  unset FAKE_SECRETS_FAIL FAKE_RUN_EXIT
}
# run ARGS...: the script's exit code in $code, its output in $tmp/out.
run() { code=0; "$src/scripts/ci-server-deploy" "$@" >"$tmp/out" 2>&1 || code=$?; }
ran() { grep -q "^run --components $1 --env prod --release $2" "$tmp/calls"; }

reset
run platform server-2
if [ "$code" = 0 ] && ran platform server-2; then ok "the platform deploys with its Postgres secret and a private address"; else bad "a good platform deploy: exit $code, calls: $(cat "$tmp/calls")"; cat "$tmp/out"; fi

reset
echo '[{"Name": "OTHER"}]' >"$tmp/secrets"
run platform server-2
if [ "$code" = 1 ] && [ ! -s "$tmp/calls" ] && grep -q "no ARUGULA_PLATFORM_DATABASE_URL secret" "$tmp/out"; then ok "no Postgres secret: nothing deploys"; else bad "no Postgres secret: exit $code, calls: $(cat "$tmp/calls")"; cat "$tmp/out"; fi

reset
echo '[{"Type": "private_v6", "Address": "fdaa::1"}, {"Type": "v4", "Address": "1.2.3.4"}]' >"$tmp/ips"
run platform server-2
if [ "$code" = 1 ] && [ ! -s "$tmp/calls" ] && grep -q "public address: 1.2.3.4" "$tmp/out"; then ok "a public address: nothing deploys"; else bad "a public address: exit $code, calls: $(cat "$tmp/calls")"; cat "$tmp/out"; fi

reset
FAKE_SECRETS_FAIL=1 run platform server-2
if [ "$code" = 0 ] && ran platform server-2 && grep -q "wouldn't list arugula-platform's secrets" "$tmp/out"; then ok "secrets fly won't list: a warning, and it deploys"; else bad "secrets fly won't list: exit $code"; cat "$tmp/out"; fi

reset
echo '[{"Name": "OTHER"}]' >"$tmp/secrets"
run platform-dump server-2
if [ "$code" = 0 ] && ran platform-dump server-2; then ok "the dump's Machine alone has no preflight"; else bad "the dump alone: exit $code"; cat "$tmp/out"; fi

reset
releases server-3 control
run control server-2
if [ "$code" = 0 ] && [ ! -s "$tmp/calls" ] && grep -q "control runs server-3, newer than server-2" "$tmp/out"; then ok "an older release leaves a newer one alone"; else bad "an older release: exit $code, calls: $(cat "$tmp/calls")"; cat "$tmp/out"; fi

reset
releases server-3 control
run platform server-2
if [ "$code" = 0 ] && ran platform server-2; then ok "a newer release of another component doesn't hold this one back"; else bad "newer elsewhere: exit $code"; cat "$tmp/out"; fi

reset
run control server-2 --deploy-unchanged
if [ "$code" = 0 ] && ran control "server-2 --deploy-unchanged" && grep -q "answers as control" "$tmp/out"; then ok "the hub deploys, flags passed on, and control.json is checked"; else bad "the hub: exit $code, calls: $(cat "$tmp/calls")"; cat "$tmp/out"; fi

reset
echo '{"control": false}' >"$tmp/control.json"
run control server-2
if [ "$code" = 1 ] && grep -q "didn't answer as control" "$tmp/out"; then ok "a hub that doesn't answer as control fails the job"; else bad "control.json wrong: exit $code"; cat "$tmp/out"; fi

reset
FAKE_RUN_EXIT=3 run control server-2
if [ "$code" = 3 ] && grep -q "exited 3" "$tmp/out"; then ok "chant's exit code is the job's"; else bad "chant failing: exit $code"; cat "$tmp/out"; fi

exit "$fail"
