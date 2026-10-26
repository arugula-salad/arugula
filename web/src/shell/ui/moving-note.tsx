// Control is moving to another address (#507): a note where the page puts
// its notes, under the bar or above the gate's screen. It used to be put
// before the page, which pushed the status footer off the window, in an
// error's red (UI audit #148): it's news, not a failure.

import { movingTo } from "../control";

export function MovingNote() {
  const to = movingTo;
  if (!to) return null;
  return (
    <div class="ws-stale maintenance-note control-moving" role="status" data-control-moving>
      Control is moving to <a href={to.href}>{to.host}</a>. Sign in there, then approve that browser from this one; this address stops working once
      everyone has moved.
    </div>
  );
}
