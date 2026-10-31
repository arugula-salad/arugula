#!/usr/bin/env bash
# S46 (#382): put the harness on three real https origins for the
# click-through sheet (TEST-SHEET.md), with Cloudflare quick tunnels: no
# account, no DNS, no cost, and gone when this stops (Ctrl-C).
#
#   spikes/s46-passkey-approver/harness/tunnel.sh
#
# Prints the RP's, the app's and the stranger's URLs (and QR codes, with
# qrencode). trycloudflare.com is on the Public Suffix List, so the three
# hosts are three different sites: the app reaches the RP's passkeys only
# through /.well-known/webauthn, as app.arugula.io would reach
# control.arugula.io's. There's no common parent to try option (b) on; the
# README says why that's fine.
#
# PORT (default 8480): the local port. TEAM: an Apple Team ID, to serve
# apple-app-site-association for a WKWebView test. CLOUDFLARED: the binary.
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
port=${PORT:-8480}
cf=${CLOUDFLARED:-cloudflared}
work=$(mktemp -d)
pids=()
trap 'kill "${pids[@]}" 2>/dev/null || true' EXIT

for role in rp app stranger; do
  "$cf" tunnel --no-autoupdate --url "http://127.0.0.1:$port" >"$work/$role.log" 2>&1 &
  pids+=($!)
done
hosts=()
for role in rp app stranger; do
  h=
  for _ in $(seq 60); do
    h=$(grep -oE 'https://[a-z0-9-]+\.trycloudflare\.com' "$work/$role.log" | head -n1 || true)
    [ -n "$h" ] && break
    sleep 1
  done
  if [ -z "$h" ]; then
    echo "tunnel.sh: no tunnel for $role:" >&2
    cat "$work/$role.log" >&2
    exit 1
  fi
  hosts+=("${h#https://}")
done

args=(--rp "${hosts[0]}" --app "${hosts[1]}" --stranger "${hosts[2]}" --http "$port" --log "$work/log.json")
[ -n "${TEAM:-}" ] && args+=(--team "$TEAM")
node "$here/serve.mjs" "${args[@]}" >"$work/serve.log" 2>&1 &
pids+=($!)

i=0
for role in RP app stranger; do
  url="https://${hosts[$i]}/"
  echo "$role: $url"
  if command -v qrencode >/dev/null; then qrencode -t ansiutf8 "$url"; fi
  i=$((i + 1))
done
echo "(the log of passkeys made is $work/log.json; requests in $work/serve.log)"
wait
