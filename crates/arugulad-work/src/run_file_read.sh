# What a run left, handed to arugulad by the run user (run_files.rs): the
# daemon's own user can't open the run's home (0700, the run user's), so
# the run user hands the bytes over on its standard output, and only of a
# file that is what it says. Nothing else of the run user's runs by then
# (the daemon killed it all), so nothing changes the file between the checks
# and the read.
#
#   sh -c "$(cat run_file_read.sh)" sh FILE MAX   # its bytes, at most MAX+1
#   sh -c "$(cat run_file_read.sh)" sh -l DIR     # its names, one a line
#
# Exits 2: no such file; 3: a link, not a regular file, more than one name,
# or not the run user's own; 4: larger than MAX bytes.
set -u
if [ "$1" = -l ]; then
  cd -- "$2" 2>/dev/null || exit 0
  for f in * .[!.]* ..?*; do
    if [ -e "$f" ] || [ -L "$f" ]; then printf '%s\n' "$f"; fi
  done
  exit 0
fi
f=$1
max=$2
if [ -L "$f" ]; then exit 3; fi
if [ ! -e "$f" ]; then exit 2; fi
if [ ! -f "$f" ]; then exit 3; fi
# shellcheck disable=SC2046 # three numbers, split on purpose
set -- $(stat -c '%h %u %s' -- "$f") || exit 3
[ "$1" = 1 ] && [ "$2" = "$(id -u)" ] || exit 3
[ "$3" -le "$max" ] || exit 4
exec head -c "$((max + 1))" -- "$f"
