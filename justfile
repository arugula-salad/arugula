# Arugula tasks. Cargo runs under mise so libghostty-vt-sys finds the Zig
# it needs (.mise.toml).

set shell := ["bash", "-euo", "pipefail", "-c"]

# rustup's default location, for shells that haven't sourced ~/.cargo/env.
export PATH := env("HOME") / ".cargo/bin" + ":" + env("PATH")

cargo := "mise exec -- cargo"

# Where cargo builds (CI keeps one per runner, outside the checkout).
target_dir := env("CARGO_TARGET_DIR", justfile_directory() / "target")

# cargo on the hosted server's own workspace (server/Cargo.toml: control,
# its platform and control-common), into the same target dir as the root's
# (scripts/server-cargo). The root workspace has none of them, so `{{cargo}}`
# never compiles the server, nor this the daemon or Ghostty.
server_cargo := "mise exec -- " + justfile_directory() / "scripts/server-cargo"

# Runs a suite while no other worktree here runs one (scripts/suite-lock).
suite_lock := justfile_directory() / "scripts/suite-lock"

default:
    @just --list

# Install toolchains (Zig via mise) and web dependencies.
bootstrap:
    mise install
    cd web && pnpm install --frozen-lockfile

# Copies scripts/sccache/sccache out of the checkout and prints the
# ~/.cargo/config.toml lines that turn the caches on; writes no config
# itself (docs/development.md, "Caches for worktrees").
# Share build caches (Ghostty's Zig build, sccache) between worktrees.
build-cache:
    #!/usr/bin/env bash
    set -euo pipefail
    if ! command -v sccache >/dev/null; then
      echo "sccache isn't on PATH: brew install sccache (macOS), or cargo install --locked sccache" >&2
      exit 1
    fi
    wrapper=${XDG_DATA_HOME:-$HOME/.local/share}/arugula/sccache/sccache
    mkdir -p "$(dirname "$wrapper")"
    install -m 755 scripts/sccache/sccache "$wrapper"
    case $(uname -s) in
      Darwin) caches=$HOME/Library/Caches ;;
      *) caches=${XDG_CACHE_HOME:-$HOME/.cache} ;;
    esac
    echo "Installed $wrapper. Add to ~/.cargo/config.toml (merge into any [build] or [env] there):" >&2
    printf '\n[build]\nrustc-wrapper = "%s"\n\n[env]\n' "$wrapper"
    printf 'LIBGHOSTTY_VT_SYS_CACHE_DIR = "%s/libghostty-vt-sys"\n' "$caches"
    printf 'SCCACHE_CACHE_SIZE = "20G"\n'

# Delete the builds in target dirs that nothing has used in HOURS, keeping
# each crate's newest (scripts/sweep-target). Cargo never deletes a build
# whose version or features changed: a worktree reached 40 GB, 31 GB of it
# dead caches. With no DIRS, this checkout's.
sweep hours="48" *dirs:
    scripts/sweep-target --hours {{hours}} {{ if dirs == "" { quote(target_dir) } else { dirs } }}

# Build the web client into web/dist (embedded into the daemon).
web:
    cd web && pnpm run build

# The web client's import rule (M148): platform/ never reaches the key code,
# the daemon client, control's trust client or the fleet, and shared/
# imports neither side. `just web` checks it too; this also runs its tests.
web-imports:
    cd web && pnpm run imports

# Write the web client's wire types (web/src/shared/proto.gen.ts) from
# crates/proto. Without `write`, check they're current, as CI does.
proto-ts mode="write":
    {{ if mode == "write" { "ARUGULA_WRITE_TS=1" } else { "" } }} cargo test -q -p arugula-proto --features ts ts::

# Write DECISIONS.md, the index of the decision records in docs/decisions/.
# Without `write`, check it's current.
decisions mode="write":
    node scripts/decisions-index.mjs {{ if mode == "write" { "" } else { "--check" } }}

# Write "What control stores" in docs/control-e2e.md from control's table
# manifest (crates/control/tables.toml). Without `write`, check it's
# current, and that the manifest lists every table control has: the same
# tests `just test` runs, so CI runs them too.
control-tables mode="write":
    {{ if mode == "write" { "ARUGULA_WRITE_DOCS=1" } else { "" } }} {{server_cargo}} test -q -p arugula-control tables::

# Release build of the daemon and the CLI (the root workspace; control and
# its platform: `just static`).
build: web
    {{cargo}} build --release

# Static musl binaries (daemon, CLI, arugula-control and arugula-platform)
# for sandboxes, machines without systemd, hosting control and releases:
# target/ARCH-unknown-linux-musl/release/. ARCH is
# x86_64 or aarch64. Zig, already here for libghostty, is the C compiler and
# brings musl; for aarch64 it links too.
static arch="x86_64": web (_static arch "daemon") (_static arch "server")

# Only the server's static binaries (arugula-control and arugula-platform),
# as `just static` builds them: no daemon, no Ghostty.
static-server arch="x86_64": web (_static arch "server")

# `just static`'s build of one workspace: daemon (the root's arugulad and
# arugula) or server (server/Cargo.toml's control and platform).
[private]
_static arch side:
    #!/usr/bin/env bash
    set -euo pipefail
    t={{arch}}-unknown-linux-musl; T=$(echo "$t" | tr a-z- A-Z_)
    rustup target add "$t" >/dev/null
    export ZIG_MUSL_ARCH={{arch}} "CC_${t//-/_}=$PWD/scripts/zig-cc-musl" "AR_${t//-/_}=$PWD/scripts/zig-ar"
    # Cross: Zig links too, with its own musl and startup files, not rustc's.
    if [ {{arch}} != "$(uname -m)" ]; then export "CARGO_TARGET_${T}_LINKER=$PWD/scripts/zig-cc-musl" "CARGO_TARGET_${T}_RUSTFLAGS=-C link-self-contained=no"; fi
    case {{side}} in
      daemon) bins=(arugulad arugula); {{cargo}} build --release --target "$t" -p arugulad -p arugula ;;
      server) bins=(arugula-control arugula-platform); {{server_cargo}} build --release --target "$t" -p arugula-control -p arugula-platform ;;
      *) echo "_static: SIDE is daemon or server" >&2; exit 2 ;;
    esac
    for b in "${bins[@]}"; do file "{{target_dir}}/$t/release/$b"; done

# Deploy the hosted Arugula control to Fly (packaging/control/fly.toml):
# the static x86_64 binary in a distroless image, from a small build context.
# Release tags deploy it from CI (release.yml's `control` job, M133); this
# is for hotfixes, and the fallback when that job can't.
control-deploy: static-server
    #!/usr/bin/env bash
    set -euo pipefail
    ctx=$(mktemp -d)
    trap 'rm -rf "$ctx"' EXIT
    cp {{target_dir}}/x86_64-unknown-linux-musl/release/arugula-control packaging/control/Dockerfile packaging/control/fly.toml "$ctx"/
    cd "$ctx" && fly deploy --local-only --ha=false

