// Where hints are drawn (../hints, DECISIONS.md "Hints come from facts,
// once per account"): a chip in a pane's header, a toast, a view's empty
// state, or a pane modal over its pane. Each says its copy, its action and
// ×, and tells the hints it's there, so a hint is only shown where
// something draws it.

import { useEffect, useLayoutEffect, useRef, useState } from "preact/hooks";
import type { Client } from "../client";
import type { PaneId } from "../../shared/proto";
import { useSubscribe } from "../../shared/ui/hooks";
import { useModal } from "../../shared/ui/modal";
import { act, dismiss, mountPlace, shownHint, subscribeHints, watchHints, type Fact, type Hint, type Line, type Place } from "../hints";

/** Watches the shown machine for facts; once, in the workspace. */
export function HintsWatch({ client }: { client: Client }) {
  useEffect(() => watchHints(client), [client]);
  return null;
}

function useHint(place: Place, pane?: PaneId) {
  useSubscribe(subscribeHints);
  const key = place === "chip" || place === "pane-modal" ? `${place}:${pane}` : place;
  useEffect(() => mountPlace(key), [key]);
  const s = shownHint();
  return s && s.hint.place === place && (pane === undefined || s.fact.pane === pane) ? s : null;
}

const label = (a: { label: string | ((f: Fact) => string) }, f: Fact) => (typeof a.label === "string" ? a.label : a.label(f));

function Body({ client, cls }: { client: Client; cls: string }) {
  const s = shownHint()!;
  return (
    <span class={cls} role="status" data-hint={s.hint.id}>
      <span class="hint-copy">{s.hint.copy(s.fact)}</span>
      {s.hint.action && (
        <button class="hint-act" data-hint-act onClick={() => act(client)}>
          {label(s.hint.action, s.fact)}
        </button>
      )}
      <button class="hint-dismiss" data-hint-dismiss aria-label="Dismiss this tip" title="Dismiss (Tips in Settings turns them all off)" onClick={() => dismiss()}>
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

/** Over pane `pane`, between its header and its edges: a big tip. Drawn
 * by a computer's pane slots only, never a phone's. */
export function HintPaneModal({ client, pane }: { client: Client; pane: PaneId }) {
  const s = useHint("pane-modal", pane);
  return s ? <PaneModal key={s.hint.id} client={client} pane={pane} hint={s.hint} fact={s.fact} /> : null;
}

function Text({ line }: { line: Line }) {
  if (typeof line === "string") return <>{line}</>;
  return <>{line.map((x) => (typeof x === "string" ? x : <kbd>{x.kbd}</kbd>))}</>;
}

function PaneModal({ client, pane, hint, fact }: { client: Client; pane: PaneId; hint: Hint; fact: Fact }) {
  const ref = useRef<HTMLDivElement>(null);
  const [never, setNever] = useState(false);
  const optOut = useRef(never);
  optOut.current = never;
  // The pane under it takes nothing (useModal's `covers`): no clicks, keys
  // or focus, a frame's neither, until it's answered. Escape is its ×.
  useModal(ref, () => dismiss(optOut.current), true, () => client.viewOf(pane)?.host ?? null);
  // A pane whose focus wasn't in it (useModal gives back only what was)
  // gets its view's, when it's the one with the keys.
  useLayoutEffect(
    () => () => {
      queueMicrotask(() => {
        const at = document.activeElement;
        if ((!at || at === document.body) && client.active() === pane) client.viewOf(pane)?.focus();
      });
    },
    [],
  );
  const title = `pane-tip-title-${pane}`;
  return (
    <div
      class="pane-tip-backdrop"
      data-pane-tip-backdrop
      // A click on the pane under it lands here, and the keys come in
      // (not to the page's body, where the click would leave them).
      onMouseDown={(e) => {
        if (e.target !== e.currentTarget) return;
        e.preventDefault();
        ref.current?.focus({ preventScroll: true });
      }}
      // Not the pane's menu: its pane takes nothing while it's up.
      onContextMenu={(e) => e.stopPropagation()}
    >
      <div ref={ref} class="pane-tip" role="dialog" aria-modal="true" aria-labelledby={title} data-hint={hint.id} data-pane-tip={pane}>
        <button class="modal-close" data-hint-dismiss aria-label="Close this tip" title="Close" onClick={() => dismiss(never)}>
          ×
        </button>
        <h2 id={title}>{hint.copy(fact)}</h2>
        {hint.body?.(fact).map((b) =>
          typeof b === "object" && !Array.isArray(b) ? (
            <ul>
              {b.list.map((l) => (
                <li>
                  <Text line={l} />
                </li>
              ))}
            </ul>
          ) : (
            <p>
              <Text line={b} />
            </p>
          ),
        )}
        <div class="pane-tip-foot">
          <label class="pane-tip-never">
            <input type="checkbox" data-pane-tip-never checked={never} onChange={(e) => setNever(e.currentTarget.checked)} />
            Don't show tips like this again
          </label>
          <span class="pane-tip-fill" />
          {hint.second && (
            <button class="pane-tip-second" data-hint-second onClick={() => act(client, "second", never)}>
              {hint.second.label}
            </button>
          )}
          {hint.action && (
            <button class="pane-tip-act primary" data-hint-act onClick={() => act(client, "action", never)}>
              {label(hint.action, fact)}
            </button>
          )}
        </div>
      </div>
    </div>
  );
}
