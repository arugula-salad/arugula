// Control is moving to another address (#507): a note where the page puts
// its notes, under the bar or above the gate's screen. It used to be put
// before the page, which pushed the status footer off the window, in an
// error's red (UI audit #148): it's news, not a failure. It can be put
// away for this visit (UI audit #179), and names Arugula control as the
// other screens do.

import { useState } from "preact/hooks";
import { movingTo } from "../control";

const KEY = "arugula.moving-dismissed";

function dismissed(to: string): boolean {
  try {
    return sessionStorage.getItem(KEY) === to;
  } catch {
    return false;
  }
}

export function MovingNote() {
  const to = movingTo;
  const [gone, setGone] = useState(() => !!to && dismissed(to.host));
  if (!to || gone) return null;
  const dismiss = () => {
    setGone(true);
    try {
      sessionStorage.setItem(KEY, to.host);
    } catch {
      // Gone for this page, then.
    }
  };
  return (
    <div class="ws-stale maintenance-note control-moving" role="status" data-control-moving>
      <span>
        Arugula control is moving to <a href={to.href}>{to.host}</a>. Sign in there, then approve that browser from this one; this address stops
        working once everyone has moved.
      </span>
      <button type="button" class="link small control-moving-dismiss" data-control-moving-dismiss aria-label="Dismiss" title="Dismiss" onClick={dismiss}>
        ✕
      </button>
    </div>
  );
}