# Deploy control's platform to Fly (packaging/platform/fly.toml, M140): the
# static x86_64 binary in a distroless image. Release tags deploy it from
# CI before the hub (release.yml's `platform` job); this is for hotfixes.
platform-deploy: static-server
    #!/usr/bin/env bash
    set -euo pipefail
    ctx=$(mktemp -d)
    trap 'rm -rf "$ctx"' EXIT
    cp {{target_dir}}/x86_64-unknown-linux-musl/release/arugula-platform packaging/platform/Dockerfile packaging/platform/fly.toml "$ctx"/
    # Its service is Flycast's, never public (fly.toml): no public IPs.
    cd "$ctx" && fly deploy --local-only --ha=false --no-public-ips

# The daemons the hosted control saw in the last 30 days, by version: a
# read-only look at its database (docs/control-ops.md, "Daemon census").
control-census days="30":
    fly ssh console -a illogical-control -C "/arugula-control census --db /data/control.db --days {{days}}"

# Hosted workspaces' usage and cost on E2B, read-only (docs/control-ops.md,
# "E2B (hosted workspaces' boxes)").
platform-usage days="7":
    fly ssh console -a arugula-platform -C "/arugula-platform usage --db env:ARUGULA_PLATFORM_DATABASE_URL --days {{days}}"

# The macOS daemon and CLI for Intel Macs, cross-compiled on Apple silicon
# (or built natively on an Intel Mac): target/x86_64-apple-darwin/release/.
# Then `just dist` and `just desktop x86_64`.
build-macos-x86_64: web
    rustup target add x86_64-apple-darwin >/dev/null
    {{cargo}} build --release --target x86_64-apple-darwin -p arugulad -p arugula

# Release tarballs in dist/: arugula-VERSION-TARGET.tar.gz with both
# binaries and the licenses, for the targets already built (`just static`,
# `just static aarch64`, `just build` and `just build-macos-x86_64` on a Mac).
dist:
    #!/usr/bin/env bash
    set -euo pipefail
    v=$({{cargo}} pkgid -p arugulad | sed 's/.*[#@]//')
    host=$(rustc -vV | sed -n 's/^host: //p')
    mkdir -p dist
    for t in x86_64-unknown-linux-musl aarch64-unknown-linux-musl aarch64-apple-darwin x86_64-apple-darwin; do
      d={{target_dir}}/$t/release
      # The Mac's own architecture builds without --target (`just build`).
      if [ "$t" = "$host" ] && [ -x {{target_dir}}/release/arugulad ]; then d={{target_dir}}/release; fi
      [ -x "$d/arugulad" ] || continue
      n=arugula-$v-$t; rm -rf "dist/$n"; mkdir -p "dist/$n"
      cp "$d/arugulad" "$d/arugula" LICENSE-MIT LICENSE-APACHE THIRD_PARTY.md README.md "dist/$n/"
      scripts/dist-pack "dist/$n"
      rm -rf "dist/$n"
    done
    (cd dist && (sha256sum *.tar.gz 2>/dev/null || shasum -a 256 *.tar.gz) > SHA256SUMS)

# The desktop app (crates/desktop, M46), carrying this build's arugulad
# and arugula: run after `just static x86_64` (Linux; the static binaries
# run anywhere) or `just build` (macOS). Writes dist/arugula-desktop-*,
# named without the version so the site's download links always find the
# latest release. Linux: `just desktop-linux ARCH` (x86_64 by default).
# macOS: `just desktop-macos ARCH`, this Mac's own arch by default;
# `just desktop x86_64` after `just build-macos-x86_64` makes the Intel app.
desktop arch="":
    #!/usr/bin/env bash
    set -euo pipefail
    a="{{arch}}"
    case "$(uname -s)" in
      Linux) just desktop-linux "${a:-x86_64}" ;;
      Darwin) just desktop-macos "$a" ;;
    esac

# The Linux desktop app for ARCH (x86_64 or aarch64): a .deb, an .rpm and
# an AppImage, after `just static ARCH`. It builds in an Ubuntu 22.04
# container (podman or docker, packaging/desktop/Containerfile) so it runs
# on glibc 2.35 and newer, and fails if anything in the bundles needs more
# (#170). Another architecture than the host's runs under emulation
# (qemu-user binfmt on the host).
desktop-linux arch="x86_64" *tauri_args="":
    #!/usr/bin/env bash
    set -euo pipefail
    # An unset GitHub secret arrives as "": treat it as unset, or Tauri
    # takes APPLE_CERTIFICATE="" for a certificate to import.
    for v in APPLE_CERTIFICATE APPLE_CERTIFICATE_PASSWORD APPLE_SIGNING_IDENTITY APPLE_ID APPLE_PASSWORD APPLE_TEAM_ID TAURI_SIGNING_PRIVATE_KEY TAURI_SIGNING_PRIVATE_KEY_PASSWORD; do
      [ -n "${!v:-}" ] || unset "$v"
    done
    root={{justfile_directory()}}
    dist=$root/dist
    mkdir -p "$dist"
    # The oldest glibc the app runs on: Ubuntu 22.04's, the container's.
    floor=2.35
    arch={{arch}}
    case "$arch" in x86_64) platform=linux/amd64 ;; aarch64) platform=linux/arm64 ;; *) echo "arch: x86_64 or aarch64" >&2; exit 2 ;; esac
    src={{target_dir}}/$arch-unknown-linux-musl/release
    # The daemon release's binaries (app-release.yml's `scripts/release sidecars`).
    src=${ARUGULA_DESKTOP_BINARIES:-$src}
    mkdir -p crates/desktop/binaries
    for b in arugulad arugula; do install -m 755 "$src/$b" "crates/desktop/binaries/$b-$arch-unknown-linux-gnu"; done
    engine=$(command -v podman || command -v docker) || { echo "the Linux desktop build needs podman or docker" >&2; exit 1; }
    toolchain=$(sed -n 's/^channel = "\(.*\)"/\1/p' crates/desktop/rust-toolchain.toml)
    image=arugula-desktop-build:jammy-$toolchain-$arch
    "$engine" build -q --platform "$platform" -t "$image" -f packaging/desktop/Containerfile --build-arg RUST_TOOLCHAIN="$toolchain" --build-arg TAURI_CLI=2.12.1 packaging/desktop
    # Its own target dir: build scripts built against 22.04's glibc
    # don't mix with the host's.
    target={{target_dir}}/desktop-jammy-$arch
    mkdir -p "$target"
    # Docker Desktop's file sharing (a Mac) refuses linuxdeploy's copies
    # into a shared directory: build in a volume there.
    [ "$(uname -s)" = Darwin ] && target=arugula-desktop-target-$arch
    "$engine" run --rm --platform "$platform" --security-opt label=disable \
      -v "$root:/src" -v "$target:/target" -v "$dist:/dist" \
      -v arugula-desktop-cargo-$arch:/opt/cargo/registry -v arugula-desktop-tauri-$arch:/root/.cache/tauri \
      -e CARGO_TARGET_DIR=/target -e TAURI_SIGNING_PRIVATE_KEY -e TAURI_SIGNING_PRIVATE_KEY_PASSWORD \
      "$image" /src/packaging/desktop/build-linux.sh {{tauri_args}}
    name=$dist/arugula-desktop-linux-$arch
    if command -v dpkg-deb >/dev/null; then
      scripts/glibc-floor "$floor" "$name.deb" "$name.AppImage"
      dpkg-deb -f "$name.deb" Depends | grep -q "libc6 (>= $floor)" \
        || { echo "the .deb should depend on libc6 (>= $floor): crates/desktop/tauri.conf.json" >&2; exit 1; }
    else
      echo "no dpkg-deb here: skipped the glibc floor check" >&2
    fi
    ls -la "$name".*

