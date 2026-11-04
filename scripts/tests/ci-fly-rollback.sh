#!/usr/bin/env bash
# A deploy that doesn't come up healthy goes back to the release it replaced
# (#723): scripts/ci-control-deploy and scripts/ci-platform-deploy against a fake
# flyctl whose machines run what it last deployed, unhealthy when that's $FAKE_BAD.
#
#   scripts/tests/ci-fly-rollback.sh
set -euo pipefail

src=$(cd "$(dirname "$0")/../.." && pwd)
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
fail=0
bad() { printf 'FAIL  %s\n' "$*"; fail=1; }
ok() { printf 'ok    %s\n' "$*"; }

mkdir "$tmp/bin"
cat >"$tmp/bin/flyctl" <<'FLY'
#!/usr/bin/env bash
image=$(cat "$FAKE/image")
case "$1" in
  status)
    state=passing
    if [ "$image" = "${FAKE_BAD:-}" ] || [ -n "${ARUGULA_FAKE_ALL_BAD:-}" ]; then state=critical; fi
    jq -n --arg t "$image" --arg s "$state" '{Machines: [{id: "m1", state: "started", checks: [{status: $s}], image_ref: {registry: "registry.fly.io", repository: "app", tag: $t}, config: {metadata: {fly_process_group: "app"}}}]}'
    ;;
  deploy)
    printf 'deploy %s\n' "${*:2}" >>"$FAKE/calls"
    if [ -n "${FAKE_DEPLOY_REFUSES:-}" ]; then exit 1; fi
    label='' img=''
    while [ "$#" != 0 ]; do
      case "$1" in
        --image-label) label=$2 ;;
        --image) img=${2##*:} ;;
      esac
      shift
    done
    new=${label:-$img}
    echo "$new" >"$FAKE/image"
    # As flyctl does: a release that fails its health check fails the deploy.
    if [ "$new" = "${FAKE_BAD:-}" ] && [ -n "${FAKE_DEPLOY_FAILS:-}" ]; then exit 1; fi
    ;;
  auth) ;;
  secrets) echo '[{"Name": "ARUGULA_PLATFORM_DATABASE_URL"}]' ;;
  ips) echo '[{"Type": "private_v6"}]' ;;
  machines) echo '[]' ;;
  *) echo "fake flyctl: $*" >&2; exit 1 ;;
esac
FLY
cat >"$tmp/bin/curl" <<'CURL'
#!/usr/bin/env bash
echo '{"control": true}'
CURL
cat >"$tmp/bin/docker" <<'DOCKER'
#!/usr/bin/env bash
exit 0
DOCKER
chmod +x "$tmp/bin/flyctl" "$tmp/bin/curl" "$tmp/bin/docker"
export PATH=$tmp/bin:$PATH FAKE=$tmp FLY_API_TOKEN=token ARUGULA_HUB_DEPLOY=always ARUGULA_DEPLOY_CHECK_TRIES=1 ARUGULA_DEPLOY_CHECK_WAIT=0
unset GITHUB_STEP_SUMMARY

# A checkout with the scripts and the two fly.tomls.
work=$tmp/work
mkdir -p "$work/scripts" "$work/packaging/control" "$work/packaging/platform" "$work/packaging/platform-dump" "$tmp/dir"
: >"$work/packaging/platform-dump/Dockerfile"
: >"$work/packaging/platform-dump/dump.sh"
cp "$src/scripts/ci-control-deploy" "$src/scripts/ci-platform-deploy" "$src/scripts/ci-fly-rollback" "$work/scripts/"
printf 'app = "ctl"\n[env]\n  ARUGULA_CONTROL_URL = "https://ctl.example"\n' >"$work/packaging/control/fly.toml"
printf 'app = "plat"\n' >"$work/packaging/platform/fly.toml"
: >"$work/packaging/control/Dockerfile"
: >"$work/packaging/platform/Dockerfile"
: >"$tmp/dir/arugula-control"
: >"$tmp/dir/arugula-platform"
cd "$work"

run() { # SCRIPT WHAT WANT_EXIT WANT_IMAGE WANT_DEPLOYS SAYS
  local script=$1 what=$2 want=$3 image=$4 deploys=$5 says=$6 code=0 n
  : >"$tmp/calls"
  echo v1.0.0 >"$tmp/image"
  "scripts/$script" deploy v1.0.1 "$tmp/dir" >"$tmp/out" 2>&1 || code=$?
  n=$(grep -c '^deploy' "$tmp/calls" || true)
  if [ "$code" != "$want" ]; then bad "$what: exit $code, wanted $want: $(tail -5 "$tmp/out")"
  elif [ "$(cat "$tmp/image")" != "$image" ]; then bad "$what: left on $(cat "$tmp/image"), wanted $image"
  elif [ "$n" != "$deploys" ]; then bad "$what: $n deploys, wanted $deploys: $(cat "$tmp/calls")"
  elif ! grep -qF -- "$says" "$tmp/out"; then bad "$what: said: $(tail -5 "$tmp/out")"
  else ok "$what"; fi
}

for s in ci-control-deploy ci-platform-deploy; do
  run "$s" "$s: a healthy release stays" 0 v1.0.1 1 "v1.0.1"
  FAKE_BAD=v1.0.1 run "$s" "$s: fails its check: goes back, and the job is red" 1 v1.0.0 2 "went back to"
  if grep -qF -- 'registry.fly.io/app:v1.0.0' "$tmp/calls"; then ok "$s: to the image it ran before"; else bad "$s: $(cat "$tmp/calls")"; fi
  FAKE_BAD=v1.0.1 FAKE_DEPLOY_FAILS=1 run "$s" "$s: the deploy itself fails: goes back too" 1 v1.0.0 2 "went back to"
  FAKE_BAD=v1.0.0 run "$s" "$s: what it ran before was already unhealthy: a healthy release stays" 0 v1.0.1 1 "v1.0.1"
done

# Going back doesn't take: the log says a person is needed.
: >"$tmp/calls"
echo v1.0.0 >"$tmp/image"
code=0
FAKE_BAD=v1.0.0,v1.0.1 ARUGULA_FAKE_ALL_BAD=1 scripts/ci-platform-deploy deploy v1.0.1 "$tmp/dir" >"$tmp/out" 2>&1 || code=$?
if [ "$code" = 1 ] && grep -qF 'needs a person' "$tmp/out"; then ok "going back fails too: says it needs a person"; else bad "exit $code: $(tail -4 "$tmp/out")"; fi

exit $fail
