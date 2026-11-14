// A board card's live session in the app (Arugula
// iss_c62e00eee3b4b953ff054d2951827de8): the agent's pane drawn directly,
// with a client of the app's own to its machine (through the hub with a
// machine token, or a daemon's own origin), no frame. `LivePane` draws it
// with Hand off to me and Give it back (today's driver, #118: take and
// release control, no pause and no new intent); `LiveAsk` draws what the
// pane asks, answered as Swarm's cards answer it (`/api/attention/act`). A
// chunk of its own (./live), so a board doesn't load terminals until a
// drawer shows a pane.

import "./live.css";
import { useEffect, useLayoutEffect, useRef, useState } from "preact/hooks";
import { Client, type E2ETarget } from "../shell/client";
import type { ActRequest, ActResponse } from "../shared/proto";
import { AskCard } from "../shared/ask";
import { usePhone, useSubscribe } from "../shared/ui/hooks";
import { StateChip } from "../shared/ui/state-chip";
import { PermissionBody, PermissionButtons, VIEWER_NOTE } from "../shell/ui/answer-card";
import { drawsItsOwn, mayAnswer } from "../shell/ui/term-ask";
import { KeyBar } from "../shell/ui/phone";
import type { AppSession } from "./session";

/** How the app reaches a machine by control's id for it: the session's
 * target, or a daemon's own origin (`own`, on a daemon's own page). */
export interface Reach {
  session: AppSession | null;
  own?: (machine: string) => string | undefined;
}

/** The client of each machine a drawer shows, shared by its pane and its question, closed a little after the last lets go. */
const held = new Map<string, { client: Client; n: number; timer?: ReturnType<typeof setTimeout> }>();
const started = new WeakSet<AppSession>();

function where(machine: string, reach: Reach): { base: string; target?: E2ETarget } | "wait" | null {
  const s = reach.session;
  if (!s) {
    const base = reach.own?.(machine);
    return base === undefined ? null : { base };
  }
  if (!started.has(s)) {
    started.add(s);
    s.start();
  }
  if (s.reachable === false) return null;
  const target = s.target(machine);
  if (target) return { base: `e2e:${machine}`, target };
  return s.phase === "loading" || s.reachable === null ? "wait" : null;
}

function hold(machine: string, at: { base: string; target?: E2ETarget }): Client {
  const h = held.get(machine);
  if (h) {
    clearTimeout(h.timer);
    h.n++;
    return h.client;
  }
  const client = new Client(at.base, at.target);
  client.connect();
  held.set(machine, { client, n: 1 });
  return client;
}

function letGo(machine: string) {
  const h = held.get(machine);
  if (!h || --h.n > 0) return;
  h.timer = setTimeout(() => {
    if (h.n > 0) return;
    h.client.close();
    held.delete(machine);
  }, 2000);
}

// For end-to-end tests: the clients the drawer holds.
Object.assign(window, { __live: { client: (machine: string) => held.get(machine)?.client } });

/** The machine's client while the component is up: null while it is
 * reached, "unreachable" when the app can't reach it from here. */
function useLive(machine: string, reach: Reach): Client | null | "unreachable" {
  const s = reach.session;
  useSubscribe((fn) => s?.subscribe(fn) ?? (() => {}));
  const at = where(machine, reach);
  const ready = at !== null && at !== "wait";
  const [client, setClient] = useState<Client | null>(null);
  const [, tick] = useState(0);
  useEffect(() => {
    if (!ready) return;
    const c = hold(machine, at);
    setClient(c);
    // Each change the machine announces redraws it (useSubscribe would hear only the first client).
    const off = c.subscribe(() => tick((n) => n + 1));
    return () => {
      off();
      setClient(null);
      letGo(machine);
    };
  }, [machine, ready]);
  if (at === null) return "unreachable";
  return client;
}

