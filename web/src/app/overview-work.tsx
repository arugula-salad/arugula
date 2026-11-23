// The Overview's Work level (Arugula #1287, #1289): a card's work zoomed
// to in place, at `/p/:project/board/:board/card/:row`, or a session on no
// card, at `/p/:project/work/:session`. Every pane of the sessions on it,
// agent blocks and terminals, live side by side: the views Panes mounts
// (`PlatformApi.LiveWork`: ./live-pane's `WorkSlot`), read and typed into
// through the page's own client to each machine (./machine-client), so terminal content never
// goes through control. At most four side by side; the rest wait as tabs,
// and the one you're talking to is brought in. Each pane's head names it,
// its machine, branch and state, with Talk (its session's tab in the talk
// panel) and Open in Panes ↗; the pane whose agent you're talking to is
// outlined. What a pane asks sits above them all, answered as Needs you
// answers it (`PlatformApi.LiveAsk`, `/api/attention/act`).
//
// A cloud workspace's session is its box's block while the box runs, else
// its last answer and a reply in its thread (as its peek has them). A
// teammate's session is watched: your keys stop at its pane. A card no
// agent works on says so, and offers its board's lead.

import type { ComponentType } from "preact";
import { useEffect, useState } from "preact/hooks";
import type { BoardRow, SessionView } from "../shared/proto";
import { pointerLabel } from "../shared/boards/text";
import { chat } from "../shared/chat-store";
import { call } from "../shared/chat/session-card";
import { usePlatformApi } from "../shared/platform-api";
import { EmptyLine } from "../shared/ui/block-states";
import { useSubscribe } from "../shared/ui/hooks";
import { ExternalIcon } from "../shared/ui/icons";
import type { Fleet } from "../shell/fleet";
import { directory } from "../shell/hosts";
import { shortHost } from "../shell/swarm/model";
import type { OvPane, OvSession, Standing } from "../shell/swarm/overview";
import { titleOf } from "./overview-board";
import { lastAnswer, ownFar, peekKey, Reply, talkKey, yours } from "./overview-peek";
import { Word } from "./overview-row";

/** Most panes side by side; the rest are tabs. */
const SIDE_BY_SIDE = 4;
/** How often a cloud workspace's session is read again while it's shown. */
const HOSTED_EVERY_MS = 5000;

/** What the Overview gives its Work level. */
export interface WorkLevelProps {
  project: string;
  fleet: Fleet;
  /** The card zoomed to, with the board it's on and how it stands; null
   * for a session on no card. */
  card: { board: string; boardTitle: string; row: BoardRow; standing: Standing } | null;
  /** The sessions working on it: the card's, or the one on no card. */
  sessions: OvSession[];
  /** The session whose tab is in front of the talk panel, by its tab's
   * key (`talkKey`): its pane is the one outlined. */
  talking: string | null;
  /** Bring a session's tab to the front of the talk panel. */
  talkTo(s: OvSession): void;
  /** A session on its machine's page, on its pane (or a cloud
   * workspace's on its own). */
  openPanes(s: OvSession): void;
  /** The card's board's lead, in the talk panel: offered when no agent works on the card. */
  askLead?: () => void;
}

/** One pane at the Work level: a pane of a session, or a session with none
 * here (a cloud workspace's, or one on a machine the page doesn't reach). */
interface Item {
  key: string;
  s: OvSession;
  p?: OvPane;
}

/** The machine a pane is reached on: control's id for it, else, on a
 * daemon's own page, whose hosts carry no ids, its name (./machine-client). */
const machineOf = (p: OvPane): string | undefined => p.machine ?? (directory.home !== null ? p.host : undefined);

/** Each session's panes, its agents first. */
function itemsOf(sessions: OvSession[]): Item[] {
  return sessions.flatMap((s) => (s.panes.length ? [...s.panes].sort((a, b) => Number(b.agent) - Number(a.agent)).map((p) => ({ key: p.key, s, p })) : [{ key: s.key, s }]));
}

/** The pane a session's Talk is with: its agent, else the one it opens. */
const talkPaneOf = (s: OvSession) => (s.panes.find((p) => p.agent) ?? s.lead ?? s.panes[0])?.key ?? s.key;

