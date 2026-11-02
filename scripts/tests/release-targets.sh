#!/usr/bin/env bash
# A ratchet on what a release ships. The targets `just dist` packs and the
# downloads the desktop recipes make are named in several other places: how
# release.yml builds them, how many scripts/release expects, the Homebrew
# formula, install.sh and the download links. Adding (or dropping) one in
# some of those places and not the others fails here, not at release time.
#
#   scripts/tests/release-targets.sh
set -euo pipefail

cd "$(dirname "$0")/../.."
fail=0
bad() { printf 'FAIL  %s\n' "$*"; fail=1; }

# The sources of truth: `just dist`'s targets, and the desktop downloads
# scripts/release requires before it publishes an app release. Each download
# must be one the build makes (`just desktop-linux ARCH` through
# packaging/desktop/build-linux.sh, `just desktop-macos ARCH`, and the
# Windows installer, which app-release.yml builds itself on GitHub's
# runner), and each one the build makes must be required. The daemon's
# release (release.yml, v*) builds the targets; the app's (app-release.yml,
# app-v*) the downloads (#393).
wf=.github/workflows/release.yml
awf=.github/workflows/app-release.yml
read -ra targets <<<"$(sed -n '/^dist:/,/^[^ ]/p' justfile | sed -n 's/^ *for t in \(.*\); do$/\1/p')"
[ "${#targets[@]}" -gt 0 ] || { echo "FAIL  can't find just dist's targets in the justfile"; exit 1; }
list=$(sed -n 's/^app_downloads="\(arugula-desktop-.*\)"$/\1/p' scripts/release)
[ -n "$list" ] || { echo "FAIL  can't find scripts/release's desktop downloads"; exit 1; }
downloads=()
# Brace expansion of scripts/release's own list; it names files only.
while read -r d; do downloads+=("$d"); done < <(eval "printf '%s\n' $list" | sort -u)
echo "targets:   ${targets[*]}"
echo "downloads: ${downloads[*]}"

linux=$(sed -n '/^desktop-linux /,/^[^ ]/p' justfile)
macos=$(sed -n '/^desktop-macos /,/^[^ ]/p' justfile)
# shellcheck disable=SC2016 # literal $arch, $name and $dist in the recipes
made=$(
  {
    read -ra arches <<<"$(sed -n 's/^ *case "\$arch" in \(.*\) esac$/\1/p' <<<"$linux" | grep -o '[a-z0-9_]*) platform' | sed 's/) platform//' | tr '\n' ' ')"
    exts=$(grep -o '"/dist/\$name\.[A-Za-z]*"' packaging/desktop/build-linux.sh | sed 's/.*\.\([A-Za-z]*\)"$/\1/' | sort -u)
    for a in "${arches[@]}"; do for e in $exts; do echo "arugula-desktop-linux-$a.$e"; done; done
    names=$(sed -n 's/.*) name=\(macos-[a-z0-9_]*\) ;;.*/\1/p' <<<"$macos")
    exts=$(grep -o '\$dist/arugula-desktop-\$name\.[a-z]*$' <<<"$macos" | sed 's/.*\.//' | sort -u)
    for n in $names; do for e in $exts; do echo "arugula-desktop-$n.$e"; done; done
    grep -o 'dist/arugula-desktop-windows-[A-Za-z0-9_.-]*\.exe' "$awf" | sed 's:^dist/::'
  } | sort -u
)
[ -n "$made" ] || { echo "FAIL  can't find what the desktop recipes make in the justfile"; exit 1; }
for m in $made; do
  printf '%s\n' "${downloads[@]}" | grep -qx "$m" || bad "the build makes $m but scripts/release doesn't require it"
done
for d in "${downloads[@]}"; do
  grep -qx "$d" <<<"$made" || bad "scripts/release requires $d but no recipe makes it"
done

# A release.yml step runs it, alone or in an `a && b` chain.
runs() { grep -Eq "^ *- run: (.* && )?$1( && .*)?$" "$wf"; }
# An app-release.yml step (or a line of one) runs it, maybe with VAR=value
# in front.
app_runs() { grep -Eq "^ *(- run: )?([A-Z_]+=[^ ]+ )*$1\$" "$awf"; }

# The daemon's release makes no app.
! grep -q 'arugula-desktop\|desktop-linux\|desktop-macos\|tauri' "$wf" || bad "$wf builds the desktop app; that's app-release.yml's (#393)"

