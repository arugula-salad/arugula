#!/usr/bin/env bash
# Tests scripts/ci-hosted-template's dry run (M266, Arugula #201): a
# release builds one E2B template per workspace size from one context,
# `arugula-workspace` (small: 2 vCPU, 2048 MiB) and
# `arugula-workspace-large` (8 vCPU, 8192 MiB), each tagged with the
# release; ARUGULA_TEMPLATE_SIZES builds only some, and a size that isn't
# one stops it before anything is built.
#
# `node`, `npm` and `file` are stand-ins on PATH, so it needs no network,
# no E2B key and no static build: the stand-in node says what it was asked.
# Where packaging/hosted's SDK is installed (npm ci) and node is real, the
# real template.mjs's dry run is checked too.
#
#   scripts/tests/hosted-template.sh
set -euo pipefail

root=$(cd "$(dirname "$0")/../.." && pwd)
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
fail=0
bad() { printf 'FAIL  %s\n' "$*"; fail=1; }
ok() { printf 'ok    %s\n' "$*"; }

real_node=$(command -v node || true)
mkdir -p "$work/bin" "$work/dist"
# shellcheck disable=SC2016 # the stand-in's own $1
printf '#!/bin/sh\necho "$1: ELF 64-bit LSB executable, x86-64, statically linked"\n' >"$work/bin/file"
printf '#!/bin/sh\nexit 0\n' >"$work/bin/npm"
# template.mjs CONTEXT NAME TAG --cpus N --memory-mb M --dry-run: one line.
cat >"$work/bin/node" <<'EOF'
#!/bin/sh
shift
echo "$*" >>"$NODE_LOG"
echo "{\"name\":\"$2\",\"tag\":\"$3\"}"
EOF
chmod +x "$work/bin/"*
touch "$work/dist/arugulad" "$work/dist/arugula"

dry() {
  : >"$work/node.log"
  # The stand-in SDK: sdk() installs nothing when it's there.
  env PATH="$work/bin:$PATH" NODE_LOG="$work/node.log" "$@" \
    "$root/scripts/ci-hosted-template" dry-run v0.0.1 "$work/dist" >"$work/out" 2>"$work/err"
}

dry env
said=$(sed "s|^[^ ]* ||" "$work/node.log")
want="arugula-workspace v0.0.1 --cpus 2 --memory-mb 2048 --dry-run
arugula-workspace-large v0.0.1 --cpus 8 --memory-mb 8192 --dry-run"
if [ "$said" = "$want" ]; then
  ok "a dry run builds both sizes: arugula-workspace 2/2048 and arugula-workspace-large 8/8192"
else
  bad "a dry run asked: $said"
fi
grep -qF 'would build arugula-workspace-large:v0.0.1 (large: 8 vCPU, 8192 MiB) from:' "$work/err" ||
  bad "it doesn't say what it would build: $(cat "$work/err")"
[ "$(wc -l <"$work/out" | tr -d ' ')" = 2 ] || bad "not one answer per template: $(cat "$work/out")"

dry env ARUGULA_TEMPLATE_SIZES=large ARUGULA_TEMPLATE_NAME=arugula-workspace-test
said=$(sed "s|^[^ ]* ||" "$work/node.log")
if [ "$said" = "arugula-workspace-test-large v0.0.1 --cpus 8 --memory-mb 8192 --dry-run" ]; then
  ok "ARUGULA_TEMPLATE_SIZES builds only some, under a test's name"
else
  bad "large alone asked: $said"
fi

if dry env ARUGULA_TEMPLATE_SIZES="small huge"; then
  bad "a size that isn't one was built"
elif [ -s "$work/node.log" ]; then
  bad "a size that isn't one stopped it only after building: $(cat "$work/node.log")"
elif grep -qF 'no such size: huge' "$work/err"; then
  ok "a size that isn't one stops it before anything is built"
else
  bad "a size that isn't one: $(cat "$work/err")"
fi

# The real template.mjs, where its SDK is installed.
here=$root/packaging/hosted
if [ -n "$real_node" ] && [ -d "$here/node_modules/e2b" ] && command -v jq >/dev/null; then
  mkdir -p "$work/ctx"
  cp "$here/Dockerfile" "$here/ready" "$here/start" "$here/arugulad.service" "$here/arugulad.json" "$here/tools" "$work/ctx"/
  cp "$work/dist/arugulad" "$work/dist/arugula" "$work/ctx"/
  got=$(env -u E2B_API_KEY "$real_node" "$here/template.mjs" "$work/ctx" arugula-workspace-large v0.0.1 \
    --cpus 8 --memory-mb 8192 --dry-run | jq -c '[.name, .tag, .cpuCount, .memoryMB, (.steps | type)]')
  if [ "$got" = '["arugula-workspace-large","v0.0.1",8,8192,"object"]' ]; then
    ok "template.mjs's dry run says the size it builds"
  else
    bad "template.mjs's dry run: $got"
  fi
  if "$real_node" "$here/template.mjs" "$work/ctx" x v1 --cpus two --dry-run >/dev/null 2>&1; then
    bad "template.mjs took --cpus two"
  else
    ok "template.mjs takes only whole numbers"
  fi
else
  echo "skip  template.mjs itself (no node, jq or packaging/hosted/node_modules: npm ci --prefix packaging/hosted)"
fi

exit "$fail"
