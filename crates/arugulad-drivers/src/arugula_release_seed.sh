#!/usr/bin/env bash
# A fake GitHub's arugula-salad/arugula for the arugula-release driver's
# tests (M221): `seed.sh ROOT SCRIPT` makes ROOT/remotes/arugula-salad/arugula.git,
# Arugula in miniature with SCRIPT as its scripts/release, and prints the
# shas of two (green), three (not green) and a pull request's green commit.
set -euo pipefail
root=$1 script=$2
export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@t GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@t GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_NOSYSTEM=1
bare=$root/remotes/arugula-salad/arugula.git
git init -q --bare --initial-branch=main "$bare"
git -C "$bare" config uploadpack.allowAnySHA1InWant true
git -C "$bare" config uploadpack.allowFilter true
w=$root/seed
git init -q --initial-branch=main "$w"
cd "$w"
mkdir -p crates/daemon crates/desktop crates/proto/fixtures docs/releases scripts src
cp "$script" scripts/release
chmod +x scripts/release
echo '{"a session":1}' >crates/proto/fixtures/session.jsonl
printf '[workspace]\nmembers = ["crates/*"]\nexclude = ["crates/desktop"]\n\n[workspace.package]\nversion = "0.1.0"\nedition = "2024"\n' >Cargo.toml
printf '[package]\nname = "arugulad"\nversion.workspace = true\n' >crates/daemon/Cargo.toml
printf '[package]\nname = "arugula-desktop"\nversion = "0.1.0"\n' >crates/desktop/Cargo.toml
printf '[[package]]\nname = "arugulad"\nversion = "0.1.0"\n\n' >Cargo.lock
printf '[[package]]\nname = "arugula-desktop"\nversion = "0.1.0"\n\n' >crates/desktop/Cargo.lock
echo one >src/code
git add -A
git commit -qm one
echo two >src/code
git commit -qam two
two=$(git rev-parse HEAD)
echo three >src/code
git commit -qam three
three=$(git rev-parse HEAD)
for n in 0.1.1 app-0.2.0; do echo "notes $n" >"docs/releases/$n.md"; done
# A daemon release's notes have a headline section (scripts/whats-new.mjs).
printf '\n```whats-new\n[]\n```\n' >>docs/releases/0.1.1.md
git add -A
git commit -qm notes
git tag -a "ci/green/$(git rev-parse HEAD~3)" -m green HEAD~3
git tag -a "ci/green/$two" -m green "$two"
git push -q "$bare" main --tags
git checkout -q -b pr "$two"
echo pr >src/code
git commit -qam pr
pr=$(git rev-parse HEAD)
git tag -a "ci/green/$pr" -m green "$pr"
git push -q "$bare" HEAD:refs/pull/1/head "refs/tags/ci/green/$pr"
echo "$two $three $pr"