/** The pane, in a fixed slot under the drawer's state card. */
export function LivePane({ machine, pane, reach }: { machine: string; pane: string; reach: Reach }) {
  const c = useLive(machine, reach);
  const client = c === "unreachable" ? null : c;
  const id = Number(pane);
  const ref = useRef<HTMLDivElement>(null);
  const phone = usePhone();
  const info = client?.connected ? client.info(id) : undefined;
  const view = info ? client!.viewOf(id) : undefined;
  useLayoutEffect(() => {
    const slot = ref.current;
    if (!view || !slot) return;
    slot.appendChild(view.host);
    view.setVisible(true);
    return () => {
      if (view.host.parentElement !== slot) return;
      view.host.remove();
      view.setVisible(false);
    };
  }, [view]);
  // The key bar sends to the client's active pane: this one.
  useEffect(() => void (info && client!.setActive(id)), [client, !!info]);
  if (c === "unreachable")
    return (
      <p class="dim" data-live-unreachable>
        Its machine can't be reached from here.
      </p>
    );
  if (!client?.connected)
    return (
      <p class="dim" data-live-reaching>
        Reaching its machine…
      </p>
    );
  if (!info)
    return (
      <p class="dim" data-live-gone>
        Its pane isn't on its machine now.
      </p>
    );
  const driving = info.driver?.who === client.me();
  const other = client.drivenBy(id);
  // On a teammate's computer you watch: its owner hands it off there.
  const watching = !client.ownsMachine() || !mayAnswer(client, id);
  return (
    <div class={driving ? "live-pane driving" : "live-pane"} data-live-pane={pane} data-live-driving={driving ? "" : undefined}>
      <div class="board-line">
        {driving ? (
          <StateChip tone="accent" data-live-you-drive>
            you are driving
          </StateChip>
        ) : other ? (
          <span class="dim" data-live-driver>
            {other.name.split("@")[0]} is driving
          </span>
        ) : null}
        {watching ? null : driving ? (
          <button type="button" class="secondary small" data-live-give-back title="Let go of control: the agent goes on as before" onClick={() => client.paneOp(id, { op: "release_control" })}>
            Give it back
          </button>
        ) : (
          <button
            type="button"
            class="primary small"
            data-live-hand-off
            title="Take control of this pane: your typing reaches it"
            onClick={() => {
              client.paneOp(id, { op: "take_control" });
              client.setActive(id);
              view?.focus();
            }}
          >
            Hand off to me
          </button>
        )}
      </div>
      {watching && (
        <p class="dim" data-live-watching>
          You're watching: this runs on a teammate's computer, so Hand off is theirs. Open it on its machine to ask them.
        </p>
      )}
      <div ref={ref} class="live-slot" data-live-slot={pane} data-keys-own={driving ? "" : undefined} />
      {driving && phone && <KeyBar client={client} />}
    </div>
  );
}

/** What the pane asks, at the drawer's top, with its choices. */
export function LiveAsk({ machine, pane, reach }: { machine: string; pane: string; reach: Reach }) {
  const c = useLive(machine, reach);
  const [err, setErr] = useState<string | null>(null);
  const client = c === "unreachable" ? null : c;
  const id = Number(pane);
  const ask = client?.connected ? client.info(id)?.ask : undefined;
  if (!client || !ask || drawsItsOwn(ask)) return null;
  const can = mayAnswer(client, id);
  const who = ask.agent ?? "Claude Code";
  const act = async (action: ActRequest["action"], extra: Partial<ActRequest> = {}) => {
    try {
      const res = await client.request("POST", "/api/attention/act", { action, pane: id, id: ask.id, ...extra } satisfies ActRequest);
      setErr(res.ok ? null : ((await res.json<Partial<ActResponse> & { error?: string }>().catch(() => null))?.error ?? `couldn't (${res.status})`));
    } catch (e) {
      setErr(String(e));
    }
  };
  return (
    <section class="board-live-ask" data-board-drawer-ask={ask.id}>
      <h3>{ask.kind === "permission" ? `${who} wants to use a tool` : `${who} asks`}</h3>
      {ask.kind === "permission" ? (
        <>
          <PermissionBody ask={ask} />
          {can ? <PermissionButtons ask={ask} act={(action, extra) => void act(action, extra)} /> : <p class="ask-viewer">{VIEWER_NOTE}</p>}
        </>
      ) : !can ? (
        <div class="ask" data-ask={ask.id}>
          <p class="ask-message">{ask.questions?.[0]?.question ?? ask.message}</p>
          <p class="ask-viewer">{VIEWER_NOTE}</p>
        </div>
      ) : (
        <AskCard key={ask.id} ask={ask} actions={{ answer: (content) => void act("answer", { content }), decline: () => void act("deny") }} />
      )}
      {err && <p class="board-bad">{err}</p>}
    </section>
  );
}
