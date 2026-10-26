#!/usr/bin/env bash
# S42 (prototype, not for main): the loam rig. Patches the testnet in this
# checkout (testnet/loam.yaml, one line each in up.sh and test.sh; undone
# with `git checkout testnet/ && rm testnet/loam.yaml`), brings a stack up
# with control's public URL at a cloudflared quick tunnel, and runs
# loam-claim.sh's claim.
#
#   rig.sh TUNNEL_URL TEMPLATE      TEMPLATE: an arugula-workspace build on
#                                   loam with template-s42.Dockerfile and
#                                   ENV ARUGULA_CONTROL_URL=TUNNEL_URL
#
# The tunnel must point at 127.0.0.1:$ARUGULA_TESTNET_CONTROL_PORT (24980).
# Needs ~/.config/arugula/loam-key and `just static` binaries in
# $CARGO_TARGET_DIR.
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../.." && pwd)
tunnel=${1:?usage: rig.sh TUNNEL_URL TEMPLATE} template=${2:?usage: rig.sh TUNNEL_URL TEMPLATE}
cd "$root"
python3 - "$tunnel" <<'EOF'
import sys
u = sys.argv[1]
c = open('testnet/compose.yaml').read()
i = c.index('  control:\n    profiles: [control]'); j = c.index('    command:\n', i); k = c.index('    environment:\n', j)
cmd = c[j:k].replace('--public-url=http://${ARUGULA_TESTNET_INNER_NET:-10.229.80}.10:8080', '--public-url=' + u)
assert u in cmd
open('testnet/loam.yaml', 'w').write('''# Local only (S42): the in-process platform at loam's E2B API, boxes
# enrolling by sandbox id at control's public URL, a tunnel.
services:
  control:
''' + cmd + '''    environment:
      ARUGULA_E2B_URL: https://api.e2b.loam.inevitable.fyi
      ARUGULA_E2B_API_KEY: ${LOAM_KEY:?}
      ARUGULA_HOSTED_TEMPLATE: ${LOAM_TEMPLATE:?}
      ARUGULA_HOSTED_ENROLL: sandbox
''')
p = 'testnet/up.sh'; s = open(p).read()
old = '      files="$files -f $HERE/hosted.yaml"\n'
if 'loam.yaml' not in s:
    s = s.replace(old, old + '      [ -z "${ARUGULA_TESTNET_LOAM:-}" ] || files="$files -f $HERE/loam.yaml"\n', 1); open(p, 'w').write(s)
p = 'testnet/test.sh'; s = open(p).read()
old = '  [ "${ARUGULA_TESTNET_HOSTED:-}" != 1 ] || CONTROL_CLAIMS="$CONTROL_CLAIMS hosted-setup deploys planter-deploy"\n'
if 's42-loam' not in s:
    s = s.replace(old, old + '  [ -z "${ARUGULA_TESTNET_LOAM:-}" ] || CONTROL_CLAIMS="$CONTROL_CLAIMS s42-loam"\n', 1)
    m = '# M187: a public preview, end to end (the header says what).\n'
    s = s.replace(m, '# shellcheck source=/dev/null\n. "$(dirname "$0")/../spikes/s42-thread-sessions/loam-claim.sh"\n\n' + m, 1)
    open(p, 'w').write(s)
EOF
export COMPOSE_PROJECT_NAME=s42 ARUGULA_TESTNET_SSH_PORT=24922 ARUGULA_TESTNET_CONTROL_PORT=24980 \
  ARUGULA_TESTNET_FAKES_PORT=24981 ARUGULA_TESTNET_GUEST_SSH_PORT=24982 ARUGULA_TESTNET_PLATFORM_PORT=24983 \
  ARUGULA_TESTNET_PLATFORM2_PORT=24984 ARUGULA_TESTNET_INNER_NET=10.229.142 ARUGULA_TESTNET_HOSTED=1 \
  ARUGULA_TESTNET_LOAM=1 LOAM_KEY="$(cat ~/.config/arugula/loam-key)" LOAM_TEMPLATE="$template" \
  LOAM_PUBLIC="$tunnel" S42_OUT="$here/results" \
  ARUGULA_TESTNET_BINARIES="${CARGO_TARGET_DIR:?}/x86_64-unknown-linux-musl/release"
testnet/up.sh control
testnet/test.sh control s42-loam
