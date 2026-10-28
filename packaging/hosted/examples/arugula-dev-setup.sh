#!/usr/bin/env bash
# A setup for a hosted workspace on this repo (the arugula-dev workspaces;
# `arugula cloud edit <ws> --setup packaging/hosted/examples/arugula-dev-setup.sh`):
# the toolchain docs/development.md asks for, where the box's session users
# run it too (Arugula #215). It builds nothing.
#
# Setup runs as `user`, whose home session users (ses1..ses6, each
# session's agent and its finish step) can't enter. So nothing a session
# runs is installed under ~user or linked into it: it goes in
# /opt/arugula/tools, and `arugula-tools` (the template's) publishes it in
# /opt/arugula/tools/bin, first on every session user's PATH.
# - Rust: rustup with RUSTUP_HOME and CARGO_HOME there, the toolchain
#   rust-toolchain.toml names, its proxies (cargo, rustc, ...) published as
#   wrappers that set RUSTUP_HOME. Each user keeps a CARGO_HOME of their
#   own (~/.cargo: the registry, `cargo install`), which cargo makes.
# - mise's installs there (MISE_DATA_DIR): .mise.toml's Zig and nextest,
#   and just and pnpm, each published as a link to its binary. Node stays
#   the image's: the agents' adapters run on it, and a node in the tools
#   would come first on their PATH.
# - A build cache for each session user (ARUGULA_DEV_SCCACHE=0 for none):
#   sccache, its cache in the user's home (~/.cache/sccache, at most
#   ARUGULA_DEV_SCCACHE_SIZE), its server on a socket there, and
#   libghostty-vt-sys's Ghostty checkout and Zig caches there too. A
#   session's checkout is new, so its own crates build again, but a slot's
#   next session finds the dependencies and Ghostty built (docs/control.md,
#   "Session users' tools"). One cache all session users write would let
#   one session plant what another links; each user's is theirs.
# - Swap, and cargo's jobs (CARGO_BUILD_JOBS), as a box this size takes.
# - The browser specs (Arugula #264): python3 (the specs' fake agent,
#   scripts/free-port), Playwright's Chromium with its system libraries
#   in $tools/ms-playwright (PLAYWRIGHT_BROWSERS_PATH, E2E_CHROMIUM=1), and
#   one suite lock for the whole box (ARUGULA_SUITE_LOCK_FILE): its
#   sessions share 8 GiB, so one Chrome (or one `just test`) runs at a
#   time across all of them. A session runs its specs with
#   `just e2e-spec e2e/<name>.spec.ts` (docs/testing.md, "On a dev box").
# - /etc/resolv.conf ends with a newline, which Zig needs to fetch
#   Ghostty's dependencies (Arugula #263). envd writes it again at every
#   resume, so arugulad --hosted puts it back too (hosted/resolv.rs); this
#   only fixes it for the setup's own run.
#
# Run again, it does only what's missing, and republishes.
set -euo pipefail

tools=/opt/arugula/tools
command -v arugula-tools >/dev/null && [ -d "$tools/bin" ] ||
  { echo "this box's template has no /opt/arugula/tools (Arugula #215): rebuild it on a newer release" >&2; exit 1; }
export PATH="$tools/bin:$PATH"

