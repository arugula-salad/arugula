// A session's peek in the Overview's talk panel (Arugula #1271): a card's
// row or a session on no card opens it as a tab. It says the session's
// name, how it stands, where it runs and its branch, and its last lines,
// read from its machine through the page's own connection (as a row's
// preview reads its last line) only while the tab is open: terminal
// content never goes through control. Under them, a box to write to it: an
// agent block takes it as its composer does (`call/send`, in at its next
// tool call), a terminal (Claude Code or a shell) has it typed, with Enter.
// Silence/Allow and Stop are the thread strip's (#653), on the same
// connection; Open in Panes ↗ is the whole terminal.
//
// A teammate's session is read-only: its state and last lines, and Ask
// its lead, the board's thread. A session in a cloud workspace is read
// through its box's daemon while the box runs, else it shows the last
// answer it posted in its thread (or its history, with chat off); its box
// writes in that thread and Stop is the platform's.

import { createContext, type JSX } from "preact";
import { useContext, useEffect, useLayoutEffect, useRef, useState } from "preact/hooks";
import type { SessionView } from "../shared/proto";
import { pointerLabel } from "../shared/boards/text";
import { chat } from "../shared/chat-store";
import { call, saidOf } from "../shared/chat/session-card";
import { useSubscribe } from "../shared/ui/hooks";
import { ExternalIcon } from "../shared/ui/icons";
import type { Fleet, FleetPane } from "../shell/fleet";
import { kindOf, shortHost } from "../shell/swarm/model";
import type { OvCard, OvSession } from "../shell/swarm/overview";
import { Word } from "./overview-row";

/** How often an open peek reads its lines again. */
const LINES_EVERY_MS = 3000;
/** How often an open peek on a cloud workspace's session reads it again. */
const HOSTED_EVERY_MS = 5000;
/** How many of its last lines it shows. */
const LINES = 20;

/** What the Overview gives its peeks. */
export interface PeekPlace {
  fleet: Fleet;
  /** The session a peek is on, and its card, while it's in the project. */
  find(key: string): { s: OvSession; card: OvCard | null } | undefined;
  /** It on its machine's page, or a cloud workspace's on its own. */
  openPanes(s: OvSession): void;
  openCard(c: OvCard): void;
  /** The board's thread in a tab, while chat is on. */
  askLead?(board: string): void;
}

export const PeekContext = createContext<PeekPlace | null>(null);

const enc = encodeURIComponent;

/** A session's key in its peek's tab: its key without its card, so the tab
 * stays on it when it moves to another card. */
export const peekKey = (s: OvSession) => s.key.slice(s.key.indexOf("|") + 1);

/** A tab's body: the session, found again on each draw. */
export function PeekTab({ id }: { id: string }) {
  const place = useContext(PeekContext);
  const at = place?.find(id);
  if (!place || !at) return <p class="dim ov-talk-note" data-ov-peek-gone>This session isn't in the project now.</p>;
  return <Peek place={place} s={at.s} card={at.card} />;
}

/** The who-line over a peek (#1290): the session, where it runs, and
 * where a message goes (named as its tab is: a lead by its session): straight to its agent (its thread, for a cloud
 * workspace's), typed into a terminal, or nowhere for one you watch. */
export function PeekWho({ id, title }: { id: string; title: string }) {
  const place = useContext(PeekContext);
  useSubscribe((fn) => (place ? place.fleet.subscribe(fn) : () => {}));
  const at = place?.find(id);
  if (!place || !at) return null;
  const { s } = at;
  const ov = s.panes.find((p) => p.agent) ?? s.lead;
  const pane = ov && place.fleet.panes.find((p) => p.key === ov.key);
  const who = (
    <>
      <b data-ov-who-name>{title}</b> on {shortHost(s.where)}
    </>
  );
  if (pane && yours(place.fleet, pane)) {
    if (pane.info.type === "agent" || kindOf(pane) === "agent") return <>Talking to {who}: a message goes straight to this agent.</>;
    return <>Typing into {who}: a message is typed into this terminal, with Enter.</>;
  }
  if (!pane && s.hosted) return <>Talking to {who}: a message goes straight to this agent, in its thread.</>;
  const mine = ownFar(place.fleet, s);
  if (mine) return <>Can't reach {who}: {mine.why}</>;
  return <>Watching {who}: it takes nothing from you here{at.card && place.askLead ? "; to ask it something, use its board's thread." : "."}</>;
}