# The Linux packages from `just desktop-linux ARCH` install on a fresh
# Ubuntu 22.04 (.deb) and Fedora (.rpm) and claim arugula:// links
# (packaging/desktop/packages.sh).
desktop-packages arch="x86_64":
    packaging/desktop/packages.sh {{arch}}

# The macOS app for ARCH (aarch64 or x86_64; this Mac's own by default),
# after `just build` (or `just build-macos-x86_64` for the Intel app,
# cross-compiled on Apple silicon): the app, a zip of it and a .dmg in
# dist/, named macos-arm64 or macos-x86_64. scripts/macos-sign signs,
# notarizes and staples them when the Developer ID secrets are set (#177)
# and says which is missing when not; the app is ad-hoc signed either way.
# With TAURI_SIGNING_PRIVATE_KEY set, also the updater's .app.tar.gz and
# its signature.
desktop-macos arch="" *tauri_args="":
    #!/usr/bin/env bash
    set -euo pipefail
    # An unset GitHub secret arrives as "": treat it as unset, or Tauri
    # takes APPLE_CERTIFICATE="" for a certificate to import.
    for v in APPLE_CERTIFICATE APPLE_CERTIFICATE_PASSWORD APPLE_SIGNING_IDENTITY APPLE_ID APPLE_PASSWORD APPLE_TEAM_ID APPLE_PROVISIONING_PROFILE TAURI_SIGNING_PRIVATE_KEY TAURI_SIGNING_PRIVATE_KEY_PASSWORD; do
      [ -n "${!v:-}" ] || unset "$v"
    done
    root={{justfile_directory()}}
    dist=$root/dist
    mkdir -p "$dist"
    command -v cargo-tauri >/dev/null || cargo install tauri-cli --version "^2" --locked
    # Passkeys in the window (D2): with the Developer ID provisioning
    # profile (APPLE_PROVISIONING_PROFILE, a base64 .provisionprofile) the
    # bundle carries it as Contents/embedded.provisionprofile, and
    # scripts/macos-sign signs the app with the associated-domains
    # entitlement it allows. The bundler doesn't sign then: it would give
    # the sidecars the app's entitlements too, and a bare executable that
    # claims a restricted one is killed at launch. Without the profile (or
    # without the certificate, which it's useless without) nothing changes.
    sysbin=$(mktemp -d); keep=$(mktemp -d); trap 'rm -rf "$sysbin" "$keep"' EXIT
    profile=()
    if [ -n "${APPLE_PROVISIONING_PROFILE:-}" ] && [ -n "${APPLE_CERTIFICATE:-}" ]; then
      cargo tauri build --help | grep -q -- --no-sign \
        || { echo "this cargo-tauri has no --no-sign: cargo install tauri-cli --version '^2' --locked" >&2; exit 1; }
      printf '%s' "$APPLE_PROVISIONING_PROFILE" | base64 --decode > "$keep/embedded.provisionprofile"
      profile=(--no-sign --config "{\"bundle\":{\"macOS\":{\"files\":{\"embedded.provisionprofile\":\"$keep/embedded.provisionprofile\"}}}}")
      echo "with the provisioning profile: passkeys in the window (D2)"
    elif [ -n "${APPLE_PROVISIONING_PROFILE:-}" ]; then
      echo "APPLE_PROVISIONING_PROFILE is set but APPLE_CERTIFICATE isn't: built without it (no passkeys in the window)"
    else
      echo "no APPLE_PROVISIONING_PROFILE: no passkeys in the window (D2)"
    fi
    cd crates/desktop
    host=$(rustc -vV | sed -n 's/^host: //p')
    a="{{arch}}"; t=${a:-${host%%-*}}-apple-darwin
    case "$t" in
      aarch64-apple-darwin) name=macos-arm64 ;;
      x86_64-apple-darwin) name=macos-x86_64 ;;
      *) echo "no macOS app for $t (arch: aarch64 or x86_64)" >&2; exit 2 ;;
    esac
    # The Mac's own arch builds without --target (`just build`).
    src={{target_dir}}/$t/release
    if [ "$t" = "$host" ] && [ -x {{target_dir}}/release/arugulad ]; then src={{target_dir}}/release; fi
    # A test's own daemon and CLI (testnet/macos/update.sh's older ones), or
    # the daemon release's (app-release.yml's `scripts/release sidecars`).
    src=${ARUGULA_DESKTOP_BINARIES:-$src}
    out=${CARGO_TARGET_DIR:-$PWD/target}
    flags=()
    if [ "$t" = "$host" ]; then out=$out/release; else
      rustup target add "$t" >/dev/null
      flags=(--target "$t"); out=$out/$t/release
    fi
    mkdir -p binaries
    for b in arugulad arugula; do install -m 755 "$src/$b" "binaries/$b-$t"; done
    # The bundler runs `xattr -crs` from PATH: pyenv's shim (Python's
    # xattr, no -c or -r) can come first and fail it (#467). Only the
    # system xattr goes first; the rest of PATH stays as it was.
    ln -s /usr/bin/xattr "$sysbin/xattr"
    # ${flags[@]+…}: macOS bash 3.2 calls an empty array unbound.
    PATH="$sysbin:$PATH" cargo tauri build --bundles app ${flags[@]+"${flags[@]}"} ${profile[@]+"${profile[@]}"} {{tauri_args}}
    # The bundle is named after the app's productName (Arugula.app).
    product=$(sed -n 's/^ *"productName": "\(.*\)",$/\1/p' tauri.conf.json)
    app=$out/bundle/macos/$product.app
    "$root/scripts/macos-sign" app "$app"
    # A zip of the app: ditto keeps its signature and symlinks.
    # --norsrc: no ._* AppleDouble files for xattrs like
    # com.apple.provenance, which a command-line unzip leaves in the
    # bundle (#177). The signature lives in the bundle, not in xattrs.
    zip=$dist/arugula-desktop-$name.zip
    rm -f "$zip"
    ditto -c -k --norsrc --keepParent "$app" "$zip"
    if zipinfo -1 "$zip" | grep -E '(^|/)\._'; then echo "AppleDouble files in $zip" >&2; exit 1; fi
    # The .dmg: the app beside a link to /Applications.
    stage=$(mktemp -d)
    ditto "$app" "$stage/$product.app"
    ln -s /Applications "$stage/Applications"
    dmg=$dist/arugula-desktop-$name.dmg
    rm -f "$dmg"
    hdiutil create -quiet -volname Arugula -srcfolder "$stage" -fs HFS+ -format UDZO "$dmg"
    rm -rf "$stage"
    "$root/scripts/macos-sign" dmg "$dmg"
    # The updater's archive of the app, signed with the updater key.
    if [ -n "${TAURI_SIGNING_PRIVATE_KEY:-}" ]; then
      tgz=$dist/arugula-desktop-$name.app.tar.gz
      # No AppleDouble `._*` entries or xattr headers: Tauri's updater fails
      # to unpack `._Arugula.app` (#533), and macOS's tar adds them unless told not to.
      COPYFILE_DISABLE=1 tar --no-mac-metadata --no-xattrs -C "$(dirname "$app")" -czf "$tgz" "$product.app"
      if tar -tzf "$tgz" | grep -qE '(^|/)\._'; then echo "$tgz has AppleDouble (._) entries" >&2; exit 1; fi
      cargo tauri signer sign "$tgz" >/dev/null
      echo "signed $tgz for the updater"
    else
      echo "no updater archive: TAURI_SIGNING_PRIVATE_KEY isn't set"
    fi
    ls -la "$dist"/arugula-desktop-"$name".*

