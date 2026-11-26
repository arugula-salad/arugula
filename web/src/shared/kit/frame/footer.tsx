// The footer (Lift 3, #1479): the place's name, this screen's keys as
// chips (the table's, shared/keys.ts, at most five), how many agents are
// running and `?` for the rest. Nothing else: what needs you is the rail's
// Inbox, and a pane's tips are its own.

import type { ComponentChildren } from "preact";
import { bindings, hintOf, keyHint, subscribeKeys } from "../../keys";
import { useSubscribe } from "../../ui/hooks";
import { Kbd } from "../kbd";
import { StateMark } from "../state-mark";
import { openKeys } from "./keys-sheet";
import "../kit.css";

/** `running`: how many agents are, where the page knows; `extra`, the
 * place's own toggles (a machine's Activity) before them. */
export function Footer({ place, running, extra }: { place: string; running?: number; extra?: ComponentChildren }) {
  useSubscribe(subscribeKeys);
  // The screen's keys were added last: they come first.
  const chips = bindings()
    .filter((b) => b.chip && hintOf(b))
    .reverse()
    .slice(0, 5);
  const all = keyHint("keys");
  return (
    <footer class="k-foot" data-status-footer>
      <span class="k-foot-place">{place}</span>
      <span class="k-foot-keys" aria-label="Shortcuts">
        {chips.map((b) => (
          <span key={b.id} class="k-foot-key" data-chip={b.id}>
            <Kbd keys={hintOf(b)!} /> {b.chip}
          </span>
        ))}
      </span>
      {extra}
      {!!running && (
        <span class="k-foot-key" data-footer-running={running}>
          <StateMark state="working" word={`${running} running`} />
        </span>
      )}
      {all && (
        <button type="button" class="k-foot-key" data-open-keys onClick={openKeys}>
          <Kbd keys={all} /> shortcuts
        </button>
      )}
    </footer>
  );
}