/** The Work level: what's working on a card (or the session on no card). */
export function OverviewWork({ card, fleet, sessions, talking, talkTo, openPanes, askLead }: WorkLevelProps) {
  useSubscribe((fn) => fleet.subscribe(fn));
  const { LiveWork, LiveAsk } = usePlatformApi();
  const lone = card ? null : sessions[0];
  const items = itemsOf(sessions);
  const hosted = useHostedViews(sessions.flatMap((s) => (s.hosted && !s.panes.length ? [s.hosted] : [])));
  const talkingKey = sessions.find((s) => talkKey(s) === talking);
  const outlined = talkingKey ? talkPaneOf(talkingKey) : null;

  // Four side by side; past that, the fourth place is a tab's, the one you're talking to's when it's among them.
  const [picked, setPicked] = useState<string | null>(null);
  useEffect(() => setPicked(null), [outlined]);
  const many = items.length > SIDE_BY_SIDE;
  const fixed = many ? items.slice(0, SIDE_BY_SIDE - 1) : items;
  const rest = many ? items.slice(SIDE_BY_SIDE - 1) : [];
  const tab = rest.find((i) => i.key === picked) ?? rest.find((i) => i.key === outlined) ?? rest[0];
  const shown = tab ? [...fixed, tab] : fixed;

  // Where each live pane is read from: its machine's, or a running box's block.
  const liveAt = (i: Item): { machine: string; pane: string } | null => {
    if (i.p) {
      const machine = machineOf(i.p);
      return machine ? { machine, pane: String(i.p.id) } : null;
    }
    const v = i.s.hosted ? hosted.get(i.s.hosted) : undefined;
    return v ? boxOf(fleet, v) : null;
  };
  const noAgent = !!card && !sessions.some((s) => s.hosted || s.panes.some((p) => p.agent));

  return (
    <section class="ov-level ov-work" data-ov-level-work={card ? card.row.id : lone ? peekKey(lone) : ""}>
      <header class="ov-level-head">
        <h2>{card ? [pointerLabel(card.row.pointer, card.row.live), titleOf(card.row)].filter(Boolean).join(" ") : lone ? `${lone.name} on ${shortHost(lone.where)}` : "A session"}</h2>
        <p class="dim ov-level-sub">
          {card ? (
            <>
              {card.boardTitle} <Word standing={card.standing} />
            </>
          ) : (
            "Not on a card"
          )}
        </p>
      </header>
      {noAgent && (
        <p class="dim ov-work-noagent" data-ov-work-noagent>
          No agent is working on this card.{" "}
          {askLead && (
            <button type="button" class="secondary" data-ov-work-ask-lead onClick={askLead}>
              Ask the lead to start one
            </button>
          )}
        </p>
      )}
      {sessions.length === 0 ? (
        <EmptyLine data-ov-level-empty>{card ? "No session is working on this card yet." : "This session isn't in the project now."}</EmptyLine>
      ) : (
        <>
          {LiveAsk && (
            <div class="ov-work-asks" data-ov-work-asks>
              {items.map((i) => {
                const at = liveAt(i);
                return at && <LiveAsk key={i.key} machine={at.machine} pane={at.pane} />;
              })}
            </div>
          )}
          {many && (
            <div class="ov-work-tabs" role="tablist" aria-label="More panes" data-ov-work-tabs>
              <span class="dim">More here:</span>
              {rest.map((i) => (
                <button key={i.key} type="button" role="tab" class={i === tab ? "ov-work-tab ov-work-tab-on" : "ov-work-tab"} aria-selected={i === tab} data-ov-work-tab={i.key} onClick={() => setPicked(i.key)}>
                  {i.p?.name ?? i.s.name}
                  <Word standing={i.p?.standing ?? i.s.standing} />
                </button>
              ))}
            </div>
          )}
          <ul class={`ov-work-grid ov-work-n${shown.length}`} data-ov-level-sessions>
            {shown.map((i) => (
              <WorkPane key={i.key} i={i} fleet={fleet} Live={LiveWork} at={liveAt(i)} hosted={i.s.hosted ? hosted.get(i.s.hosted) : undefined} outlined={outlined === i.key} talkTo={talkTo} openPanes={openPanes} />
            ))}
          </ul>
        </>
      )}
    </section>
  );
}

/** A box's block for a cloud workspace's session while its box runs. */
function boxOf(fleet: Fleet, v: SessionView): { machine: string; pane: string } | null {
  const { machine, block, parked } = v.card;
  if (!machine || !block || !/^\d+$/.test(block) || parked) return null;
  return fleet.hostById(machine)?.state === "connected" ? { machine, pane: block } : null;
}

