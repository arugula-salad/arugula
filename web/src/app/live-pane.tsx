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
import type { ActRequest, ActResponse } from "../shared/proto";
import { askNeed, NeedRow } from "../shared/kit/need-row";
import { needAge } from "../shared/needs-you";
import { usePhone } from "../shared/ui/hooks";
import { StateChip } from "../shared/ui/state-chip";
import { PermissionBody, PermissionButtons, VIEWER_NOTE } from "../shell/ui/answer-card";
import { askedOf, drawsItsOwn, mayAnswer } from "../shell/ui/term-ask";
import { KeyBar } from "../shell/ui/phone";
import { pickCard } from "../shell/ui/pane-card";
import { usePlatformApi } from "../shared/platform-api";
import { useLive, type Reach } from "./machine-client";

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

/** A pane as the agent panel's Terminal draws it (Lift 4, #1480; the
 * Overview's Work level's before, #1289): the same view Panes mounts,
 * filling its slot, typed into where you may.
 * `watch` holds your keys back (a teammate's session); so does a pane
 * whose session you only view. */
export function WorkSlot({ machine, pane, reach, watch }: { machine: string; pane: string; reach: Reach; watch?: boolean }) {
  const c = useLive(machine, reach);
  const client = c === "unreachable" ? null : c;
  const id = Number(pane);
  const ref = useRef<HTMLDivElement>(null);
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
  // Its stream comes though its tab isn't the one the client shows.
  useEffect(() => (info ? client!.drawAlone(id) : undefined), [client, !!info, id]);
  const readOnly = !!watch || (!!info && !mayAnswer(client!, id));
  // A pane you watch takes no keys: they stop here, before its terminal hears them.
  useEffect(() => {
    const slot = ref.current;
    if (!slot || !readOnly) return;
    const stop = (e: Event) => {
      e.stopImmediatePropagation();
      e.preventDefault();
    };
    const kinds = ["keydown", "keypress", "beforeinput", "input", "paste", "drop", "compositionend"];
    for (const k of kinds) slot.addEventListener(k, stop, true);
    return () => kinds.forEach((k) => slot.removeEventListener(k, stop, true));
  }, [readOnly, !!view]);
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
  return <div ref={ref} class="work-slot" data-live-slot={pane} data-live-readonly={readOnly ? "" : undefined} data-keys-own={readOnly ? undefined : ""} />;
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
        <NeedRow key={ask.id} need={{ ...askNeed(ask, needAge(ask.at_ms)), for: askedOf(client, reach.session?.daemons.find((d) => d.id === machine)?.owner_name), lines: [VIEWER_NOTE] }} open data-ask={ask.id} />
      ) : (
        <NeedRow key={ask.id} need={askNeed(ask, needAge(ask.at_ms))} open ask={ask} actions={{ answer: (content) => void act("answer", { content }), decline: () => void act("deny") }} />
      )}
      {err && <p class="board-bad">{err}</p>}
    </section>
  );
}

/** The drawer's last line (Arugula #1019): why the pane's session is on
 * this card, where its machine says (put there by hand, else its card
 * found from a branch: the pane's), and Change card, the pane chip's
 * picker on that session. Nothing while its machine isn't reached, or for
 * a session in no project. */
export function CardWhy({ machine, pane, reach }: { machine: string; pane: string; reach: Reach }) {
  const api = usePlatformApi();
  const c = useLive(machine, reach);
  const client = c === "unreachable" ? null : c;
  const id = Number(pane);
  const info = client?.connected ? client.info(id) : undefined;
  if (!client || !info?.session_project) return null;
  const sid = client.sessionOfPane(id);
  const session = client.state?.sessions.find((s) => s.id === sid);
  // A block on an issue of its own keeps it whatever its session's card: changing that wouldn't move this one.
  if (info.on_issue && info.on_issue.issue !== session?.work?.item) return null;
  const branch = !session?.work_by_hand && session?.work?.item ? info.branch : null;
  return (
    <p class="dim" data-board-drawer-why>
      {session?.work_by_hand ? (
        "On this card because you put it here. "
      ) : branch ? (
        <>
          On this card because its branch is <code>{branch}</code>.{" "}
        </>
      ) : null}
      <button type="button" class="link" data-board-drawer-change-card onClick={() => void pickCard(client, id, api.toast)}>
        Change card
      </button>
    </p>
  );
}