/** A session this page can't read that runs on one of your own machines: why,
 * in words to act on. A machine the page doesn't list, or a teammate's, is
 * `undefined`: it's watched, or just far. */
export function ownFar(fleet: Fleet, s: OvSession): { why: string; note: string } | undefined {
  if (s.hosted) return undefined;
  const h = fleet.list.find((x) => x.name === s.where);
  if (!h || h.owner || h.team) return undefined;
  const name = shortHost(h.name);
  if (h.state !== "connected") return { why: `${name} isn't connected to this page. Connect it to read and talk to this session.`, note: `${name} is yours, but this page isn't connected to it now.` };
  return { why: `${name} is connected, but this page doesn't see this session on it.`, note: `${name} is yours and connected, but this page doesn't see this session on it.` };
}

/** Whose a pane is, as the peek treats it: yours, or a team's box or the
 * project's hosted one where you may steer it, to write to; a teammate's
 * own, to watch. */
export function yours(fleet: Fleet, p: FleetPane): boolean {
  if (p.person.kind === "me") return true;
  if (p.person.kind === "team" || fleet.host(p.host)?.hosted) return fleet.role(p) !== "viewer";
  return false;
}

function Peek({ place, s, card }: { place: PeekPlace; s: OvSession; card: OvCard | null }) {
  const { fleet } = place;
  useSubscribe((fn) => fleet.subscribe(fn));
  // The pane it's written to and read from: its agent where it has one, else the one it opens.
  const ov = s.panes.find((p) => p.agent) ?? s.lead;
  const pane = ov && fleet.panes.find((p) => p.key === ov.key);
  const hosted = useHosted(s.hosted ?? null);
  const box = hosted?.card.machine ? fleet.hostById(hosted.card.machine) : undefined;
  const block = hosted?.card.block && /^\d+$/.test(hosted.card.block) ? Number(hosted.card.block) : null;
  // Where its lines are read: its pane, else its cloud workspace's block while the box runs.
  const read = pane ? { host: pane.host, id: pane.id } : box?.state === "connected" && block !== null && !hosted?.card.parked ? { host: box.name, id: block } : null;
  const lines = useLines(fleet, read);
  const branch = pane?.info.branch ?? hosted?.card.branch ?? s.branch ?? null;
  const team = !!pane && !yours(fleet, pane);
  const far = !pane && !s.hosted;
  const own = far ? ownFar(fleet, s) : undefined;

  const head = (
    <header class="ov-peek-tab-head" data-ov-peek-head>
      <b data-ov-peek-name>{s.name}</b>
      <Word standing={s.standing} data-ov-peek-state />
      <span class="dim mono" data-ov-peek-where>
        {shortHost(s.where)}
        {branch ? ` · ${branch}` : ""}
      </span>
      {card && (
        <button type="button" class="ov-peek-card-link link" title="Open its card" data-ov-peek-card onClick={() => place.openCard(card)}>
          {pointerLabel(card.row.pointer, card.row.live) || "Its card"}
        </button>
      )}
    </header>
  );
  const lead = card && place.askLead && (
    <button type="button" class="secondary" data-ov-peek-ask-lead onClick={() => place.askLead!(card.board)}>
      Board thread
    </button>
  );
  const panes = (
    <button type="button" class="link ov-peek-panes" disabled={far} data-ov-peek-panes onClick={() => place.openPanes(s)}>
      Open in Panes <ExternalIcon size="xs" />
    </button>
  );

  // The tail keeps its newest line in sight.
  const foot = useRef<HTMLPreElement>(null);
  useLayoutEffect(() => {
    const el = foot.current;
    if (el) el.scrollTop = el.scrollHeight;
  });

  // Its lines: read from its machine; a cloud workspace's, from its box, else its last answer.
  let tail: string[] | null = lines;
  let tailSays = "Reading its last lines…";
  if (!read) {
    if (far) {
      tail = null;
      tailSays = own ? own.why : `It runs on ${s.where}, which this page doesn't reach.`;
    } else if (s.hosted) {
      tail = hosted ? lastAnswer(hosted) : null;
      tailSays = hosted ? "Nothing said yet." : "Reading the session…";
    }
  }
  const tailEl = (
    <>
      {!read && s.hosted && hosted && tail?.length ? <p class="dim ov-peek-why">Its box isn't running: what it last said.</p> : null}
      <pre ref={foot} class="ov-peek-tail" data-ov-peek-tail data-ov-peek-live={read ? "" : undefined}>
        {tail?.length ? tail.join("\n") : <span class="dim">{tailSays}</span>}
      </pre>
    </>
  );

  if (team || far)
    return (
      <div class="ov-peek-tab" data-ov-peek={s.key} data-ov-peek-readonly="">
        {head}
        <p class="dim ov-peek-why" data-ov-peek-note>
          {team ? `${pane!.person.name}'s session. You see its state and last lines; ${card ? "to ask it something, write in its board's thread, which its lead reads." : "it takes nothing from you here."}` : (own?.note ?? "You see where it runs and how it stands.")}
        </p>
        {tailEl}
        <div class="ov-peek-acts">
          {lead}
          {panes}
        </div>
      </div>
    );
  if (pane) return <MachinePeek s={s} fleet={fleet} pane={pane} head={head} tail={tailEl} panes={panes} />;
  return <HostedPeek s={s} v={hosted} head={head} tail={tailEl} panes={panes} />;
}

/** A session on a machine the page reaches: write to it, Silence/Allow it, Stop it. */
function MachinePeek({ s, fleet, pane, head, tail, panes }: { s: OvSession; fleet: Fleet; pane: FleetPane; head: JSX.Element; tail: JSX.Element; panes: JSX.Element }) {
  const block = pane.info.type === "agent";
  const agent = kindOf(pane) === "agent";
  const up = fleet.host(pane.host)?.state === "connected" && !pane.stale;
  const [said, setSaid] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const post = async (path: string, body: unknown, done: string) => {
    setBusy(true);
    try {
      const res = await fleet.request(pane.host, "POST", path, body);
      const err = res.ok ? null : ((await res.json<{ error?: string }>().catch(() => null))?.error ?? `HTTP ${res.status}`);
      setSaid(err ? `Couldn't: ${err}` : done);
      return !err;
    } catch {
      setSaid(`Couldn't: ${pane.host} isn't reachable now.`);
      return false;
    } finally {
      setBusy(false);
    }
  };
  // An agent block takes it as its composer does; a terminal has it typed, with Enter.
  const send = (text: string) => (block ? post(`/api/blocks/${pane.id}/call/send`, { text }, "Sent: it goes in at its next tool call.") : post(`/api/panes/${pane.id}/send`, { text, enter: true }, "Typed, with Enter."));
  // Stop as the strip's does: an agent block's turn is cancelled, Claude Code gets Escape; a shell, Ctrl-C.
  const stop = () => (block ? post(`/api/blocks/${pane.id}/call/cancel`, {}, "Stopped.") : post(`/api/panes/${pane.id}/keys`, { keys: [agent ? "Escape" : "C-c"] }, "Stopped."));
  const off = !!pane.info.mentions_off;
  return (
    <div class="ov-peek-tab" data-ov-peek={s.key} data-ov-peek-kind={block ? "block" : agent ? "agent" : "shell"}>
      {head}
      {tail}
      <div class="ov-peek-acts">
        {agent && (
          <button type="button" class="secondary" disabled={busy || !up} data-ov-peek-silence={off ? "allow" : "silence"} onClick={() => void post(`/api/panes/${pane.id}/mentions`, { on: off }, off ? "Mentions allowed." : "Silenced: mentions wait.")}>
            {off ? "Allow" : "Silence"}
          </button>
        )}
        <button type="button" class="secondary" disabled={busy || !up} data-ov-peek-stop onClick={() => void stop()}>
          Stop
        </button>
        {panes}
      </div>
      {said && (
        <p class="dim ov-peek-said" role="status" data-ov-peek-said>
          {said}
        </p>
      )}
      <Reply placeholder={!up ? `${pane.host} isn't reachable now` : block || agent ? "Reply: it reads it at its next tool call" : `Type a command for the shell on ${shortHost(pane.host)}`} disabled={!up} send={send} />
    </div>
  );
}

/** A session in a cloud workspace: write in its thread, Stop it on the platform. */
function HostedPeek({ s, v, head, tail, panes }: { s: OvSession; v: SessionView | null; head: JSX.Element; tail: JSX.Element; panes: JSX.Element }) {
  useSubscribe((fn) => chat.subscribe(fn));
  const [said, setSaid] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const ended = !!v && ["done", "failed", "stopped"].includes(v.card.state);
  const may = !!v?.may;
  const stop = async () => {
    setBusy(true);
    try {
      await call<SessionView>("POST", `/api/sessions/${enc(s.hosted!)}/stop`);
      setSaid("Stopped.");
    } catch (e) {
      setSaid(`Couldn't: ${(e as Error).message}`);
    } finally {
      setBusy(false);
    }
  };
  // Its box reads its thread: a reply there is its next prompt.
  const send = async (text: string) => {
    try {
      await chat.reply(v!.card.room, v!.card.parent, text, false);
      setSaid("Sent in its thread: it goes in at its next turn.");
      return true;
    } catch (e) {
      setSaid(`Couldn't: ${(e as Error).message}`);
      return false;
    }
  };
  return (
    <div class="ov-peek-tab" data-ov-peek={s.key} data-ov-peek-kind="hosted">
      {head}
      {tail}
      <div class="ov-peek-acts">
        {may && !ended && (
          <button type="button" class="secondary" disabled={busy} data-ov-peek-stop onClick={() => void stop()}>
            Stop
          </button>
        )}
        {panes}
      </div>
      {said && (
        <p class="dim ov-peek-said" role="status" data-ov-peek-said>
          {said}
        </p>
      )}
      {v && (may && chat.on ? <Reply placeholder="Reply in its thread" send={send} /> : <p class="dim ov-peek-why">{may ? "Its thread comes with chat, which is off for you." : "You're watching this session: an editor of the project answers it."}</p>)}
    </div>
  );
}

/** The box to write to it. */
export function Reply({ placeholder, disabled, send }: { placeholder: string; disabled?: boolean; send: (text: string) => Promise<boolean> }) {
  const [text, setText] = useState("");
  const [busy, setBusy] = useState(false);
  const go = async () => {
    const t = text.trim();
    if (!t || busy) return;
    setBusy(true);
    if (await send(t)) setText("");
    setBusy(false);
  };
  return (
    <form
      class="ov-peek-reply"
      data-ov-peek-reply
      onSubmit={(e) => {
        e.preventDefault();
        void go();
      }}
    >
      <input value={text} placeholder={placeholder} aria-label={placeholder} disabled={disabled || busy} onInput={(e) => setText((e.target as HTMLInputElement).value)} />
      <button type="submit" disabled={disabled || busy || !text.trim()}>
        Send
      </button>
    </form>
  );
}

/** A pane's last lines, read from its machine while the peek is open and the page is shown. */
function useLines(fleet: Fleet, at: { host: string; id: number } | null): string[] | null {
  const [lines, setLines] = useState<string[] | null>(null);
  const key = at ? `${at.host}:${at.id}` : "";
  useEffect(() => {
    setLines(null);
    if (!at) return;
    let live = true;
    const read = async () => {
      if (document.visibilityState !== "visible") return;
      try {
        const res = await fleet.request(at.host, "GET", `/api/panes/${at.id}/capture?format=text`);
        const text = res.ok && res.text ? await res.text() : "";
        const all = text.split("\n").map((l) => l.trimEnd());
        while (all.length && !all[all.length - 1]) all.pop();
        if (live) setLines(all.slice(-LINES));
      } catch {
        // Its machine isn't reachable now: the lines it had stay.
      }
    };
    void read();
    const t = setInterval(() => void read(), LINES_EVERY_MS);
    return () => {
      live = false;
      clearInterval(t);
    };
  }, [key]);
  return lines;
}

/** A cloud workspace's session as the platform has it, read again while the peek is open. */
export function useHosted(ses: string | null): SessionView | null {
  const [v, setV] = useState<SessionView | null>(null);
  useEffect(() => {
    setV(null);
    if (!ses) return;
    let live = true;
    const read = () =>
      call<SessionView>("GET", `/api/sessions/${enc(ses)}`).then(
        (x) => live && setV(x),
        () => {},
      );
    void read();
    const t = setInterval(() => document.visibilityState === "visible" && void read(), HOSTED_EVERY_MS);
    return () => {
      live = false;
      clearInterval(t);
    };
  }, [ses]);
  // Its thread, for its last answer, while chat is on.
  useEffect(() => {
    if (v && chat.on) void chat.threads.load(v.card.room, v.card.parent).catch(() => {});
  }, [v?.card.room, v?.card.parent, chat.on]);
  return v;
}

/** What a paused session last said: its last answer in its thread, else
 * (chat off, or none yet) the last of its history. */
export function lastAnswer(v: SessionView): string[] {
  const t = chat.on ? chat.threads.get(v.card.room, v.card.parent) : undefined;
  const answer = t?.replies.findLast((m) => saidOf(m) === v.card.session);
  if (answer) return answer.text.split("\n").slice(-LINES);
  return v.events.slice(-LINES).map((e) => e.text);
}
