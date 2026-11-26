#!/bin/sh
# A pane for the desktop tests (app.sh, the daemon's --shell): it records
# where it started, its size while it runs, and every byte typed into it,
# under $REC_DIR/<pid>/.
d=$REC_DIR/$$
mkdir -p "$d"
pwd >"$d/cwd"
(while :; do stty size </dev/tty >"$d/size.new" 2>/dev/null && mv "$d/size.new" "$d/size"; sleep 0.3; done) &
stty raw -echo
# Appended, so a check that empties the file mid-run reads only what came
# after: at dd's own offset it would read a hole of zeros first.
exec dd bs=1 oflag=append conv=notrunc of="$d/keys" 2>/dev/null
