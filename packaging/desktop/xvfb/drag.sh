# shellcheck shell=bash
# Dragging the app's window by a page's drag strip, for app.sh and
# platform.sh (sourced). Needs xdotool and a window manager (Openbox).

# drag WINDOW X Y: press at X,Y in WINDOW and move the pointer right and
# down a little at a time, as a hand would, until the window has followed
# at least 100 right and 50 down (or 10 s pass); then let go. If the
# window never moved, press again, up to 3 presses. Sets X and Y to where
# the window ended up, x0 and y0 to where it started, and presses to how
# many it took (`dragged` says whether it moved).
#
# The page doesn't move the window itself: its mousedown asks the app over
# IPC to start the move, and the app asks the window manager, which then
# follows the pointer. On a loaded machine that comes late: a quick fixed
# drag (the button up 0.8 s after it went down) was often over first, and
# the window stayed put (CI, 2270e2f2; 3 runs in 4 with the container held
# to a fifth of a CPU). Held until the window follows, that's gone. On such
# a machine a first press now and then starts no move at all, even held
# 10 s, while the same press again does (about 1 run in 6, there): that's
# what the second and third presses are for. The checks print how many it
# took, so a drag that gets worse shows.
drag() {
  local w=$1 _
  eval "$(xdotool getwindowgeometry --shell "$w")"
  x0=$X y0=$Y
  for presses in 1 2 3; do
    xdotool mousemove --window "$w" "$2" "$3" mousedown 1
    for _ in $(seq 40); do
      xdotool sleep 0.25 mousemove_relative 15 8
      eval "$(xdotool getwindowgeometry --shell "$w")"
      if dragged; then break; fi
    done
    xdotool mouseup 1
    # Where the window manager left it, once the move is over.
    sleep 0.3
    eval "$(xdotool getwindowgeometry --shell "$w")"
    if [ "$X" != "$x0" ] || [ "$Y" != "$y0" ]; then return; fi
  done
}

# dragged: the window is at least 100 right and 50 down from x0,y0.
dragged() { [ $((X - x0)) -ge 100 ] && [ $((Y - y0)) -ge 50 ]; }

# How many presses the last drag took, for the checks to say: "" for one.
pressed() { [ "$presses" = 1 ] || echo " ($presses presses)"; }