# The Linux desktop app under Xvfb, in a container: builds the app (debug,
# no bundle) in its build image (packaging/desktop/Containerfile) and runs
# packaging/desktop/xvfb's tests against a static daemon: `join` (#204,
# test.sh, with a stand-in control), `app` (app.sh: keys, the titlebar,
# arugula:// links, the global hotkey, a right-click in Nautilus opens a
# tab), `stale` (#317, stale.sh: a 0.8.0 daemon, stopped or running,
# gets the setup page and is left alone) and `platform` (M157,
# platform.sh: control's platform page stays in the window and its window
# controls work there; lookalikes go to the browser). Needs podman or docker; the
# container runs the host's architecture (aarch64 under Docker Desktop on
# a Mac). `just desktop-xvfb app keys` runs one claim.
#
# ARUGULA_DESKTOP_BINARIES: a directory with the static daemon and CLI to
# test (check.yml's static job's) in place of `just static`.
# ARUGULA_DESKTOP_IMAGE_REPO: a registry repository (check.yml: GHCR) to
# pull the test image from, tagged by what it's built from, in place of
# building it (6 min on a fresh runner); built when the pull fails, and
# pushed there too with ARUGULA_DESKTOP_IMAGE_PUSH=1.
desktop-xvfb *tests="join app stale platform":
    #!/usr/bin/env bash
    set -euo pipefail
    root={{justfile_directory()}}
    engine=$(command -v podman || command -v docker) || { echo "the desktop test needs podman or docker" >&2; exit 1; }
    arch=$(uname -m); [ "$arch" = arm64 ] && arch=aarch64
    src=${ARUGULA_DESKTOP_BINARIES:-}
    if [ -z "$src" ]; then just static "$arch"; src={{target_dir}}/$arch-unknown-linux-musl/release; fi
    host=$arch-unknown-linux-gnu
    mkdir -p crates/desktop/binaries
    for b in arugulad arugula; do install -m 755 "$src/$b" "crates/desktop/binaries/$b-$host"; done
    toolchain=$(sed -n 's/^channel = "\(.*\)"/\1/p' crates/desktop/rust-toolchain.toml)
    tauri_cli=2.12.1
    base=arugula-desktop-build:jammy-$toolchain
    image=arugula-desktop-xvfb:jammy-$toolchain
    repo=${ARUGULA_DESKTOP_IMAGE_REPO:-}
    # Everything the two images are built from (neither COPYs a file).
    tag=$(cat packaging/desktop/Containerfile packaging/desktop/xvfb/Containerfile | { cat; echo "$toolchain $tauri_cli $arch"; } | sha256sum | cut -c1-16)
    if [ -n "$repo" ] && "$engine" pull -q "$repo:$tag" >/dev/null 2>&1; then
      echo "the test image: $repo:$tag"
      "$engine" tag "$repo:$tag" "$image"
    else
      [ -z "$repo" ] || echo "no $repo:$tag yet: building it"
      "$engine" build -q -t "$base" -f packaging/desktop/Containerfile --build-arg RUST_TOOLCHAIN="$toolchain" --build-arg TAURI_CLI="$tauri_cli" packaging/desktop
      "$engine" build -q -t "$image" -f packaging/desktop/xvfb/Containerfile --build-arg BASE="$base" packaging/desktop/xvfb
      if [ -n "$repo" ] && [ "${ARUGULA_DESKTOP_IMAGE_PUSH:-}" = 1 ]; then
        # Never the test's failure: the next run builds it again.
        "$engine" tag "$image" "$repo:$tag"
        "$engine" push -q "$repo:$tag" || echo "couldn't push $repo:$tag" >&2
      fi
    fi
    target={{target_dir}}/desktop-xvfb-$arch
    mkdir -p "$target"
    "$engine" run --rm --security-opt label=disable \
      -v "$root:/src" -v "$target:/target" -v arugula-desktop-cargo:/opt/cargo/registry \
      -e CARGO_TARGET_DIR=/target -w /src/crates/desktop \
      "$image" bash -c 'set -euo pipefail
        cargo tauri build --debug --no-bundle
        APP=/target/debug/arugula-desktop BIN=/src/crates/desktop/binaries HOST='"$host"' \
          /src/packaging/desktop/xvfb/run.sh {{tests}}'

