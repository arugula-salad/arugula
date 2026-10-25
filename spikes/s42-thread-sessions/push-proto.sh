#!/usr/bin/env bash
# S42 q2 (prototype, not for main): the push M259 needs, with no real push.
# Run on a box built with template-s42.Dockerfile, as `user` (the daemon's
# user). The "remote" is a local bare repo behind git's HTTP-less file
# transport, and the "token" is a fake one; what's checked is who runs what,
# which ref moves, and where the token is.
#
# The session's checkout is ses1's, and the agent controls everything in
# it, its .git/config and hooks included. So the daemon never runs git
# there as itself: ses1 writes a bundle of the one branch, and the daemon
# fetches that bundle into a repo of its own and pushes from there, with
# an explicit refspec, no hooks, no config but its own, and the token for
# that one call through a credential helper that reads it from the
# environment.
set -u
U=ses1
SES=ses_s42push
REF="arugula/$SES-fix-readme"
W=/home/$U/work/$SES
REMOTE=/tmp/s42-remote.git
MIRROR=/home/user/.local/state/s42-push/mirror.git
TOKEN=fake-token-s42
say() { printf '\n== %s\n' "$*"; }
run() { printf '$ %s\n' "$*"; "$@" 2>&1; printf '(exit %s)\n' "$?"; }

say "setup: the remote (as GitHub), and the session's checkout (ses1's)"
rm -rf "$REMOTE" /tmp/s42-src; sudo -n rm -rf "$W"
git init -q --bare -b main "$REMOTE"
git init -q -b main /tmp/s42-src && cd /tmp/s42-src && git config user.email box@example.invalid && git config user.name box
echo "# demo" > README.md && git add README.md && git commit -qm init && git push -q "$REMOTE" main
cd /home/user || exit 1
sudo -n mkdir -p /home/$U/work && sudo -n chown $U:$U /home/$U/work
st=$(mktemp -d); git clone -q --no-hardlinks /tmp/s42-src "$st/repo"; sudo -n chown -R $U:$U "$st/repo"; sudo -n mv "$st/repo" "$W"; rmdir "$st"
as() { sudo -n -u $U -- env -i HOME=/home/$U PATH=/usr/bin:/bin "$@"; }
as git -C "$W" config user.email "$SES@sessions.invalid"; as git -C "$W" config user.name "Arugula session"
as git -C "$W" switch -q -c "$REF"
as sh -c "cd $W && echo fixed >> README.md && git commit -qam 'fix readme' && git checkout -q -b evil && echo evil > EVIL && git add EVIL && git commit -qm evil && git checkout -q $REF"

say "the agent plants things a careless push would run or follow"
as sh -c "cd $W && printf '#!/bin/sh\nid -un > /tmp/s42-hook-ran-as\n' > .git/hooks/pre-push && chmod +x .git/hooks/pre-push"
as git -C "$W" config core.fsmonitor "id -un > /tmp/s42-fsmonitor-ran-as; false"
as git -C "$W" config remote.origin.pushurl "$REMOTE"
as git -C "$W" config remote.origin.push "+refs/heads/*:refs/heads/*"
as git -C "$W" config credential.helper "!f() { cat > /tmp/s42-helper-saw; }; f"
run as git -C "$W" config --list --local

say "1. ses1 writes a bundle of its one branch (git runs as ses1; what it plants runs as ses1)"
B=/home/$U/out-$SES.bundle
run as git -C "$W" bundle create "$B" "refs/heads/$REF"

say "2. the daemon takes the bundle's bytes (read as ses1, written by user) and checks it names just that ref"
mkdir -p "$(dirname "$MIRROR")"; [ -d "$MIRROR" ] || git init -q --bare "$MIRROR"
sudo -n -u $U cat "$B" > /tmp/s42-in.bundle
run git -C "$MIRROR" bundle list-heads /tmp/s42-in.bundle
heads=$(git -C "$MIRROR" bundle list-heads /tmp/s42-in.bundle | awk '{print $2}')
[ "$heads" = "refs/heads/$REF" ] && echo "only refs/heads/$REF: ok" || echo "REFUSED: the bundle names $heads"

say "3. fetched into the daemon's own repo, then pushed: one refspec, no hooks, no repo config of the agent's, token for this call only"
GIT_SAFE="env -i HOME=/nonexistent PATH=/usr/bin:/bin GIT_CONFIG_NOSYSTEM=1 GIT_CONFIG_GLOBAL=/dev/null GIT_TERMINAL_PROMPT=0"
run $GIT_SAFE git -C "$MIRROR" -c core.hooksPath=/dev/null fetch -q /tmp/s42-in.bundle "+refs/heads/$REF:refs/heads/$REF"
push() {
  local ref=$1
  case "$ref" in arugula/"$SES"-*) ;; *) echo "REFUSED: $ref isn't arugula/$SES-…"; return 1 ;; esac
  # The helper reads the token from its environment: never a file, never argv.
  $GIT_SAFE S42_TOKEN="$TOKEN" git -C "$MIRROR" -c core.hooksPath=/dev/null -c credential.helper= \
    -c 'credential.helper=!f() { echo username=x-access-token; echo "password=$S42_TOKEN"; }; f' \
    push --no-follow-tags --porcelain "$REMOTE" "refs/heads/$ref:refs/heads/$ref"
}
run push "$REF"
run push main
run push "arugula/ses_other-x"
run git -C "$REMOTE" for-each-ref --format='%(refname) %(objectname:short) %(subject)'

say "nothing the agent planted ran as user"
for f in /tmp/s42-hook-ran-as /tmp/s42-fsmonitor-ran-as /tmp/s42-helper-saw; do printf '%s: ' "$f"; cat "$f" 2>/dev/null || echo "(not there)"; done
say "the helper hands git the token from its environment (what an https push asks it)"
printf 'protocol=https\nhost=github.com\n\n' | $GIT_SAFE S42_TOKEN="$TOKEN" git -c credential.helper= \
  -c 'credential.helper=!f() { echo username=x-access-token; echo "password=$S42_TOKEN"; }; f' credential fill | sed 's/password=.*/password=<the token>/'
say "the token isn't on disk"
printf '$ grep -rlF <token> /home/user /tmp /home/ses1 (as root)\n'
sudo -n grep -rlF "$TOKEN" /home/user /tmp /home/$U 2>/dev/null || echo "(none)"
sudo -n rm -f /tmp/s42-hook-ran-as /tmp/s42-fsmonitor-ran-as /tmp/s42-helper-saw /tmp/s42-in.bundle "$B"
say "done"
