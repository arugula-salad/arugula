#!/usr/bin/env bash
#
# M140's gate (#1157): what a platform route costs through the hub's
# gateway, against the platform in control's own process. Times a few
# routes from the host through control's published port, as a signed-in
# person (testnet/platform.ts measure), in whichever way the running stack
# has the platform (ARUGULA_TESTNET_PLATFORM=in or out at `up`):
#
#   testnet/measure-control.sh [samples]   (needs `just testnet up control`)
#
# GET /api/me is a hub route, for the floor. Each run's numbers are kept
# in testnet/.measure-<stack>/<in|out>.json, which `down` leaves, so the
# other way can be brought up and measured next. Once both are there it
# prints them side by side and fails if any platform route's p95 out of
# process is more than 20 ms over in process.
#
# Needs Docker, node and the web client's packages.
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source-path=SCRIPTDIR source=env.sh
. "$HERE/env.sh"
N="${1:-300}"
GATE_MS=20

need_docker
[ -f "$STATE/control.env" ] || { echo "no $STATE/control.env; run 'just testnet up control' first" >&2; exit 1; }
# shellcheck source=/dev/null
. "$STATE/control.env"
mode="${ARUGULA_TESTNET_PLATFORM:-in}"
out_dir="$HERE/.measure-$TESTNET"
mkdir -p "$out_dir"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT
ROOT="$(cd "$HERE/.." && pwd)"

# shellcheck disable=SC2086 # CONTROL_VIA is several words
node --experimental-strip-types --no-warnings "$ROOT/web/fixtures/device-cli.ts" --state "$WORK/device.json" \
  signin --control "$CONTROL_URL" $CONTROL_VIA --login "measure-$$" > /dev/null
node --experimental-strip-types --no-warnings "$HERE/platform.ts" --state "$WORK/device.json" measure "$N" > "$out_dir/$mode.json"
echo "platform $mode of process, $N samples each (ms):"
# shellcheck disable=SC2016 # JavaScript's ${}, not the shell's
node -e '
const r = JSON.parse(require("fs").readFileSync(process.argv[1], "utf8")).routes;
for (const [k, v] of Object.entries(r)) console.log(`  ${k.padEnd(40)} p50 ${String(v.p50).padStart(7)}  p95 ${String(v.p95).padStart(7)}  p99 ${String(v.p99).padStart(7)}`);
' "$out_dir/$mode.json"

[ -f "$out_dir/in.json" ] && [ -f "$out_dir/out.json" ] || {
  echo "measure the other way too (ARUGULA_TESTNET_PLATFORM=$([ "$mode" = in ] && echo out || echo in)) to compare"
  exit 0
}
# shellcheck disable=SC2016 # JavaScript's ${}, not the shell's
node -e '
const fs = require("fs");
const [inF, outF, gate] = process.argv.slice(1);
const a = JSON.parse(fs.readFileSync(inF, "utf8")).routes, b = JSON.parse(fs.readFileSync(outF, "utf8")).routes;
let bad = 0;
console.log("p95, in process -> out of process (ms):");
for (const k of Object.keys(a)) {
  if (!b[k]) continue;
  const d = Math.round((b[k].p95 - a[k].p95) * 100) / 100;
  const over = !k.startsWith("GET /api/me") && d > Number(gate);
  if (over) bad++;
  console.log(`  ${k.padEnd(40)} ${String(a[k].p95).padStart(7)} -> ${String(b[k].p95).padStart(7)}  (${d >= 0 ? "+" : ""}${d})${over ? "  OVER" : ""}`);
}
if (bad) { console.log(`FAIL: ${bad} route(s) over the ${gate} ms gate`); process.exit(1); }
console.log(`within the ${gate} ms gate`);
' "$out_dir/in.json" "$out_dir/out.json" "$GATE_MS"