# Lint the desktop app (its own workspace) without building its sidecars.
desktop-check:
    #!/usr/bin/env bash
    set -euo pipefail
    cd crates/desktop
    host=$(rustc -vV | sed -n 's/^host: //p')
    mkdir -p binaries
    # tauri-build wants the sidecars to exist; empty stand-ins do for a lint.
    for b in arugulad arugula; do [ -e "binaries/$b-$host" ] || : > "binaries/$b-$host"; done
    cargo fmt --check
    cargo clippy -- -D warnings

# THIRD_PARTY.md: notices for the Rust crates (cargo-about) and the npm
# packages bundled into the web client; server/THIRD_PARTY.md for the
# server's workspace (control and its platform); crates/desktop/THIRD_PARTY.md
# for the desktop app's own crates (its about.toml also accepts MPL-2.0).
# Needs cargo-about 0.9.2; leaves all three alone when anything fails.
notices:
    scripts/notices

# CI runs it on main (check.yml's clippy) and on pull requests (binaries.yml).
# Fails when a THIRD_PARTY.md is stale: run `just notices`.
notices-check: notices
    git diff --exit-code THIRD_PARTY.md server/THIRD_PARTY.md crates/desktop/THIRD_PARTY.md

# CI runs it on main (check.yml's lint) and on pull requests (binaries.yml).
# rustfmt's check over the workspace, the server's and the desktop app's.
fmt-check:
    cargo fmt --all --check
    scripts/server-cargo fmt --all --check
    cd crates/desktop && cargo fmt --check

# Release without waiting for a version bump to pass CI (#524): commit the
# bump on top of the newest commit on main with a ci/green tag, tag it, and
# push the tag with main, the bump merged in (docs/development.md).
# just release [patch|minor|major] [--app] [COMMIT] [--dry-run]
release *args:
    scripts/release bump {{args}}

# The newest commits on main that passed CI (their ci/green tags), newest
# first; a tag CI revoked since is gone here too.
green n="20":
    @git fetch -q --force --prune https://github.com/arugula-salad/arugula.git '+refs/tags/ci/*:refs/tags/ci/*'
    @git log --no-walk --tags='ci/green/*' --date=format-local:'%Y-%m-%d %H:%M' --format='%h %cd %s' | head -n {{n}}

# All tests, the root workspace's and the server's. The Rust ones run under
# cargo-nextest (.config/nextest.toml, server/.config/nextest.toml), which
# `just bootstrap` installs; the doctests, which it can't run, under cargo
# test. Builds first (test-build), then runs the suite one worktree at a time
# (scripts/suite-lock).
test: test-build
    {{suite_lock}} just test-run

# What `just test` runs, built. check.yml's test job builds it before
# static's binaries are in.
[private]
test-build: web
    {{cargo}} nextest run --workspace --no-run
    {{server_cargo}} nextest run --workspace --no-run
    {{test_bins}}
    {{server_test_bins}}
    {{cargo}} build -p arugula-e2e --example interop
    cd web && pnpm run typecheck

# The binaries control-smoke and the specs run, built as the tests' own
# build resolves features: the whole workspace with an example, which turns
# on the dev-dependencies' features (arugula-proto's fixtures, rmcp's
# client...). A `-p` build resolves them again without those, and every crate
# under them, arugulad among them, built a second time (~50 s from cold).
# So these are the units `nextest run --workspace` built.
test_bins := cargo + " build --workspace --bins --example interop"

# The server's binaries (arugula-control's, arugula-platform's), the same
# way: with its tests, so its dev-dependencies' features (the platform's
# `contract`) are on as in its tests' build.
server_test_bins := server_cargo + " build --workspace --bins --tests"

# PART (check.yml's test and test-2, 1 or 2) runs half the Rust tests of
# each workspace, nextest's count:PART/2; only the first half runs the
# doctests and control-smoke after them.
[private]
test-run part="":
    #!/usr/bin/env bash
    set -euo pipefail
    case "{{part}}" in
      '') part=() ;;
      1|2) part=(--partition "count:{{part}}/2") ;;
      *) echo "test-run: PART is 1 or 2" >&2; exit 2 ;;
    esac
    # ${part[@]+…}: macOS bash 3.2 calls an empty array unbound.
    {{cargo}} nextest run --workspace ${part[@]+"${part[@]}"}
    {{server_cargo}} nextest run --workspace ${part[@]+"${part[@]}"}
    [ "{{part}}" != 2 ] || exit 0
    {{cargo}} test --workspace --doc
    {{server_cargo}} test --workspace --doc
    just e2e-interop control-smoke

# The platform's suite on Postgres (M143), as hosted control runs it: a
# postgres:17 container of its own on a free port, removed after. The same
# tests as on SQLite (`Db::test`); check.yml's test-2 job runs it after
# `test-run 2`, on what test-build built.
test-platform-postgres:
    #!/usr/bin/env bash
    set -euo pipefail
    name="arugula-platform-pg-$$"
    docker run -d --rm --name "$name" -e POSTGRES_PASSWORD=platform -p 127.0.0.1::5432 \
        postgres:17 -c max_connections=500 >/dev/null
    trap 'docker rm -f "$name" >/dev/null' EXIT
    port=$(docker port "$name" 5432/tcp | head -1 | sed 's/.*://')
    # Over TCP: the image's first start answers on its socket only while it
    # sets itself up.
    for _ in $(seq 120); do docker exec "$name" pg_isready -h 127.0.0.1 -U postgres -q && break; sleep 0.5; done
    ARUGULA_TEST_POSTGRES_URL="postgres://postgres:platform@127.0.0.1:$port/postgres" \
        {{server_cargo}} nextest run --workspace -E 'package(arugula-platform)'

# Control end to end without a browser: sign in (fake GitHub), enroll,
# join a daemon, reach it through the relay and directly.
control-smoke:
    {{test_bins}}
    {{server_test_bins}}
    cd web && TARGET_DIR="{{target_dir}}/debug" {{suite_lock}} node --experimental-strip-types --no-warnings control-smoke.ts

# The swarm (M26) by hand: three throwaway daemons with scripted work on
# 7730-7732 (t: make trouble, a: an agent asks, x: quit).
fake-fleet:
    {{cargo}} build -p arugulad -p arugula
    cd web && pnpm run build && node --experimental-strip-types --no-warnings fake-fleet.ts

# The browser's end-to-end crypto (web/src/shell/e2e) against Rust's (crates/e2e).
e2e-interop:
    {{cargo}} build -p arugula-e2e --example interop
    cd web && INTEROP_BIN="{{target_dir}}/debug/examples/interop" {{suite_lock}} node --experimental-strip-types --no-warnings e2e-interop.ts

# Browser tests in system Chrome; pass a URL to test a running daemon.
e2e url="": web e2e-build
    cd web && E2E_BASE_URL="{{url}}" {{suite_lock}} pnpm exec playwright test