# release.yml builds every target and every download.
for t in "${targets[@]}"; do
  case "$t" in
    *-unknown-linux-musl) step="just static ${t%%-*}" ;;
    aarch64-apple-darwin) step="just build" ;; # the macOS runner's own arch
    x86_64-apple-darwin) step="just build-macos-x86_64" ;;
    *) bad "$t: no rule for how release.yml builds it; add one here"; continue ;;
  esac
  runs "$step" || bad "$wf doesn't build $t (expected: run: $step)"
done
for d in "${downloads[@]}"; do
  case "$d" in
    arugula-desktop-linux-*) a=${d#arugula-desktop-linux-}; step="just desktop-linux ${a%%.*}" ;;
    arugula-desktop-macos-arm64.*) step="just desktop-macos aarch64" ;;
    arugula-desktop-macos-*) a=${d#arugula-desktop-macos-}; step="just desktop-macos ${a%%.*}" ;;
    # Built in the job itself; this is the step that hands it to publish.
    arugula-desktop-windows-*)
      grep -Eq "^ *path: dist/$d\\*\$" "$awf" || bad "$awf doesn't hand $d to its publish job (expected: path: dist/$d*)"
      continue ;;
    *) bad "$d: no rule for how app-release.yml makes it; add one here"; continue ;;
  esac
  app_runs "$step" || bad "$awf doesn't make $d (expected: run: $step)"
done

# What chant publishes (#1986): release/daemon's forge target attaches a
# tarball for each target, and release/app's targets every download.
dcfg=release/daemon/chant.config.ts
acfg=release/app/chant.config.ts
for t in "${targets[@]}"; do
  grep -q "\"$t\"" "$dcfg" || bad "$dcfg doesn't publish $t's tarball"
done
for d in "${downloads[@]}"; do
  grep -q "\"$d\"" "$acfg" || bad "$acfg doesn't publish $d"
done
# And nothing that isn't made.
for t in $(sed -n 's/^const targets = \[\(.*\)\];$/\1/p' "$dcfg" | tr -d '",'); do
  printf '%s\n' "${targets[@]}" | grep -qx "$t" || bad "$dcfg publishes $t, which just dist doesn't make"
done

# scripts/release counts exactly these tarballs before it writes SHA256SUMS.
# shellcheck disable=SC2016 # literal \$n in scripts/release
n=$(sed -n 's/.*\[ "\$n" = \([0-9]*\) \].*/\1/p' scripts/release)
[ "$n" = "${#targets[@]}" ] || bad "scripts/release expects ${n:-?} tarballs; just dist makes ${#targets[@]}"

# The Homebrew formula's template (chant's homebrew target, #1986) has a
# URL and, under it, a checksum for each target's tarball, and no
# placeholder chant doesn't fill: {{version}}, and {{url:NAME}} and
# {{sha256:NAME}} for a tarball the target attaches.
formula=packaging/homebrew/arugula.rb.in
for t in "${targets[@]}"; do
  name="arugula-@VERSION@-$t.tar.gz"
  grep -qF "url \"{{url:$name}}\"" "$formula" || bad "$formula has no URL for $t"
  above=$(grep -B1 -F "sha256 \"{{sha256:$name}}\"" "$formula" | head -1)
  grep -qF "{{url:$name}}" <<<"$above" || bad "$formula has no checksum for $t under its URL"
done
while read -r ph; do
  [ -n "$ph" ] || continue
  case "$ph" in
    '{{version}}') ;;
    '{{url:arugula-@VERSION@-'*'.tar.gz}}'|'{{sha256:arugula-@VERSION@-'*'.tar.gz}}')
      t=${ph#*arugula-@VERSION@-}; t=${t%.tar.gz\}\}}
      printf '%s\n' "${targets[@]}" | grep -qx "$t" || bad "$formula names $t's tarball, which just dist doesn't make" ;;
    *) bad "$formula has $ph, which chant's homebrew target doesn't fill" ;;
  esac
done < <(grep -o '{{[^}]*}}' "$formula" | sort -u)
grep -q 'assets: tarballs,' "$dcfg" || bad "$dcfg's homebrew target doesn't attach the tarballs"
# shellcheck disable=SC2016 # a literal ${dist} in the member's config
grep -qF 'template: `${dist}/arugula.rb`' "$dcfg" || bad "$dcfg's homebrew target doesn't read the formula release.yml writes"
# shellcheck disable=SC2016 # a literal $GITHUB_REF_NAME in the workflow
grep -Eq '^ *- run: scripts/release formula "\$GITHUB_REF_NAME" dist/release$' "$wf" || bad "$wf doesn't write the formula's template for the publish"

