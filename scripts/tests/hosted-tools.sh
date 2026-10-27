#!/usr/bin/env bash
# Tests the hosted template's shared tools (Arugula #215, packaging/hosted):
# the template makes /opt/arugula/tools and puts its bin first on `user`'s
# PATH, as the daemon puts it first on a session user's; and
# `arugula-tools` (packaging/hosted/tools) publishes a tool there: a link
# for a plain binary, a wrapper for rustup's proxies, only from where every
# user can reach, and never by writing through a link already there. Its
# `env` keeps the session users' environment file, which `as` reads as the
# daemon does.
#
# It runs against a prefix of its own (ARUGULA_TOOLS) and a sudo that runs
# as this user (ARUGULA_HOSTED_SUDO), so it touches nothing of this
# machine's. Linux only: the helper is the box's, with GNU's stat and mv.
#
#   scripts/tests/hosted-tools.sh
set -euo pipefail

root=$(cd "$(dirname "$0")/../.." && pwd)
here=$root/packaging/hosted
tools=$here/tools
if [ "$(uname -s)" != Linux ]; then
  echo "skip  hosted-tools (the box's helper, Linux only)"
  exit 0
fi
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
chmod 0755 "$work"
fail=0
bad() { printf 'FAIL  %s\n' "$*"; fail=1; }
ok() { printf 'ok    %s\n' "$*"; }
has() { grep -qF -- "$2" "$1" || { bad "$(basename "$1") has no: $2"; return 1; }; }

# The template and the daemon agree on the place.
f0=$fail
rust_path=$(sed -n 's/^pub const SESSION_PATH: &str = "\(.*\)";$/\1/p' "$root/crates/daemon/src/agent/hosted.rs")
[ "$rust_path" = /opt/arugula/tools/bin:/usr/local/bin:/usr/bin:/bin ] || bad "the daemon's SESSION_PATH is $rust_path"
has "$tools" "session_path=$rust_path" || true
has "$root/crates/daemon/src/agent/hosted.rs" 'pub const SESSION_ENV: &str = "/opt/arugula/tools/env";' || true
has "$here/Dockerfile" 'COPY tools /usr/local/bin/arugula-tools' || true
has "$here/Dockerfile" 'install -d -o user -g user -m 0755 /opt/arugula/tools /opt/arugula/tools/bin' || true
has "$here/Dockerfile" '/etc/profile.d/arugula-tools.sh' || true
has "$here/arugulad.service" 'Environment=PATH=/opt/arugula/tools/bin:' || true
has "$here/start" 'PATH=/opt/arugula/tools/bin:' || true
# shellcheck disable=SC2016 # the script's own text
has "$root/scripts/ci-hosted-template" '"$here/arugulad.json" "$here/tools"' || true
[ "$fail" = "$f0" ] && ok "the template makes the tools' place and puts it first on PATH, as the daemon does for sessions"

export ARUGULA_TOOLS=$work/tools
bin=$ARUGULA_TOOLS/bin
mkdir -p "$bin"
chmod -R 0755 "$ARUGULA_TOOLS"
run() { "$tools" "$@" >"$work/out" 2>&1; }

# Where setup's tools were: a home closed to other users, and a link into
# it already in bin, as the old setup left /usr/local/bin.
closed=$work/user
mkdir -p "$closed/.cargo/bin" "$closed/zig"
printf '#!/bin/sh\necho user-cargo\n' >"$closed/.cargo/bin/cargo"
chmod 0755 "$closed/.cargo/bin/cargo"
chmod 0750 "$closed"
ln -s "$closed/.cargo/bin/cargo" "$bin/cargo"
ln -s "$closed/zig" "$bin/zig"

