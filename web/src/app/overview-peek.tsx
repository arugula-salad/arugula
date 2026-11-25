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
//
// An issue agent's tab (Arugula #1441), opened from its chip on a Board
// level ticket or as a Work level's first tab, is its issue's thread (a
// post there prompts the agent that leads it, #1439) above its peek; its
// peek then has no box of its own. Every agent's who-line with a rung (a
// board's lead, an issue agent) is its head (./agent-head.tsx): where it
// runs, whether it's awake, its rung, who started it and its parent's
// latest word to it.

import { createContext, type JSX } from "preact";
import { useContext, useEffect, useLayoutEffect, useRef, useState } from "preact/hooks";
import type { BoardLead, ChatRoom, SessionView } from "../shared/proto";
import { pointerLabel } from "../shared/boards/text";
import { chat } from "../shared/chat-store";
import { RoomBody } from "../shared/chat/room";
import { call, saidOf } from "../shared/chat/session-card";
import { useSubscribe } from "../shared/ui/hooks";
import { ExternalIcon } from "../shared/ui/icons";
import type { Fleet, FleetPane } from "../shell/fleet";
import { kindOf, shortHost } from "../shell/swarm/model";
import { leadAwake, type OvCard, type OvLead, type OvSession } from "../shell/swarm/overview";
import { AgentHead, objectRoom, starterOf, useParentWord } from "./agent-head";
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
  /** A session that leads something (a staff room, #1342) as the Overview reads a board's lead. */
  lead(l: Pick<BoardLead, "machine" | "machine_name" | "session">): OvLead;
  /** The session a peek is on, and its card, while it's in the project. */
  find(key: string): { s: OvSession; card: OvCard | null } | undefined;
  /** It on its machine's page, or a cloud workspace's on its own. */
  openPanes(s: OvSession): void;
  openCard(c: OvCard): void;
  /** The board's thread in a tab, while chat is on. */
  askLead?(board: string): void;
  /** The project's name, for the chief of staff's rung (#1441). */
  project?: string;
  /** A session's rung on the agent ladder (#1441), where it has one: a board's lead, or an issue agent. */
  rung?(s: OvSession, card: OvCard | null): PeekRung | null;
}

/** Where a session is on the agent ladder (#1441), for its tab. */
export interface PeekRung {
  /** "lead of Launch", "on #12 for Launch". */
  line: string;
  /** The session above it (its machine's id and its name there), and what to call it. */
  parent: { machine: string; session: string; who: string } | null;
  /** What the threads its parent writes to it in are on: its issue, or its boards. */
  rooms: string[];
  /** An issue agent's issue (`iss_…`) and how it's named (`#12`): its thread is the tab's top. */
  issue?: { id: string; label: string };
}

/** Whether a lead (a room's, a board's) is this session: the same tab, or its pane is one of the session's. */
export const isSession = (l: OvLead, s: OvSession) => talkKey(l.peek) === talkKey(s) || (!!l.peek.lead && s.panes.some((p) => p.key === l.peek.lead!.key));

export const PeekContext = createContext<PeekPlace | null>(null);

const enc = encodeURIComponent;

/** A session's key in its peek's tab: its key without its card, so the tab
 * stays on it when it moves to another card. */
export const peekKey = (s: OvSession) => s.key.slice(s.key.indexOf("|") + 1);

/** A session's tab in the talk panel: the hub's id for it where the hub
 * lists it, which stays put when its panes come onto the page or its
 * daemon restarts (#1390); else its key without its card. */
export const talkKey = (s: OvSession) => s.hub ?? peekKey(s);

/** A peek tab's id as saved before #1390, as it is now: a session the hub
 * listed with no pane on the page was `machine:<machine>:ses_…` or
 * `hosted:ses_…`, and is its `ses_…`. */
export const talkKeyOf = (id: string) => /^(?:machine:[^:|]+|hosted):(ses_[^:|]+)$/.exec(id)?.[1] ?? id;

/** A tab's body: the session, found again on each draw; an issue agent's, its issue's thread above it (#1441). */
export function PeekTab({ id, phone }: { id: string; phone: boolean }) {
  const place = useContext(PeekContext);
  useSubscribe((fn) => chat.subscribe(fn));
  const at = place?.find(id);
  if (!place || !at) return <p class="dim ov-talk-note" data-ov-peek-gone>This session isn't in the project now.</p>;
  const issue = chat.on ? place.rung?.(at.s, at.card)?.issue : undefined;
  if (issue) return <AgentTab key={issue.id} place={place} s={at.s} card={at.card} issue={issue} phone={phone} />;
  return <Peek place={place} s={at.s} card={at.card} />;
}

/** An issue agent's tab (#1441): its issue's thread, where a post prompts
 * the agent leading it, over its peek. When the thread's lead is another
 * session, or none, it says so, and the peek keeps its own box. */