repo=""
for d in "$PWD" "$HOME/arugula" "$HOME"/*/; do
  if [ -f "${d%/}/rust-toolchain.toml" ] && [ -f "${d%/}/.mise.toml" ]; then repo="${d%/}"; break; fi
done
[ -n "$repo" ] || { echo "no arugula checkout under $HOME" >&2; exit 1; }
echo "checkout: $repo"

# Links the old setup left in /usr/local/bin, into ~user: nobody else can
# follow them, and the tools in /opt/arugula/tools/bin come first anyway.
for f in /usr/local/bin/*; do
  if [ -L "$f" ] && case $(readlink "$f") in /home/user/*) true ;; *) false ;; esac; then
    sudo -n rm -f -- "$f" && echo "removed $f, a link into ~user"
  fi
done

# Rust: rust-toolchain.toml's toolchain, which is the default too, for
# cargo outside a checkout (a session's agent can't change the default:
# RUSTUP_HOME is closed to its writes).
export RUSTUP_HOME="$tools/rustup" CARGO_HOME="$tools/cargo"
if [ ! -x "$CARGO_HOME/bin/rustup" ]; then
  curl -fsSL https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain none --no-modify-path
fi
(cd "$repo" && "$CARGO_HOME/bin/rustup" toolchain install)
channel=$(sed -n 's/^channel *= *"\(.*\)"$/\1/p' "$repo/rust-toolchain.toml")
"$CARGO_HOME/bin/rustup" default "$channel"
arugula-tools rustup
unset RUSTUP_HOME CARGO_HOME
hash -r

# mise, and what it installs, in the tools. Setup has no one to answer
# mise's prompts (its trust prompt hung the first box).
export MISE_DATA_DIR="$tools/mise" MISE_YES=1
mise="$tools/mise/bin/mise"
if [ ! -x "$mise" ]; then
  curl -fsSL https://mise.run | MISE_INSTALL_PATH="$mise" sh
fi
"$mise" trust "$repo"
"$mise" use -g node@24 pnpm@12 just@latest
(cd "$repo" && "$mise" install)
for b in zig cargo-nextest just pnpm; do
  arugula-tools link "$b" "$(cd "$repo" && "$mise" which "$b")"
done
hash -r

# The web client's dependencies, as `just bootstrap` installs them.
(cd "$repo/web" && pnpm install --frozen-lockfile)

# Zig reads a last line with no newline as cut off (Arugula #263).
if [ -s /etc/resolv.conf ] && [ "$(tail -c 1 /etc/resolv.conf | od -An -c | tr -d ' ')" != '\n' ]; then
  sudo -n sh -c "printf '\n' >> /etc/resolv.conf" && echo "ended /etc/resolv.conf with a newline"
fi

# The browser specs (Arugula #264): python3 for the specs' fake agent and
# scripts/free-port, and Playwright's Chromium, the version this checkout's
# Playwright wants, with the system libraries it needs (its --with-deps
# runs apt through sudo). In the tools, where every session user reads it.
if ! command -v python3 >/dev/null; then
  sudo -n apt-get update -qq
  sudo -n env DEBIAN_FRONTEND=noninteractive apt-get install -y -qq --no-install-recommends python3 perl
fi
export PLAYWRIGHT_BROWSERS_PATH="$tools/ms-playwright"
(cd "$repo/web" && pnpm exec playwright install --with-deps chromium)
arugula-tools env "PLAYWRIGHT_BROWSERS_PATH=$tools/ms-playwright" E2E_CHROMIUM=1
# One suite at a time on the box (scripts/suite-lock): a file every session
# user can open to lock, in a directory only `user` writes, so none can
# swap it for another.
install -d -m 0755 /opt/arugula/locks
[ -f /opt/arugula/locks/suite.lock ] || install -m 0666 /dev/null /opt/arugula/locks/suite.lock
chmod 0666 /opt/arugula/locks/suite.lock
export ARUGULA_SUITE_LOCK_FILE=/opt/arugula/locks/suite.lock
arugula-tools env "ARUGULA_SUITE_LOCK_FILE=$ARUGULA_SUITE_LOCK_FILE"

# Swap, so a Rust build doesn't take the box down: with 2 GB, cargo's
# rustc and linker ran the box out of memory and the OOM killer took the
# daemon (2026-10-24: six boxes paused and never came back).
if ! swapon --show | grep -q /swapfile; then
  [ -f /swapfile ] || { sudo -n fallocate -l 6G /swapfile && sudo -n chmod 600 /swapfile && sudo -n mkswap /swapfile >/dev/null; }
  sudo -n swapon /swapfile
fi

# Cargo's jobs: one per CPU, but no more than a job per GB (a third rustc
# OOMed a 2 GB box), and at least two. For `user` in ~/.cargo/config.toml,
# for session users in their environment.
mem_gb=$(($(awk '/MemTotal/ {print $2}' /proc/meminfo) / 1048576))
jobs=$(nproc)
if [ "$jobs" -gt "$mem_gb" ]; then jobs=$mem_gb; fi
if [ "$jobs" -lt 2 ]; then jobs=2; fi
mkdir -p "$HOME/.cargo"
grep -q '^jobs' "$HOME/.cargo/config.toml" 2>/dev/null || printf '[build]\njobs = %s\n' "$jobs" >>"$HOME/.cargo/config.toml"
arugula-tools env "CARGO_BUILD_JOBS=$jobs"

# Each session user's build cache.
if [ "${ARUGULA_DEV_SCCACHE:-1}" != 0 ]; then
  v=0.18.0
  sum=45f1447fbe231e3037bde351ef70677dd212216c8d62ae7ca409fecc4d6acc89
  dir="$tools/sccache/$v"
  if [ ! -x "$dir/sccache" ]; then
    tmp=$(mktemp -d)
    f="sccache-v$v-x86_64-unknown-linux-musl.tar.gz"
    curl -fsSL -o "$tmp/$f" "https://github.com/mozilla/sccache/releases/download/v$v/$f"
    echo "$sum  $tmp/$f" | sha256sum -c -
    mkdir -p "$dir"
    tar -xzf "$tmp/$f" -C "$dir" --strip-components=1 "sccache-v$v-x86_64-unknown-linux-musl/sccache"
    chmod 0755 "$dir/sccache"
    rm -rf "$tmp"
  fi
  arugula-tools link sccache "$dir/sccache"
  # The cc crate puts C compiles through RUSTC_WRAPPER only when it's named
  # sccache. Its server listens on a socket in the user's home: on
  # sccache's default (127.0.0.1:4226) one session user's server would
  # compile another's. Unreachable, the compiler runs without it.
  arugula-tools env \
    "RUSTC_WRAPPER=$tools/bin/sccache" \
    'SCCACHE_DIR=~/.cache/sccache' \
    "SCCACHE_CACHE_SIZE=${ARUGULA_DEV_SCCACHE_SIZE:-2G}" \
    'SCCACHE_SERVER_UDS=~/.sccache.sock' \
    SCCACHE_IGNORE_SERVER_IO_ERROR=1 \
    'LIBGHOSTTY_VT_SYS_CACHE_DIR=~/.cache/libghostty-vt-sys'
else
  arugula-tools env RUSTC_WRAPPER SCCACHE_DIR SCCACHE_CACHE_SIZE SCCACHE_SERVER_UDS SCCACHE_IGNORE_SERVER_IO_ERROR \
    LIBGHOSTTY_VT_SYS_CACHE_DIR
fi

# The same tools in `user`'s own shells.
line="export MISE_DATA_DIR=$tools/mise PLAYWRIGHT_BROWSERS_PATH=$tools/ms-playwright E2E_CHROMIUM=1 ARUGULA_SUITE_LOCK_FILE=/opt/arugula/locks/suite.lock PATH=\"$tools/bin:\$HOME/.cargo/bin:\$HOME/.local/bin:$tools/mise/bin:$tools/mise/shims:\$PATH\""
for f in "$HOME/.profile" "$HOME/.bashrc"; do
  grep -qxF "$line" "$f" 2>/dev/null || echo "$line" >>"$f"
done

# What a session sees, as ses1 when the box has it (expanded there).
# shellcheck disable=SC2016
check='cd /tmp && rustc --version && cargo --version && cargo nextest --version | head -1 && zig version && just --version && pnpm --version && node --version && python3 --version && ls -d "$PLAYWRIGHT_BROWSERS_PATH"/chromium-*/chrome-linux*/chrome && test -w "$ARUGULA_SUITE_LOCK_FILE"'
if getent passwd ses1 >/dev/null; then
  arugula-tools as ses1 sh -c "$check"
else
  sh -c "$check"
fi
echo "arugula-dev ready"