/** One pane: its head, then it live (or what stands for it). */
function WorkPane({ i, fleet, Live, at, hosted, outlined, talkTo, openPanes }: { i: Item; fleet: Fleet; Live?: ComponentType<{ machine: string; pane: string; watch?: boolean }>; at: { machine: string; pane: string } | null; hosted?: SessionView; outlined: boolean; talkTo(s: OvSession): void; openPanes(s: OvSession): void }) {
  const { s, p } = i;
  const fp = p && fleet.panes.find((x) => x.key === p.key);
  const branch = fp?.info.branch ?? hosted?.card.branch ?? s.branch ?? null;
  // A teammate's own session is watched; a cloud workspace's, by whoever may not steer it.
  const watch = fp ? !yours(fleet, fp) : hosted ? !hosted.may : false;
  const far = !(p && machineOf(p)) && !s.hosted;
  const kind = fp ? (fp.info.type === "agent" ? "block" : p!.agent ? "agent" : "shell") : s.hosted ? "hosted" : "far";
  let body;
  if (at && Live) body = <Live machine={at.machine} pane={at.pane} watch={watch} />;
  else if (s.hosted) body = <HostedPaused s={s} v={hosted} />;
  else body = <p class="dim ov-work-note">{ownFar(fleet, s)?.why ?? `It runs on ${s.where}, which this page doesn't reach.`}</p>;
  return (
    <li class={`ov-work-pane${outlined ? " ov-work-pane-talking" : ""}`} data-ov-work-session={peekKey(s)} data-ov-work-pane={i.key} data-ov-work-kind={kind} data-ov-talking={outlined ? "" : undefined}>
      <header class="ov-work-head">
        <b class="ov-work-name" title={p?.name ?? s.name}>
          {p?.name ?? s.name}
        </b>
        <Word standing={p?.standing ?? s.standing} />
        <span class="dim mono ov-work-where">
          {shortHost(p?.host ?? s.where)}
          {branch ? ` · ${branch}` : ""}
        </span>
        <span class="ov-work-acts">
          <button type="button" class="secondary small" data-ov-work-talk onClick={() => talkTo(s)}>
            Talk
          </button>
          <button type="button" class="link" disabled={far} data-ov-work-panes onClick={() => openPanes(s)}>
            Open in Panes <ExternalIcon size="xs" />
          </button>
        </span>
      </header>
      {watch && fp && (
        <p class="dim ov-work-note" data-ov-work-watching>
          {fp.person.name}'s session: you watch it here.
        </p>
      )}
      {body}
    </li>
  );
}

/** A cloud workspace's session while its box isn't running: what it last said, and a reply in its thread. */
function HostedPaused({ s, v }: { s: OvSession; v?: SessionView }) {
  useSubscribe((fn) => chat.subscribe(fn));
  const [said, setSaid] = useState<string | null>(null);
  useEffect(() => {
    if (v && chat.on) void chat.threads.load(v.card.room, v.card.parent).catch(() => {});
  }, [v?.card.room, v?.card.parent, chat.on]);
  if (!v) return <p class="dim ov-work-note">Reading the session…</p>;
  const last = lastAnswer(v);
  const send = async (text: string) => {
    try {
      await chat.reply(v.card.room, v.card.parent, text, false);
      setSaid("Sent in its thread: it goes in at its next turn.");
      return true;
    } catch (e) {
      setSaid(`Couldn't: ${(e as Error).message}`);
      return false;
    }
  };
  return (
    <div class="ov-work-paused" data-ov-work-paused={s.hosted}>
      <p class="dim ov-work-note">Its box isn't running: what it last said.</p>
      <pre class="ov-peek-tail">{last.length ? last.join("\n") : <span class="dim">Nothing said yet.</span>}</pre>
      {said && (
        <p class="dim ov-peek-said" role="status">
          {said}
        </p>
      )}
      {v.may && chat.on ? <Reply placeholder="Reply in its thread" send={send} /> : <p class="dim ov-work-note">{v.may ? "Its thread comes with chat, which is off for you." : "You're watching this session: an editor of the project answers it."}</p>}
    </div>
  );
}

/** Cloud workspaces' sessions as the platform has them, read again while shown. */
function useHostedViews(ids: string[]): Map<string, SessionView> {
  const [m, setM] = useState<Map<string, SessionView>>(new Map());
  const key = ids.join(",");
  useEffect(() => {
    if (!ids.length) return;
    let live = true;
    const read = () =>
      ids.forEach((id) =>
        call<SessionView>("GET", `/api/sessions/${encodeURIComponent(id)}`).then(
          (v) => live && setM((was) => new Map(was).set(id, v)),
          () => {},
        ),
      );
    read();
    const t = setInterval(() => document.visibilityState === "visible" && read(), HOSTED_EVERY_MS);
    return () => {
      live = false;
      clearInterval(t);
    };
  }, [key]);
  return m;
}