# Installs every user can reach.
opt=$work/opt
mkdir -p "$opt/zig-0.16.0" "$opt/cargo/bin" "$opt/rustup"
printf '#!/bin/sh\necho "zig $*"\n' >"$opt/zig-0.16.0/zig"
# rustup: its proxies are links to it; cargo install's are binaries.
cat >"$opt/cargo/bin/rustup" <<'EOF'
#!/bin/sh
echo "$(basename "$0") RUSTUP_HOME=${RUSTUP_HOME:-none}"
EOF
ln "$opt/cargo/bin/rustup" "$opt/cargo/bin/cargo"
ln -s rustup "$opt/cargo/bin/rustc"
printf '#!/bin/sh\necho nextest\n' >"$opt/cargo/bin/cargo-nextest"
chmod 0755 "$opt"/zig-0.16.0/zig "$opt"/cargo/bin/*

# A plain binary: a link, written beside the old one, not through it.
if run link zig "$opt/zig-0.16.0/zig" && [ "$(readlink "$bin/zig")" = "$opt/zig-0.16.0/zig" ] &&
  [ -z "$(ls -A "$closed/zig")" ] && [ "$("$bin/zig" version)" = "zig version" ]; then
  ok "link replaces a link to a directory with a link to the binary, and puts nothing in that directory"
else
  bad "link zig: $(cat "$work/out"); now $(readlink "$bin/zig"); in the old directory: $(ls -A "$closed/zig")"
fi
if run link closed "$closed/.cargo/bin/cargo"; then
  bad "link took a binary in a directory closed to other users"
elif grep -qF "is closed to other users" "$work/out" && [ ! -e "$bin/closed" ]; then
  ok "link refuses a binary other users can't reach"
else
  bad "link from a closed home: $(cat "$work/out")"
fi
for home in /home/user/.cargo/bin/cargo /root/.cargo/bin/cargo; do
  if [ -f "$home" ] && run link x "$home"; then bad "link took $home"; fi
done
printf '#!/bin/sh\n' >"$opt/mine"
chmod 0700 "$opt/mine"
if run link mine "$opt/mine"; then
  bad "link took a binary only its owner can run"
else
  ok "link refuses a binary other users can't run"
fi
if run link ../x "$opt/zig-0.16.0/zig" || run link .hidden "$opt/zig-0.16.0/zig"; then
  bad "link took a name that isn't one"
else
  ok "link takes only plain names"
fi

# A wrapper: sets what it names unless it's set, quoted as given.
f0=$fail
run wrap greet "$opt/zig-0.16.0/zig" "GREETING=it's a test" || bad "wrap greet: $(cat "$work/out")"
# shellcheck disable=SC2016 # the stand-in's own $GREETING
printf '#!/bin/sh\necho "$GREETING"\n' >"$opt/say"
chmod 0755 "$opt/say"
run wrap say "$opt/say" "GREETING=it's a test" || bad "wrap say: $(cat "$work/out")"
[ "$(env -u GREETING "$bin/say")" = "it's a test" ] || bad "the wrapper didn't set GREETING: $(env -u GREETING "$bin/say")"
[ "$(GREETING=mine "$bin/say")" = mine ] || bad "the wrapper overrode a GREETING already set"
if run wrap bad "$opt/say" PATH=/tmp; then bad "wrap set PATH"; fi
[ "$fail" = "$f0" ] && ok "wrap sets a variable unless it's set, and never PATH"

# rustup's proxies: wrappers with RUSTUP_HOME, over the link into ~user,
# which stays as it was.
f0=$fail
if CARGO_HOME=$opt/cargo RUSTUP_HOME=$opt/rustup run rustup; then
  [ ! -L "$bin/cargo" ] && [ -f "$bin/cargo" ] || bad "bin/cargo is still a link: $(readlink "$bin/cargo")"
  [ "$(cat "$closed/.cargo/bin/cargo")" = "$(printf '#!/bin/sh\necho user-cargo')" ] ||
    bad "writing bin/cargo went through the old link into ~user: $(cat "$closed/.cargo/bin/cargo")"
  [ "$(env -u RUSTUP_HOME "$bin/cargo")" = "cargo RUSTUP_HOME=$opt/rustup" ] || bad "cargo: $(env -u RUSTUP_HOME "$bin/cargo")"
  [ "$(env -u RUSTUP_HOME "$bin/rustc")" = "rustc RUSTUP_HOME=$opt/rustup" ] || bad "rustc (a symlinked proxy) runs as $(env -u RUSTUP_HOME "$bin/rustc")"
  [ "$(env -u RUSTUP_HOME "$bin/rustup")" = "rustup RUSTUP_HOME=$opt/rustup" ] || bad "rustup: $("$bin/rustup")"
  [ "$(readlink "$bin/cargo-nextest")" = "$opt/cargo/bin/cargo-nextest" ] || bad "cargo-nextest isn't a link: $(ls -l "$bin/cargo-nextest")"
else
  bad "rustup: $(cat "$work/out")"
fi
[ "$fail" = "$f0" ] && ok "rustup wraps its proxies with RUSTUP_HOME and links cargo install's, never through ~user's link"
if CARGO_HOME=$closed/.cargo RUSTUP_HOME=$opt/rustup run rustup; then bad "rustup took a CARGO_HOME with no rustup"; fi
for left in "$bin"/.[!.]*; do
  [ ! -e "$left" ] || bad "a temporary file was left in bin: $left"
done

# The session users' environment.
f0=$fail
run env A=1 'B=~/cache' "C=one two" || bad "env: $(cat "$work/out")"
run env A=2 B || bad "env again: $(cat "$work/out")"
[ "$(cat "$ARUGULA_TOOLS/env")" = "$(printf 'C=one two\nA=2')" ] || bad "the env file: $(cat "$ARUGULA_TOOLS/env")"
for reserved in PATH=/tmp HOME=/tmp GIT_DIR=/tmp "BAD NAME=x"; do
  if run env "$reserved"; then bad "env took $reserved"; fi
done
[ "$(cat "$ARUGULA_TOOLS/env")" = "$(printf 'C=one two\nA=2')" ] || bad "a refused env changed the file: $(cat "$ARUGULA_TOOLS/env")"
[ "$fail" = "$f0" ] && ok "env sets, replaces and removes a variable, and refuses the daemon's own"

# `as`: what a session's agent sees, by a sudo that runs as this user.
me=$(id -un)
myhome=$(getent passwd "$me" | cut -d: -f6 || true)
if [ -n "$myhome" ]; then
  # shellcheck disable=SC2016 # the stand-in's own
  printf '#!/bin/sh\n[ "$1" = -n ] && shift\n[ "$1" = -u ] && shift 2\n[ "$1" = -- ] && shift\nexec "$@"\n' >"$work/sudo"
  chmod 0755 "$work/sudo"
  run env 'CACHE=~/.cache/x'
  # shellcheck disable=SC2016 # the session's own
  got=$(ARUGULA_HOSTED_SUDO=$work/sudo "$tools" as "$me" /bin/sh -c 'echo "$PATH|$CACHE|$C|$HOME"')
  want="$bin:/usr/local/bin:/usr/bin:/bin|${myhome%/}/.cache/x|one two|$myhome"
  if [ "$got" = "$want" ]; then
    ok "as runs a command with the tools first on PATH and the env file, ~/ its user's home"
  else
    bad "as: $got, not $want"
  fi
else
  echo "skip  as (no passwd entry for $me)"
fi

shellcheck "$tools" >"$work/out" 2>&1 || bad "shellcheck: $(cat "$work/out")"
exit "$fail"