# A new user's journeys (#551), as a person takes them: J1 (one person,
# from no account to a terminal) and J2 (two people linking up). Each ends
# with a graph of the path, every step annotated with what went right or
# wrong, in web/journey-reports/ (local output, never committed). They run
# one worktree at a time, like `just e2e` (scripts/suite-lock).
journey which="": web e2e-build
    cd web && {{suite_lock}} pnpm exec playwright test --project=chrome e2e/journey-{{ if which == "" { "" } else { which } }}
    @echo "Graphs: web/journey-reports/ (open the .html; the .svg is the graph alone)"
    @just journey-map

# Every journey on one page (#664): a metro map of the journeys' paths and a
# grid of what each reaches, from the newest reports in web/journey-reports/
# (the real-Mac ones too), into web/journey-reports/map.html. Fails when a
# step has no station in web/e2e/journey/map.ts.
journey-map:
    cd web && node --experimental-strip-types --no-warnings e2e/journey/map.ts

# The map's design alone (no runs) into
# docs/journey-map.svg and docs/journey-steps.svg, which docs/testing.md
# shows. Run it after changing anything in web/e2e/journey/map.ts: a station,
# a line, a step (STEPS, COMPARED) or a gap (UNCOVERED).
journey-map-doc:
    cd web && node --experimental-strip-types --no-warnings e2e/journey/map.ts --design ../docs

