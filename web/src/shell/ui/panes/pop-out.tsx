// Pop a pane out into a window of its own (Arugula #497): chat, a board, a
// Doc on a second monitor, out of the crowded panes.
//
// - Pop out (the pane menu, ⇧⌘O) opens `/#pop=<daemon>.<pane>`: a new
//   window in the desktop app (crates/desktop keeps it out of the main
//   window's native tabs), a new tab in a browser.
// - That page shows only the pane. Once it's connected it sends the
//   daemon's `pop_out` intent, so it holds the pop: its client going away
//   (the window closing) brings the pane back, as a focus ends with its
//   holder (crates/core `Tab::popped`).
// - Every other screen shows a placeholder in the pane's place, with Bring
//   back. Brought back from there, the window says so and closes.
// - A block only: a terminal's size is its tab's until sizes are per pane
//   (#477), so it doesn't pop out yet.

import { useEffect, useLayoutEffect, useRef, useState } from "preact/hooks";
import type { Client } from "../../client";
import type { PaneId } from "../../../shared/proto";
import { directory } from "../../hosts";
import { useSubscribe } from "../../../shared/ui/hooks";

/** `#pop=<daemon>.<pane>`, or `#pop=<pane>` on this page's own machine. */
export const POP_HASH = /^#pop=(?:([0-9a-f]+)\.)?(\d+)$/;

/** Whether `pane` can pop out: a block, on a screen that shows panes side
 * by side, and not out already. */
export function canPopOut(client: Client, pane: PaneId, phone: boolean): boolean {
  const info = client.info(pane);
  return !phone && !!info && info.type !== "terminal" && !isPopped(client, pane);
}

/** Whether `pane` is out in a window of its own. */
export function isPopped(client: Client, pane: PaneId): boolean {
  return client.tabOfPane(pane)?.popped?.includes(pane) ?? false;
}

/** Open `pane` in a window of its own. */
export function popOut(client: Client, pane: PaneId) {
  const daemon = directory.list?.hosts.find((h) => h.name === directory.current)?.id;
  const hash = `#pop=${daemon && directory.current !== directory.home ? `${daemon}.` : ""}${pane}`;
  // A name per pane: Pop out again on the same pane brings its window forward.
  window.open(`${location.origin}/${hash}`, `arugula-pop-${daemon ?? "home"}-${pane}`);
  client.setActive(pane);
}

/** Bring `pane` back into its tab from wherever it's popped out. */
export function bringBack(client: Client, pane: PaneId) {
  client.intent({ op: "pop_out", pane, out: false });
}

/** What's in a popped pane's place in its tab, on every other screen. */
export function PoppedOut({ client, id }: { client: Client; id: PaneId }) {
  return (
    <div class="popped-out" data-popped-out={id}>
      <span class="popped-out-title">{client.title(id) || "This pane"}</span>
      <span class="dim">is in a window of its own</span>
      <button type="button" class="secondary small" data-bring-back={id} onClick={() => bringBack(client, id)}>
        Bring back
      </button>
    </div>
  );
}

type Phase = { kind: "waiting" } | { kind: "out" } | { kind: "back" } | { kind: "refused"; why: string };

/** The page a popped-out pane is alone in. */
export function PopOutPage({ client, pane, daemon }: { client: Client; pane: PaneId; daemon?: string }) {
  useSubscribe((fn) => client.subscribe(fn));
  useSubscribe((fn) => directory.subscribe(fn));
  const [phase, setPhase] = useState<Phase>({ kind: "waiting" });
  const slot = useRef<HTMLDivElement>(null);
  const sent = useRef(false);

  // Its machine, once the directory knows it.
  useEffect(() => {
    const host = daemon ? directory.list?.hosts.find((h) => h.id === daemon)?.name : (directory.home ?? undefined);
    if (host && directory.current !== host) directory.select(host);
  });

  const info = client.info(pane);
  // Once the pane is there: pop it out, held by this window.
  useEffect(() => {
    if (sent.current || !client.connected || !info) return;
    sent.current = true;
    if (info.type === "terminal") {
      setPhase({ kind: "refused", why: "A terminal can't pop out yet: its size is its tab's." });
      return;
    }
    client.intent({ op: "pop_out", pane, out: true }, (why) => setPhase({ kind: "refused", why }));
    setPhase({ kind: "out" });
  });

  // Brought back from another screen, or closed: this window's done. Only
  // once the daemon has said it's out: a state from before the intent
  // doesn't count.
  const popped = isPopped(client, pane);
  const seenOut = useRef(false);
  if (popped) seenOut.current = true;
  useEffect(() => {
    if (phase.kind !== "out" || !seenOut.current || (info && popped)) return;
    setPhase({ kind: "back" });
    window.close();
  });

  // The block's own view, moved in, as a pane slot does.
  const view = phase.kind === "out" ? client.viewOf(pane) : undefined;
  useLayoutEffect(() => {
    const el = slot.current;
    if (!view || !el) return;
    el.appendChild(view.host);
    view.setVisible(true);
    view.focus();
    return () => {
      if (view.host.parentElement === el) view.host.remove();
    };
  }, [view]);

  const title = client.title(pane);
  useEffect(() => {
    document.title = title ? `${title} · Arugula` : "Arugula";
  }, [title]);

  return (
    <div class="pop-out-page" data-pop-out-page={pane} data-phase={phase.kind}>
      {phase.kind === "out" ? (
        <div class="pop-out-slot" ref={slot} />
      ) : (
        <div class="pop-out-note">
          {phase.kind === "waiting" && <p class="dim">Opening the pane…</p>}
          {phase.kind === "back" && <p>It's back in its tab. You can close this window.</p>}
          {phase.kind === "refused" && <p>{phase.why}</p>}
        </div>
      )}
    </div>
  );
}
