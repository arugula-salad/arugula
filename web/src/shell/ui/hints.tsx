// Where hints are drawn (../hints, DECISIONS.md "Hints come from facts,
// once per account"): a chip in a pane's header, a toast, or a view's
// empty state. Each says its one line, its action and ×, and tells the
// hints it's there, so a hint is only shown where something draws it.

import { useEffect } from "preact/hooks";
import type { Client } from "../client";
import type { PaneId } from "../../shared/proto";
import { useSubscribe } from "../../shared/ui/hooks";
import { act, dismiss, mountPlace, shownHint, subscribeHints, watchHints, type Place } from "../hints";

/** Watches the shown machine for facts; once, in the workspace. */
export function HintsWatch({ client }: { client: Client }) {
  useEffect(() => watchHints(client), [client]);
  return null;
}

function useHint(place: Place, pane?: PaneId) {
  useSubscribe(subscribeHints);
  const key = place === "chip" ? `chip:${pane}` : place;
  useEffect(() => mountPlace(key), [key]);
  const s = shownHint();
  return s && s.hint.place === place && (place !== "chip" || s.fact.pane === pane) ? s : null;
}

function Body({ client, cls }: { client: Client; cls: string }) {
  const s = shownHint()!;
  return (
    <span class={cls} role="status" data-hint={s.hint.id}>
      <span class="hint-copy">{s.hint.copy(s.fact)}</span>
      {s.hint.action && (
        <button class="hint-act" data-hint-act onClick={() => act(client)}>
          {s.hint.action.label}
        </button>
      )}
      <button class="hint-dismiss" data-hint-dismiss aria-label="Dismiss this tip" title="Dismiss (Tips in Settings turns them all off)" onClick={dismiss}>
        ×
      </button>
    </span>
  );
}

/** In pane `pane`'s header. */
export function HintChip({ client, pane }: { client: Client; pane: PaneId }) {
  return useHint("chip", pane) ? <Body client={client} cls="hint hint-chip" /> : null;
}

/** Over the workspace, at the bottom. */
export function HintToast({ client }: { client: Client }) {
  return useHint("toast") ? <Body client={client} cls="hint hint-toast" /> : null;
}

/** In a view's empty state. */
export function HintEmpty({ client }: { client: Client }) {
  return useHint("empty") ? <Body client={client} cls="hint hint-empty" /> : null;
}