# web/e2e/durations.json from check.yml run RUN's e2e shards (their
# e2e-timings-N artifacts), for the shards' split (web/scripts/e2e-shard.mjs).
e2e-durations run:
    #!/usr/bin/env bash
    set -euo pipefail
    d=$(mktemp -d)
    trap 'rm -rf "$d"' EXIT
    gh run download {{run}} -R arugula-salad/arugula -p 'e2e-timings-*' -D "$d"
    cd web && node scripts/e2e-durations.mjs "$d"/*/e2e-timings.json > e2e/durations.json
    echo "web/e2e/durations.json: $(grep -c '": ' e2e/durations.json) files"

# Only the WebKit specs (`*.webkit.spec.ts`), as the macOS runner runs them.
e2e-webkit: web e2e-build
    cd web && {{suite_lock}} pnpm exec playwright test --project=webkit

# What the specs run, from ../target/debug: with CARGO_TARGET_DIR elsewhere
# (CI), target is a link to it.
[private]
e2e-build:
    #!/usr/bin/env bash
    set -euo pipefail
    {{cargo}} build -p arugulad -p arugula
    {{server_cargo}} build -p arugula-control
    t="{{target_dir}}"
    if [ "$t" != "{{justfile_directory()}}/target" ]; then
      if [ -L target ] || [ ! -e target ]; then ln -sfn "$t" target
      else echo "target/ is a directory but CARGO_TARGET_DIR is $t: the specs would run stale binaries" >&2; exit 1; fi
    fi

# Playwright's browsers (Chromium and WebKit by default). On Linux their
# system libraries come too when sudo needs no password; otherwise a spec
# that can't start its browser says which are missing.
browsers *which="chromium webkit":
    #!/usr/bin/env bash
    set -euo pipefail
    cd web
    # /etc/arugula-ci/browser-deps: a runner image with their system
    # packages already in (home-cloud's gha-runner, target arugula: #868).
    if [ "$(uname -s)" = Linux ] && [ ! -e /etc/arugula-ci/browser-deps ] && sudo -n true 2>/dev/null; then
      pnpm exec playwright install --with-deps {{which}}
    else
      pnpm exec playwright install {{which}}
    fi

# Arugula's VS Code extension as a VSIX in target/ (M28), for Open VSX
# and the Marketplace (`just publish-vsix`).
# Nothing went out as `illogical.illogical-editor`, so no old id to keep.
vsix:
    {{cargo}} build -p arugulad
    {{target_dir}}/debug/arugulad _vsix {{target_dir}}

# Publish the extension as `arugula.arugula-editor` (#529): to the Visual
# Studio Marketplace (VSCE_PAT, an Azure DevOps token with Marketplace
# Manage) and Open VSX (OVSX_PAT), or just one: `just publish-vsix ovsx`.
# release.yml does this on every `v*` tag; this is the same by hand, for
# whoever holds the tokens. A version already there is skipped, so it can
# run again after one of them fails.
publish-vsix to="both":
    #!/usr/bin/env bash
    set -euo pipefail
    case "{{to}}" in both|vsce|ovsx) ;; *) echo "to: both, vsce or ovsx" >&2; exit 2 ;; esac
    want() { [ "{{to}}" = both ] || [ "{{to}}" = "$1" ]; }
    want vsce && [ -z "${VSCE_PAT:-}" ] && { echo "VSCE_PAT isn't set" >&2; exit 1; }
    want ovsx && [ -z "${OVSX_PAT:-}" ] && { echo "OVSX_PAT isn't set" >&2; exit 1; }
    {{just_executable()}} vsix
    version=$(sed -n 's/^  "version": "\(.*\)",$/\1/p' crates/daemon/src/editor/ext/package.json)
    f="{{target_dir}}/arugula-editor-$version.vsix"
    want vsce && npx -y @vscode/vsce@4.0.0 publish --skip-duplicate --packagePath "$f"
    want ovsx && npx -y ovsx@1.2.0 publish --skip-duplicate --packagePath "$f"
    echo "code --install-extension arugula.arugula-editor"

# The site's images, from a throwaway daemon with a demo HOME and a
# scripted agent (web/screenshots/). They go to apps/site/public/img in a
# checkout of arugula-salad/site: ../site, or SITE_DIR. Needs nvim for the
# editor pane. The web client first: the daemon build picks up web/dist.
screenshots:
    #!/usr/bin/env bash
    set -euo pipefail
    site="${SITE_DIR:-{{justfile_directory()}}/../site}"
    [ -d "$site/apps/site/public/img" ] || { echo "$site isn't a checkout of arugula-salad/site: set SITE_DIR" >&2; exit 1; }
    img="$(cd "$site/apps/site/public/img" && pwd)"
    (cd web && pnpm run build)
    {{cargo}} build -p arugulad -p arugula
    (cd web && SHOTS_OUT="$img" pnpm exec playwright test -c screenshots.config.ts)
    scripts/webp "$img"

# M4a for real: a wisp sprite installs the static daemon on the tailnet and
# joins a throwaway home daemon's list; the phone gets vim there. Needs
# ARUGULA_E2E_TAILNET_AUTHKEY_FILE (an ephemeral tag:sandbox key) and wispd.
e2e-sandbox: static
    {{cargo}} build -p arugulad
    cd web && pnpm exec playwright test e2e/sandbox.spec.ts

# The local Docker test stack (testnet/README.md): up|test|break|measure|down [profile] [claim...].
# The control profile builds what it runs: the static binaries for Docker's
# architecture (unless ARUGULA_TESTNET_BINARIES names others) and the CLI.
testnet cmd="test" profile="ssh" *claims:
    #!/usr/bin/env bash
    set -euo pipefail
    if [ {{profile}} = control ] && docker info >/dev/null 2>&1; then
      case "{{cmd}}" in
        up) arch=$(docker info --format '{{{{.Architecture}}')
            case "$arch" in arm64) arch=aarch64 ;; amd64) arch=x86_64 ;; esac
            [ -n "${ARUGULA_TESTNET_BINARIES:-}" ] || just static "$arch" >&2 ;;
        test|break) [ -n "${ARUGULA_CLI:-}" ] || {{cargo}} build -q -p arugula ;;
      esac
    fi
    case "{{cmd}}" in
      up) testnet/up.sh {{profile}} ;;
      test) testnet/test.sh {{profile}} {{claims}} ;;
      break) testnet/test.sh {{profile}} --break {{claims}} ;;
      measure) testnet/measure-{{profile}}.sh {{claims}} ;;
      down) testnet/down.sh ;;
      *) echo "usage: just testnet up|test|break|measure|down [profile] [claim...]" >&2; exit 2 ;;
    esac

# #17 on two Docker machines, one dropping off the network (testnet/hosts/README.md).
testnet-hosts:
    #!/usr/bin/env bash
    set -euo pipefail
    # No Docker: a failure, or with ARUGULA_SKIP_DOCKER=1 a loud skip.
    if ! docker info >/dev/null 2>&1; then testnet/hosts/net.sh check; exit $?; fi
    a=$(uname -m); [ "$a" = arm64 ] && a=aarch64
    just static "$a"
    {{cargo}} build -p arugulad
    cd web && ARUGULA_TESTNET_HOSTS=1 pnpm exec playwright test e2e/testnet-hosts.spec.ts

# M28 for real: VS Code over Remote-SSH into a Docker box (testnet/editors/README.md).
testnet-editors:
    #!/usr/bin/env bash
    set -euo pipefail
    # No Docker: a failure, or with ARUGULA_SKIP_DOCKER=1 a loud skip.
    if ! docker info >/dev/null 2>&1; then testnet/editors/box.sh check; exit $?; fi
    a=$(uname -m); [ "$a" = arm64 ] && a=aarch64
    just static "$a"
    {{cargo}} build -p arugulad
    cd web
    # Electron needs a display: a virtual one where there's none (Linux CI).
    x=(); if [ "$(uname -s)" = Linux ] && [ -z "${DISPLAY:-}" ]; then x=(xvfb-run -a); fi
    ARUGULA_TESTNET_EDITORS=1 ${x[@]+"${x[@]}"} pnpm exec playwright test e2e/editor-remote-ssh.spec.ts

# Real Forgejo and GitLab in Docker (testnet/forges/README.md): up|test|down [forgejo|gitlab|all].
forges cmd="test" forge="forgejo":
    #!/usr/bin/env bash
    set -euo pipefail
    case "{{cmd}}" in
      up) testnet/forges/up.sh {{forge}} ;;
      test) testnet/forges/test.sh {{forge}} ;;
      down) testnet/forges/down.sh {{forge}} ;;
      *) echo "usage: just forges up|test|down [forgejo|gitlab|all]" >&2; exit 2 ;;
    esac

# macOS checks in a throwaway tart VM (docs/testing.md, "A fresh Mac"):
# `just macos tools` (safari and launchd in one VM, booted once),
# each alone (`just macos launchd`, `safari`), `just macos ios`
# (the iOS Simulator, on the Xcode base: `just macos base --xcode` once;
# parked, #257), or base|up|down|ssh for the VM.
# Fails without tart or the base VM; ARUGULA_SKIP_MACOS_VM=1 skips loudly.
macos cmd="tools" *args:
    #!/usr/bin/env bash
    set -euo pipefail
    case "{{cmd}}" in
      base|up|down|ssh|ip|push|restart) exec testnet/macos/vm.sh {{cmd}} {{args}} ;;
    esac
    {{cargo}} build -p arugulad -p arugula
    {{server_cargo}} build -p arugula-control
    # web/safari serves web/dist.
    if [ "{{cmd}}" = ios ]; then just web; fi
    exec testnet/macos/test.sh {{cmd}} {{args}}

# Tests for the shell side of releases: install.sh picks the right release
# per machine, and the ratchet that what a release ships is named the same
# everywhere (release.yml, scripts/release, Homebrew, install.sh, the site);
# `just notices` keeping THIRD_PARTY.md when cargo-about fails;
# `scripts/release check-green` refusing a tag on a commit CI didn't pass;
# a release deploying the hub only when what it's built from changed; the
# hosted template's supervision of arugulad (M258); its templates, one
# per workspace size (M266); and its tools for session users (Arugula #215).
test-scripts:
    scripts/tests/ci-red.sh
    scripts/tests/install.sh
    scripts/tests/release-targets.sh
    scripts/tests/notices.sh
    scripts/tests/release-green.sh
    scripts/tests/suite-lock.sh
    scripts/tests/release-bump.sh
    scripts/tests/hub-changed.sh
    scripts/tests/hosted-supervisor.sh
    scripts/tests/hosted-template.sh
    scripts/tests/hosted-tools.sh

# The two workspaces stay apart: the root's has no server crate, and the
# server's (server/Cargo.toml) builds none of the daemon's, the mux or
# Ghostty, whatever a path dependency brings in.
workspace-guard:
    #!/usr/bin/env bash
    set -euo pipefail
    status=0
    names() { awk '{print $1}' | sort -u; }
    root=$({{cargo}} tree --workspace -e all --prefix none --target all | names)
    server=$({{server_cargo}} tree --workspace -e all --prefix none --target all | names)
    for crate in arugula-control arugula-platform arugula-control-common; do
      if grep -qx "$crate" <<<"$root"; then echo "workspace-guard: the root workspace builds $crate: it belongs to server/Cargo.toml" >&2; status=1; fi
    done
    for crate in arugulad arugula arugula-mux arugula-vt libghostty-vt-sys arugula-testkit; do
      if grep -qx "$crate" <<<"$server"; then echo "workspace-guard: the server's workspace builds $crate: it belongs to the root's" >&2; status=1; fi
    done
    exit $status

# arugula-mux names no integration (DECISIONS.md, "The mux names no
# integration"): none of these crates may be in its dependency tree, with or
# without Labs. Whole package names, so `hyper-util` isn't `hyper`.
mux-guard:
    #!/usr/bin/env bash
    set -euo pipefail
    banned='axum hyper reqwest rmcp rust-embed russh'
    status=0
    for features in "" "--features labs"; do
      tree=$({{cargo}} tree -p arugula-mux -e normal --prefix none $features)
      for crate in $banned; do
        if printf '%s\n' "$tree" | awk '{print $1}' | grep -qx "$crate"; then
          echo "mux-guard: arugula-mux depends on $crate${features:+ ($features)}: the mux names no integration (DECISIONS.md, docs/mux-crate.md)" >&2
          status=1
        fi
      done
    done
    exit $status

# What CI runs.
check: test mux-guard workspace-guard
    just web-imports
    cd web && pnpm run paths
    just proto-ts check
    just decisions check
    just control-tables check
    scripts/check-binaries
    {{cargo}} fmt --all --check
    {{server_cargo}} fmt --all --check
    {{cargo}} clippy --workspace --all-targets -- -D warnings
    {{server_cargo}} clippy --workspace --all-targets -- -D warnings

# The daemon and the CLI without Labs (#452): clippy and the tests with the
# `labs` feature off, which CI runs as the `core` job. Run it after touching
# anything in or beside crates/daemon/src/labs; `just check` covers the rest.
# The forge and the editor stay on (#461): "core" is everything but Labs.
check-core: web mux-guard
    {{cargo}} clippy -p arugulad -p arugula-mux -p arugula --no-default-features --features arugulad/forge,arugulad/editor --all-targets -- -D warnings
    {{cargo}} nextest run -p arugulad -p arugula-mux -p arugula --no-default-features --features arugulad/forge,arugulad/editor

# The daemon with none of its integrations (#461), and with only the forge
# or only the editor: clippy for each, which finds what one feature's code
# left unused in the others. Not part of `check` or CI.
check-features:
    {{cargo}} clippy -p arugulad --no-default-features --all-targets -- -D warnings
    {{cargo}} clippy -p arugulad --no-default-features --features forge --all-targets -- -D warnings
    {{cargo}} clippy -p arugulad --no-default-features --features editor --all-targets -- -D warnings

# Type-check and lint the macOS build from Linux: ARCH is aarch64 (Apple
# silicon) or x86_64 (Intel). Zig is the C compiler; this compiles but
# doesn't link, so build and test on a Mac too.
check-macos arch="aarch64":
    rustup target add {{arch}}-apple-darwin >/dev/null
    ZIG_MACOS_ARCH={{arch}} CC_{{arch}}_apple_darwin="$PWD/scripts/zig-cc-macos" AR_{{arch}}_apple_darwin="$PWD/scripts/zig-ar" \
      {{cargo}} clippy --target {{arch}}-apple-darwin --workspace --all-targets -- -D warnings
    ZIG_MACOS_ARCH={{arch}} CC_{{arch}}_apple_darwin="$PWD/scripts/zig-cc-macos" AR_{{arch}}_apple_darwin="$PWD/scripts/zig-ar" \
      {{server_cargo}} clippy --target {{arch}}-apple-darwin --workspace --all-targets -- -D warnings

# Run the daemon the way it runs for real (port 7681, behind `tailscale serve`).
run *args: build
    {{target_dir}}/release/arugulad {{args}}

# Install as a systemd user service (starts at boot with lingering).
install: build
    {{target_dir}}/release/arugulad install

# This checkout's web client as control.arugula.io serves it: a local
# control (fake GitHub sign-in) on 7690 with machines joined to it
# (default: workstation laptop), rebuilt on every change (web/dev-control.ts).
# DEV_CONTROL_APPROVE_BROWSERS=0 leaves a signed-in browser waiting.
dev-control *machines:
    {{cargo}} build -p arugulad
    {{server_cargo}} build -p arugula-control
    cd web && TARGET_DIR="{{target_dir}}/debug" node --experimental-strip-types --no-warnings dev-control.ts {{machines}}

# The platform page (M150) on 127.0.0.1:7691, beside `just dev-control`
# (run that first): Vite serves it and sends control's paths to 7690. Sign
# in on control's page; the session cookie is the same host's, so this page
# is signed in too. DEV_CONTROL_CHAT=1 on dev-control gives it a team's
# chat to show.
dev-platform:
    cd web && pnpm run dev-platform

# A headless Chrome on a page, driven by plain commands, for screenshots:
# a virtual passkey authenticator (--passkey), a login (--login), a profile
# kept between runs (--profile). Usage at the top of web/dev-browser.ts.
[no-cd]
dev-browser *args:
    node --experimental-strip-types --no-warnings {{justfile_directory()}}/web/dev-browser.ts {{args}}

# Blocks on ports (browsers, editors) listen on a free loopback port.
# Dev loop: separate daemon on 7682 + Vite on 5173; leaves the real one alone.
dev:
    {{cargo}} build -p arugulad
    trap 'kill 0' EXIT; \
      {{target_dir}}/debug/arugulad --listen 127.0.0.1:7682 --allow-origin http://localhost:5173 --block-listen 127.0.0.1:0 --state-dir ~/.local/state/arugula-dev & \
      (cd web && pnpm run dev)

# S39: 8 sessions, a six-pane tab and attention on two panes, replacing
# what's there. Run `just dev` first.
# Seed `just dev`'s daemon with a workspace to screenshot (scripts/dev-fixture).
dev-fixture:
    {{cargo}} build -p arugula
    CARGO_TARGET_DIR="{{target_dir}}" scripts/dev-fixture

# Re-record snapshot fixtures (crates/vt/fixtures).
fixtures *names:
    python3 crates/vt/fixtures/record.py {{names}}

# Re-record the client fixtures (crates/proto/fixtures, #200) from throwaway
# daemons: all of them, or the ones named (attach, run-capture, events).
record-fixtures *names:
    {{cargo}} build -p arugulad
    {{cargo}} run -q -p arugula-testkit --bin record-fixtures -- --daemon {{target_dir}}/debug/arugulad --record crates/proto/fixtures {{names}}

# Copy the client fixtures to crates/proto/fixtures/releases/VERSION, as each
# release does (docs/development.md): the daemon's tests replay the last two.
release-fixtures:
    #!/usr/bin/env bash
    set -euo pipefail
    v=$({{cargo}} pkgid -p arugulad | sed 's/.*[#@]//')
    d=crates/proto/fixtures/releases/$v
    rm -rf "$d" && mkdir -p "$d"
    cp crates/proto/fixtures/*.jsonl "$d/"
    ls "$d"

# The certificate vectors checked in for other implementations
# (crates/e2e/fixtures/certs.json, from `interop fixtures`).
e2e-vectors:
    {{cargo}} build -p arugula-e2e --example interop
    {{target_dir}}/debug/examples/interop fixtures | python3 -m json.tool --indent 2 --no-ensure-ascii > crates/e2e/fixtures/certs.json