# install.sh installs every target.
for t in "${targets[@]}"; do
  grep -q "target=$t\b" scripts/install.sh || bad "scripts/install.sh never picks $t"
done
# And on a Mac the app's zip, which the app's release makes (#318).
zips=$(grep -o 'zip=arugula-desktop-[A-Za-z0-9_-]*\.zip' scripts/install.sh | sed 's/^zip=//')
[ -n "$zips" ] || bad "scripts/install.sh fetches no Mac app zip"
for z in $zips; do
  printf '%s\n' "${downloads[@]}" | grep -qx "$z" || bad "scripts/install.sh fetches $z, which no app release makes"
done

# No page here links a download that isn't made. The site and the docs for
# users live in arugula-salad/site, which checks its links against
# app-latest daily (scripts/check-code.mjs).
while IFS=: read -r f name; do
  printf '%s\n' "${downloads[@]}" | grep -qx "$name" || bad "$f links $name, which no release makes"
done < <(grep -oH 'arugula-desktop-[A-Za-z0-9_.-]*[A-Za-z0-9]' README.md docs/*.md | sort -u)

# Every archive is packed by scripts/dist-pack, and only under Arugula's
# name: every install is on 0.26 or later (#534).
# shellcheck disable=SC2016 # literal $n and $name in the scripts
sed -n '/^dist:/,/^[^ ]/p' justfile | grep -q 'scripts/dist-pack "dist/\$n"' || bad "just dist doesn't pack with scripts/dist-pack"
# shellcheck disable=SC2016
grep -q 'scripts/dist-pack "dist/\$name"' scripts/windows-dist || bad "scripts/windows-dist doesn't pack with scripts/dist-pack"
! grep -Eq 'illogical-(\[0-9|\*|\$|desktop)' "$wf" scripts/release scripts/dist-pack || bad "an old illogical-* download name is still made"

# What scripts/dist-pack makes, from stand-in binaries.
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
for t in x86_64-unknown-linux-musl x86_64-pc-windows-msvc; do
  case "$t" in *-windows-*) x=.exe e=zip ;; *) x="" e=tar.gz ;; esac
  n=arugula-1.2.3-$t
  mkdir -p "$work/$n" "$work/$t"
  echo daemon >"$work/$n/arugulad$x"; echo cli >"$work/$n/arugula$x"; echo license >"$work/$n/LICENSE-MIT"
  scripts/dist-pack "$work/$n" >/dev/null || { bad "scripts/dist-pack failed for $t"; continue; }
  [ -f "$work/$n.$e" ] || bad "scripts/dist-pack made no $n.$e"
  [ ! -e "$work/illogical-1.2.3-$t.$e" ] || bad "scripts/dist-pack still makes the old name"
  if [ "$e" = zip ]; then
    python3 -I -m zipfile -e "$work/$n.$e" "$work/$t/new"
  else
    mkdir -p "$work/$t/new"
    tar -xzf "$work/$n.$e" -C "$work/$t/new"
  fi
  got=$(cd "$work/$t/new" && find . -type f | LC_ALL=C sort | tr '\n' ' ')
  [ "$got" = "./$n/LICENSE-MIT ./$n/arugula$x ./$n/arugulad$x " ] || bad "$n.$e holds $got"
done

# What `scripts/release formula` writes for chant: the template with the
# version in the tarballs' names, and nothing of its own left.
if scripts/release formula v1.2.3 "$work/formula" >/dev/null; then
  f=$work/formula/arugula.rb
  grep -q '^class Arugula < Formula$' "$f" || bad "the formula isn't class Arugula"
  ! grep -q '@[A-Z_]*@' "$f" || bad "the formula has placeholders left: $(grep -o '@[A-Z_]*@' "$f" | tr '\n' ' ')"
  for t in "${targets[@]}"; do
    grep -qF "{{sha256:arugula-1.2.3-$t.tar.gz}}" "$f" || bad "the formula for 1.2.3 doesn't name $t's tarball"
  done
else
  bad "scripts/release formula failed"
fi
! scripts/release formula app-v1.2 "$work/formula2" >/dev/null 2>&1 || bad "scripts/release formula took a tag that isn't a version"

[ "$fail" = 0 ] && echo "release targets: all agree"
exit "$fail"
