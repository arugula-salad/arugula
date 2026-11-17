#!/usr/bin/env bash
# `scripts/release app-latest TAG` (#868): after an app release's publish, the
# rolling app-latest release is titled "Arugula app (latest)" and holds only
# what TAG's release holds, and latest.json. A fake gh keeps the releases in
# files, so nothing reaches the network.
#
#   scripts/tests/release-app-latest.sh
set -euo pipefail

release=$(cd "$(dirname "$0")/.." && pwd)/release
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
fail=0
bad() { printf 'FAIL  %s\n' "$*"; fail=1; }
ok() { printf 'ok    %s\n' "$*"; }

# The fake forge: $FAKE/tag-assets lists TAG's release's asset names;
# $FAKE/latest-assets is app-latest's, "ID<tab>NAME" a line; $FAKE/latest-name
# its title. Each write gh makes is logged in $FAKE/calls.
mkdir "$tmp/bin"
cat >"$tmp/bin/gh" <<'EOF'
#!/usr/bin/env bash
[ "$1" = api ] || { echo "fake gh: $*" >&2; exit 1; }
shift
method=GET
if [ "$1" = -X ]; then method=$2; shift 2; fi
path=$1
case "$method $path" in
  "GET repos/forge/arugula/releases/tags/app-v1.0.0") cat "$FAKE/tag-assets" ;;
  "GET repos/forge/arugula/releases/tags/app-latest") echo 42 ;;
  "GET repos/forge/arugula/releases/42") cat "$FAKE/latest-name" ;;
  "GET repos/forge/arugula/releases/42/assets") cat "$FAKE/latest-assets" ;;
  "PATCH repos/forge/arugula/releases/42")
    printf '%s\n' "$*" >>"$FAKE/calls"
    printf '%s\n' "${3#name=}" >"$FAKE/latest-name" ;;
  "DELETE repos/forge/arugula/releases/assets/"*)
    printf 'delete %s\n' "${path##*/}" >>"$FAKE/calls" ;;
  *) echo "fake gh: $method $path" >&2; exit 1 ;;
esac
EOF
chmod +x "$tmp/bin/gh"
export PATH=$tmp/bin:$PATH FAKE=$tmp GITHUB_REPOSITORY=forge/arugula

reset() {
  printf '%s\n' arugula-desktop-linux-x86_64.deb arugula-desktop-linux-x86_64.AppImage SHA256SUMS >"$tmp/tag-assets"
  printf '1\tarugula-desktop-linux-x86_64.deb\n2\tarugula-desktop-linux-x86_64.AppImage\n3\tSHA256SUMS\n4\tlatest.json\n' >"$tmp/latest-assets"
  : >"$tmp/calls"
}

# chant's swap: the files are right, the title is chant's.
reset
echo "app-latest 1.0.0" >"$tmp/latest-name"
if "$release" app-latest app-v1.0.0 >"$tmp/out" 2>&1; then ok "chant's swap: runs"; else bad "chant's swap: $(cat "$tmp/out")"; fi
if [ "$(cat "$tmp/latest-name")" = "Arugula app (latest)" ]; then ok "chant's swap: retitled"; else bad "chant's swap: titled $(cat "$tmp/latest-name")"; fi
if grep -q '^delete' "$tmp/calls"; then bad "chant's swap: deleted $(grep '^delete' "$tmp/calls")"; else ok "chant's swap: keeps every file, latest.json too"; fi

# Leftovers from before: two illogical-desktop-* files go, and a title that's
# already right isn't written again.
reset
echo "Arugula app (latest)" >"$tmp/latest-name"
printf '7\tillogical-desktop-linux-x86_64.deb\n8\tillogical-desktop-macos-arm64.dmg\n' >>"$tmp/latest-assets"
if "$release" app-latest app-v1.0.0 >"$tmp/out" 2>&1; then ok "leftovers: runs"; else bad "leftovers: $(cat "$tmp/out")"; fi
if [ "$(grep '^delete' "$tmp/calls" | tr '\n' ' ')" = "delete 7 delete 8 " ]; then ok "leftovers: only the two deleted"; else bad "leftovers: $(cat "$tmp/calls")"; fi
if grep -q PATCH "$tmp/calls"; then bad "leftovers: retitled a right title"; else ok "leftovers: the right title left alone"; fi

# TAG's release with nothing on it: fail, and delete nothing.
reset
: >"$tmp/tag-assets"
echo "app-latest 1.0.0" >"$tmp/latest-name"
if "$release" app-latest app-v1.0.0 >"$tmp/out" 2>&1; then bad "an empty tag release should fail"; else ok "an empty tag release: fails"; fi
if [ -s "$tmp/calls" ]; then bad "an empty tag release: wrote $(cat "$tmp/calls")"; else ok "an empty tag release: touches nothing"; fi

exit "$fail"