function AgentTab({ place, s, card, issue, phone }: { place: PeekPlace; s: OvSession; card: OvCard | null; issue: { id: string; label: string }; phone: boolean }) {
  useSubscribe((fn) => chat.subscribe(fn));
  const [opened, setOpened] = useState<ChatRoom | null>(objectRoom(issue.id) ?? null);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    let live = true;
    chat.openObject(issue.id).then(
      (r) => live && setOpened(r),
      (e: Error) => live && setError(e.message),
    );
    return () => void (live = false);
  }, [issue.id]);
  // Chat's own copy, which a change of its lead reaches.
  const room = (opened && chat.rooms.find((r) => r.id === opened.id)) ?? opened;
  const lead = room?.lead ? place.lead(room.lead) : null;
  const hears = !!lead && isSession(lead, s);
  const why = hears
    ? `A post here prompts ${s.name}, which leads ${issue.label}'s thread. Everyone in the project reads it.`
    : room?.lead
      ? `${room.lead.session} leads ${issue.label}'s thread, so a post here goes to it, not to ${s.name}: write to ${s.name} under its lines.`
      : `No session leads ${issue.label}'s thread, so a post here waits for someone to read it: write to ${s.name} under its lines.`;
  return (
    <div class="ov-agent-tab" data-ov-agent-tab={issue.id} data-ov-agent-hears={hears ? "" : undefined}>
      {!room ? (
        <p class="dim ov-talk-note">{error ? `The thread didn't load: ${error}` : "Loading its thread…"}</p>
      ) : (
        <div class="chat-thread ov-talk-room" data-ov-issue-thread={room.id}>
          {!hears && (
            <p class="dim ov-agent-note" data-ov-agent-note>
              {why}
            </p>
          )}
          <RoomBody key={room.id} room={room} phone={phone} embedded name={issue.label} empty={{ title: `Nothing said on ${issue.label} yet.`, why }} />
        </div>
      )}
      <Peek place={place} s={s} card={card} reply={!hears} />
    </div>
  );
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
  const rung = place.rung?.(s, at.card);
  if (rung) return <RungWho place={place} s={s} rung={rung} title={title} />;
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

/** An agent with a rung (#1441): its head, its rung and who started it, and its parent's latest word to it. */
function RungWho({ place, s, rung, title }: { place: PeekPlace; s: OvSession; rung: PeekRung; title: string }) {
  const word = useParentWord(rung.parent, rung.rooms);
  const at = leadAwake({ machineName: s.where, standing: s.standing }, place.fleet.host(s.where));
  return <AgentHead role={title} at={at} rung={{ line: rung.line, started: starterOf(place.fleet, s), word: word && rung.parent && { who: rung.parent.who, m: word } }} />;
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

/** A session this page can't read on a teammate's machine, which the fleet lists but shares no panes of: whose it is, in words. */
function theirsFar(fleet: Fleet, s: OvSession): string | undefined {
  if (s.hosted) return undefined;
  const h = fleet.list.find((x) => x.name === s.where);
  if (!h?.owner) return undefined;
  return `It runs on ${s.where}, ${h.owner}'s machine, whose panes aren't shared with you.`;
}

/** Whose a pane is, as the peek treats it: yours, or a team's box or the
 * project's hosted one where you may steer it, to write to; a teammate's
 * own, to watch. */
export function yours(fleet: Fleet, p: FleetPane): boolean {
  if (p.person.kind === "me") return true;
  if (p.person.kind === "team" || fleet.host(p.host)?.hosted) return fleet.role(p) !== "viewer";
  return false;
}

/** A session's peek; `reply` false leaves out its box (an issue agent's tab, whose thread prompts it). */
function Peek({ place, s, card, reply = true }: { place: PeekPlace; s: OvSession; card: OvCard | null; reply?: boolean }) {
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
      tailSays = own ? own.why : (theirsFar(fleet, s) ?? `It runs on ${s.where}, which this page doesn't reach.`);
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
  if (pane) return <MachinePeek s={s} fleet={fleet} pane={pane} head={head} tail={tailEl} panes={panes} reply={reply} />;
  return <HostedPeek s={s} v={hosted} head={head} tail={tailEl} panes={panes} reply={reply} />;
}

/** A session on a machine the page reaches: write to it, Silence/Allow it, Stop it. */
function MachinePeek({ s, fleet, pane, head, tail, panes, reply }: { s: OvSession; fleet: Fleet; pane: FleetPane; head: JSX.Element; tail: JSX.Element; panes: JSX.Element; reply: boolean }) {
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
      {reply && <Reply placeholder={!up ? `${pane.host} isn't reachable now` : block || agent ? "Reply: it reads it at its next tool call" : `Type a command for the shell on ${shortHost(pane.host)}`} disabled={!up} send={send} />}
    </div>
  );
}

/** A session in a cloud workspace: write in its thread, Stop it on the platform. */
function HostedPeek({ s, v, head, tail, panes, reply }: { s: OvSession; v: SessionView | null; head: JSX.Element; tail: JSX.Element; panes: JSX.Element; reply: boolean }) {
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
      {v && (may && chat.on ? reply && <Reply placeholder="Reply in its thread" send={send} /> : <p class="dim ov-peek-why">{may ? "Its thread comes with chat, which is off for you." : "You're watching this session: an editor of the project answers it."}</p>)}
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
