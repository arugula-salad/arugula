#!/usr/bin/env bash
# S42 q1 (prototype, not for main): a session on a hosted box, by hand, as
# M255's daemon would do it. Run on the box as `user` (the daemon's user),
# in a pane, against the box's own daemon:
#
#   bash session-proto.sh > ~/s42-q1.txt 2>&1
#
# The template's S42 layer (template-s42.Dockerfile) made ses1..ses3 (no
# sudo, not in `user`'s group, homes 0700) and put fake_acp.py in /opt/s42.
#
# 1. A checkout of the session's own: the box's clone copied by `user`,
#    handed to ses1 (sudo chown), on branch arugula/<ses>-<slug>; and
#    why a `git worktree` of the box's clone doesn't work across users.
# 2. An ACP agent block whose agent runs as ses1: `sudo -n -u ses1 -- env
#    -i … python3 fake_acp.py`, with the session's directory as its cwd.
# 3. Through it: a turn, a permission, a question, work that commits, the
#    block's state and turn_result, and Stop.
# 4. What's dangerous for M255/M259 to get wrong: git run as `user` in a
#    directory the agent owns, a secret on a command line, and loopback.
set -u
SES=ses_s42demo
SLUG=fix-readme
U=ses1
W=/home/$U/work/$SES
SRC=/home/user/s42-repo
A="arugula"
say() { printf '\n== %s\n' "$*"; }
run() { printf '$ %s\n' "$*"; "$@" 2>&1; printf '(exit %s)\n' "$?"; }

say "the box: who, and the session users"
run id
run getent passwd ses1 ses2 ses3
run ls -ld /home/user /home/ses1 /home/user/.local/state/arugula
run sudo -n -l -U ses1

say "a repo standing in for the workspace's clone (setup's)"
rm -rf "$SRC"; mkdir -p "$SRC"; cd "$SRC" || exit 1
git init -q -b main . && git config user.email box@example.invalid && git config user.name box
echo "# demo" > README.md && git add README.md && git commit -qm init
git remote add origin https://github.com/example/demo.git
run git log --oneline

say "1a. a git worktree of it for ses1: refused"
sudo -n mkdir -p /home/$U/work && sudo -n chown $U:$U /home/$U/work
run git worktree add -b "arugula/$SES-$SLUG-wt" /tmp/s42-wt
run sudo -n chown -R $U:$U /tmp/s42-wt
run sudo -n -u $U git -C /tmp/s42-wt commit --allow-empty -qm "as ses1 in a worktree"
git worktree remove --force /tmp/s42-wt 2>/dev/null; sudo -n rm -rf /tmp/s42-wt; git branch -D "arugula/$SES-$SLUG-wt" -q 2>/dev/null

say "1b. a checkout of its own: cloned by user, handed to ses1"
stage=$(mktemp -d /tmp/s42-stage.XXXXXX)
run git clone -q --no-hardlinks "$SRC" "$stage/repo"
git -C "$stage/repo" remote set-url origin "$(git -C "$SRC" remote get-url origin)"
run sudo -n chown -R $U:$U "$stage/repo"
run sudo -n mv "$stage/repo" "$W"
rmdir "$stage"
run sudo -n -u $U git -C "$W" config user.email "$SES@sessions.invalid"
run sudo -n -u $U git -C "$W" config user.name "Arugula session"
run sudo -n -u $U git -C "$W" switch -q -c "arugula/$SES-$SLUG"
run sudo -n -u $U git -C "$W" status -sb
run ls -ld "$W"

say "2. an agent block whose agent runs as $U"
ACP="sudo -n -u $U -- env -i HOME=/home/$U USER=$U LOGNAME=$U PATH=/usr/local/bin:/usr/bin:/bin LANG=C.UTF-8 FAKE_ACP_DIR=/home/$U/.fake-acp /usr/bin/python3 /opt/s42/fake_acp.py"
out=$($A agent --acp "$ACP" --cwd "$W" --title "session $SES" hello 2>&1); echo "$out"
B=$(grep -o '%[0-9]*' <<< "$out" | head -1)
run $A wait "$B" --idle --timeout 30
run $A tail "$B"
say "who the agent runs as, from the daemon's side"
for p in $(pgrep -f /opt/s42/fake_acp.py); do ps -o pid,ppid,user,pgid,cmd -p "$p" --no-headers; done
run ps -o pid,ppid,user,pgid,cmd --ppid "$(pgrep -f '_shim.*fake_acp' | head -1)" --no-headers

say "3a. a permission: run a command"
run $A call "$B" send '{"text": "run touch /home/'"$U"'/ran-it"}'
sleep 2
run $A call "$B" state
run $A call "$B" approve '{}'
run $A wait "$B" --idle --timeout 30
run sudo -n ls -l /home/$U/ran-it

say "3b. a question"
run $A call "$B" send '{"text": "ask one"}'
sleep 2
run $A call "$B" state
run $A call "$B" answer '{"question_0": "Blue"}'
run $A wait "$B" --idle --timeout 30

say "3c. work that commits, and turn_result"
run $A call "$B" send '{"text": "work README.md NOTES.md"}'
run $A wait "$B" --idle --timeout 30
run $A call "$B" turn_result
run sudo -n -u $U git -C "$W" log --oneline -3
run sudo -n -u $U git -C "$W" status --porcelain

say "3d. Stop: a slow turn, cancelled, then the block closed"
run $A call "$B" send '{"text": "slow"}'
sleep 2
run $A call "$B" cancel '{}'
run $A wait "$B" --idle --timeout 30
run $A close "$B"
sleep 4
run pgrep -u $U -a
say "what's left as $U after Stop (kill_all would take it)"
run sudo -n -u $U pkill -KILL -u $U

say "4a. git as user in the session's checkout: can't even get in (0700)"
run git -C "$W" status --porcelain
say "4b. as root (sudo git, the easy fix): refused, dubious ownership"
sudo -n -u $U git -C "$W" config core.fsmonitor "id -un > /tmp/s42-fsmonitor-ran-as; false"
run sudo -n git -C "$W" status --porcelain
say "4b'. ...and with safe.directory, the agent's config runs as root"
run sudo -n git -c safe.directory="$W" -C "$W" status --porcelain
run cat /tmp/s42-fsmonitor-ran-as
sudo -n rm -f /tmp/s42-fsmonitor-ran-as
say "4c. the same git as $U: harmless to user"
run sudo -n -u $U git -C "$W" status --porcelain
run cat /tmp/s42-fsmonitor-ran-as
sudo -n -u $U git -C "$W" config --unset core.fsmonitor

say "4d. a secret on sudo's command line: another session user reads it"
sudo -n -u $U -- env -i S42_TOKEN=not-a-real-token sleep 30 &
sleep 1
run sudo -n -u ses2 sh -c 'for p in $(pgrep -x sudo); do tr "\0" " " < /proc/$p/cmdline; echo; done'
kill %1 2>/dev/null; sudo -n pkill -u $U sleep

say "4e. loopback from $U: the daemon's port and the provider's"
run sudo -n -u $U bash -c 'for p in 7681 49983; do (exec 3<>/dev/tcp/127.0.0.1/$p) 2>/dev/null && echo "port $p open" || echo "port $p closed"; done'
run sudo -n -u $U ls /home/user/.local/state/arugula
run sudo -n nft list ruleset

say "done"
