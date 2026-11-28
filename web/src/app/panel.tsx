// The agent panel's content (Lift 4, #1480), in the kit's Panel frame
// (../shared/kit/panel.tsx): what each tab shows, over the app's machines.
// A place reaches it through `usePanel()`; the Overview draws it in its
// right third, below Needs you.
//
// - The chief of staff is always the first tab: the reader's staff room
//   with its own agents (#1275), its messages and its box. Its head says
//   where it runs and whether a message is heard now (#1342); while it's
//   asleep or no session leads it, the room offers to run it elsewhere
//   (#1453, #1472), and while it runs, Restart (#1500); with no room yet,
//   Start (#1341, ./panel-staff.tsx).
// - An agent's tab is its session: its transcript (an agent block's
//   Markdown, its tool calls folded to a line; a terminal's last lines in a
//   well), read from its machine through the page's own connection while
//   the tab is open, never through control (#1271); Claude Code's screen in
//   a terminal folded the same way (shared/transcript.ts); `v` shows the pane
//   itself, live, in a well. Its composer writes to it as the peek did: an
//   agent block at its next tool call, a terminal typed with Enter, a
//   cloud workspace's session in its thread; Silence and Stop sit beside
//   it. A teammate's session, or one on a machine this page doesn't reach,
//   is read only, and says why. A session that ended is its transcript,
//   read only, ending in when it ended; on a to-do of the reader's, the
//   line says Done carries on with their note, over the note (checked for
//   secrets as it's typed) and Done or Can't: answered, the to-do (`POST
//   /api/inbox/{prp}`) wakes its conversation on its machine with the note,
//   and the foot is the row's receipt, read from its record (#1462).
// - A thread's tab is a chat room (a board's thread, an issue's), or one
//   message's reply thread in it.
//
// In any room here, "N replies" and "Reply in thread" open the thread in
// the panel, over its room, which keeps its place: the one Back (or Esc)
// returns to it, and its ⋯ has Follow. The page's address doesn't change and nothing opens in Chat.

import type { ComponentChildren } from "preact";
import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "preact/hooks";
import type { ChatRoom, InboxItem, SessionView } from "../shared/proto";
import { agentName, rungOf } from "../shared/agent-name";
import { boards } from "../shared/boards/store";
import { chat } from "../shared/chat-store";
import { RoomBody } from "../shared/chat/room";
import { call, saidOf } from "../shared/chat/session-card";
import { TodoAnswer } from "../shared/chat/todo-answer";
import { followThread, followingThread, ThreadPanel } from "../shared/chat/thread-panel";
import { Button } from "../shared/kit/button";
import { pastedName } from "../shared/chat/files";
import { upload } from "../shared/chat/files-upload";
import { Composer } from "../shared/kit/composer";
import { receiptLine, type Receipt } from "../shared/kit/need-row";
import { CHIEF_KEY, Panel, PanelNote, panelKey, Transcript, type AgentPanelProps, type PanelHead, type PanelTarget, type PanelView } from "../shared/kit/panel";
import type { State } from "../shared/kit/state-mark";
import type { TabItem } from "../shared/kit/tabs";
import { Well } from "../shared/kit/well";
import { needAge, needsYou, needsYouRank } from "../shared/needs-you";
import { PlatformApiContext, usePlatformApi, type ChatNav } from "../shared/platform-api";
import { surfacesFor } from "../shared/surfaces";
import { cleanTitle } from "../shared/titles";
import { isClaudeScreen, lastLines } from "../shared/transcript";
import { useSubscribe } from "../shared/ui/hooks";
import type { MenuItem } from "../shared/ui/menu";
import type { Fleet, FleetPane } from "../shell/fleet";
import { activityOf, kindOf, shortHost } from "../shell/fleet-model";
import { boardLead, leadAwake, type LeadAwake, type OvPane, type Standing } from "../shell/fleet-overview";
import { store } from "../shell/upload";
import { answerItem, readInbox } from "../platform/inbox";
import { boot } from "./machine-page";
import { paneOwner, receiptOf, sessionName, yours } from "./needs";
import { StaffMove, StaffRestart, StaffStart } from "./panel-staff";
import type { AppSession } from "./session";

/** The chief of staff's tab, which `onPick` is given for it. */
export const CHIEF = CHIEF_KEY;

/** How often an open agent's tab reads its lines again. */
const LINES_EVERY_MS = 3000;
/** How often a cloud workspace's session is read again. */
const HOSTED_EVERY_MS = 5000;
/** How many of its last lines a transcript shows. */
const LINES = 400;

const enc = encodeURIComponent;

/** What the panel reaches the page's machines through. */
interface Place {
  fleet: Fleet;
  project: string;
  hostedIn: (machine: string | undefined) => string | undefined;
}

/** What a tab draws in the frame. */
interface Shown {
  head: PanelHead;
  body: ComponentChildren;
  foot?: ComponentChildren;
  lede?: ComponentChildren;
  over?: ComponentChildren;
  back?: { label: string; go: () => void };
  flush?: boolean;
  view?: PanelView;
  onView?: (v: PanelView) => void;
}

/** It asks, it failed, it works, or it's quiet: what a tile says of a pane. */
export function standingOf(p: FleetPane): Standing {
  if (needsYou(p.info)) return needsYouRank(p.info) <= 1 ? "needs" : "failed";
  if (!p.stale && (activityOf(p) > 0.3 || p.info.attention === "working")) return "working";
  return "quiet";
}

/** A pane as the Overview groups it. */
export function ovPane(fleet: Fleet, p: FleetPane): OvPane {
  const owner = paneOwner(fleet, p);
  return {
    key: p.key,
    host: p.host,
    machine: fleet.host(p.host)?.id,
    id: p.id,
    session: p.session,
    project: p.info.session_project?.id ?? null,
    on_issue: p.info.on_issue,
    title: cleanTitle(p.info.title ?? ""),
    name: sessionName(p),
    agent: kindOf(p) === "agent",
    ...(owner !== null ? { owner } : {}),
    standing: standingOf(p),
    last: p.info.activity?.last_ms ?? 0,
  };
}

/** How a tile's standing reads as a kit mark. */
const MARK: Partial<Record<Standing, State>> = { needs: "asks", asks: "asks", failed: "failing", waits: "waits", working: "working" };

/** The panel, with the page's fleet booted (`usePanel()`'s, outside the Overview). */
export function AgentPanelPlace(props: AgentPanelProps & { session: AppSession | null }) {
  const [b, setB] = useState<Awaited<ReturnType<typeof boot>> | null>(null);
  useEffect(() => {
    let live = true;
    void boot(props.session).then((x) => live && setB(x));
    return () => void (live = false);
  }, []);
  if (!b) return <>{props.above}</>;
  return <AgentPanel {...props} fleet={b.machines.fleet} />;
}

/** The panel over `fleet`: the chief of staff, then `targets`. */
export function AgentPanel({ project, above, targets, current, onPick, onClose, folded, chiefIcon, toggle, fleet, session }: AgentPanelProps & { fleet: Fleet; session: AppSession | null }) {
  useSubscribe((fn) => fleet.subscribe(fn));
  useSubscribe((fn) => chat.subscribe(fn));
  const api = usePlatformApi();
  const place: Place = { fleet, project, hostedIn: (id) => (id ? session?.daemons.find((d) => d.id === id)?.hosted?.project : undefined) };
  const target = current ? targets.find((t) => panelKey(t) === current) : undefined;
  const key = target ? panelKey(target) : CHIEF;
  const hostName = (m: string) => (fleet.hostById(m) ?? fleet.host(m))?.name;

  // A room's "N replies" opens its thread here, over the room: by tab, the
  // thread open in it (null: the room), else the target's own.
  const [drill, setDrill] = useState<{ key: string; parent: number | null } | null>(null);
  const [view, setView] = useState<{ key: string; view: PanelView } | null>(null);
  const viewOf = view?.key === key ? view.view : "transcript";
  const pickView = (v: PanelView) => setView({ key, view: v });

  const chief = useChief(target === undefined, place, viewOf, pickView);
  const agent = useAgent(target?.kind === "agent" ? target : null, place, viewOf, pickView);
  const thread = useThread(target?.kind === "thread" ? target : null);
  const room = target === undefined ? chief.room : target.kind === "thread" ? thread.room : null;
  const parent = drill?.key === key ? drill.parent : target?.kind === "thread" ? (target.parent ?? null) : null;
  const shown: Shown = target === undefined ? chief.shown : target.kind === "agent" ? agent : thread.shown;

  // The room's nav, with its threads opening here.
  const nav = useMemo<ChatNav>(() => ({ ...api.nav, openRoom: (id, msg, t) => (room && id === room.id && t !== undefined ? setDrill({ key, parent: t }) : api.nav.openRoom(id, msg, t)) }), [api.nav, key, room?.id]);
  const here = useMemo(() => ({ ...api, nav }), [api, nav]);
  const over =
    room && parent !== null ? (
      <PlatformApiContext.Provider value={here}>
        <ThreadPanel key={`${room.id}/${parent}`} store={chat} room={room} parent={parent} phone={false} inPanel close={() => setDrill({ key, parent: null })} />
      </PlatformApiContext.Provider>
    ) : undefined;

  const tabs: TabItem[] = [
    { id: CHIEF, title: agentName({ role: "chief" }), ...(chiefIcon ? { icon: <span class="k-panel-chief" aria-hidden="true">✦</span>, bare: true } : {}) },
    ...targets.map((t) => {
      // The place's own tab (named, or with its switch) stays; a visitor is in italics, with its ×.
      const peek = !(t.kind === "agent" ? t.label : toggle?.key === panelKey(t));
      return {
        id: panelKey(t),
        title: t.kind === "agent" ? (t.label ?? agentName({ role: t.role, title: t.title, session: t.session, machine: hostName(t.machine) ?? null })) : t.title,
        closes: !!onClose && peek,
        peek,
        after:
          toggle?.key === panelKey(t) ? (
            <button type="button" class="k-button is-plain m-meta" title={toggle.title} data-panel-toggle onClick={toggle.run}>
              ⇄ {toggle.label}
            </button>
          ) : undefined,
      };
    }),
  ];
  const note = target?.kind === "agent" && target.note ? <p class="m-meta" data-panel-note>{target.note}</p> : null;
  return (
    <Panel
      label="Agent panel"
      tabs={tabs}
      current={key}
      onPick={(id) => onPick?.(id)}
      onClose={onClose}
      above={above}
      head={shown.head}
      view={shown.view}
      onView={shown.onView}
      floors={surfacesFor().floor}
      lede={note || shown.lede ? (
        <>
          {note}
          {shown.lede}
        </>
      ) : undefined}
      back={over ? { label: `Back to ${shown.head.name}`, go: () => setDrill({ key, parent: null }), more: room && parent !== null ? threadMore(room.id, parent) : undefined } : undefined}
      over={over}
      flush={shown.flush}
      foot={shown.foot}
      folded={folded}
    >
      <PlatformApiContext.Provider value={here}>{shown.body}</PlatformApiContext.Provider>
    </Panel>
  );
}

/** A thread's ⋯ in the panel: Follow, or stop, once it has loaded. */
function threadMore(room: string, parent: number): MenuItem[] | undefined {
  const on = followingThread(chat, room, parent);
  if (on === undefined) return undefined;
  return [{ label: on ? "Stop following" : "Follow", hint: on ? "You hear its replies" : "Hear its replies", run: () => void followThread(chat, room, parent).catch(() => {}) }];
}

/** The chief of staff's head from how its lead stands (#1342). */
function chiefHead(at: LeadAwake): PanelHead {
  const machine = at.state === "none" ? undefined : shortHost(at.machine);
  const base = { name: agentName({ role: "chief" }), rung: rungOf({ role: "chief" }), machine, status: at.state };
  if (at.state === "awake") return { ...base, state: "working" };
  if (at.state === "idle") return { ...base, state: "idle", hint: "wakes on its own" };
  if (at.state === "asleep") return { ...base, state: "stuck", word: "asleep", hint: "messages wait", machine: `${machine} is offline` };
  if (at.state === "none") return { ...base, state: "idle", word: "no one leads it" };
  return base;
}

/** A pane's terminal, live, in a well. */
function TerminalWell({ machine, pane, label, watch }: { machine: string; pane: number; label: string; watch?: boolean }) {
  const { LiveWork } = usePlatformApi();
  return (
    <Well label={label}>
      <div data-panel-terminal={pane}>{LiveWork ? <LiveWork machine={machine} pane={String(pane)} watch={watch} /> : <p>This page doesn't draw terminals.</p>}</div>
    </Well>
  );
}

/** The chief of staff's tab: its room, or Start. */
function useChief(on: boolean, place: Place, view: PanelView, setView: (v: PanelView) => void): { shown: Shown; room: ChatRoom | null } {
  const { fleet, project, hostedIn } = place;
  const { projectName } = usePlatformApi();
  const r = chat.rooms.find((x) => x.staff && x.project === project) ?? null;
  const l = r?.lead;
  const ov = on && l ? fleet.panes.map((p) => ovPane(fleet, p)) : [];
  const lead = l ? boardLead(l, ov, null, (m) => fleet.hostById(m)?.name ?? fleet.host(m)?.name, projectName) : null;
  const at = leadAwake(lead, l && fleet.hostById(l.machine));
  const head = chiefHead(at);
  const pane = lead?.peek.lead;
  const term = on && pane && surfacesFor().terminal ? { machine: pane.machine ?? pane.host, pane: pane.id } : null;
  if (!on) return { shown: { head, body: null }, room: null };
  if (!r) {
    const body =
      chat.on === false ? (
        <PanelNote data-panel-chat-off>Your chief of staff talks with you in a room, and rooms come with chat, which is off for you.</PanelNote>
      ) : chat.on ? (
        <StaffStart project={project} fleet={fleet} hostedIn={hostedIn} />
      ) : (
        <p class="dim">Reaching chat…</p>
      );
    return { shown: { head, body }, room: null };
  }
  const why = r.lead
    ? `What you write here goes to ${agentName({ session: r.lead.session, machine: r.lead.machine_name })} on ${r.lead.machine_name || "its machine"}. Only you and your agents read it.`
    : "No session leads this room yet, so what you write waits here. Only you and your agents read it.";
  const lede =
    (at.state === "asleep" || at.state === "none") && r.project ? (
      <StaffMove key={at.state === "asleep" ? at.machine : ""} project={r.project} fleet={fleet} hostedIn={hostedIn} asleepOn={at.state === "asleep" ? at.machine : undefined} />
    ) : (at.state === "awake" || at.state === "idle" || at.state === "unknown") && r.project ? (
      <StaffRestart key={at.machine} project={r.project} fleet={fleet} hostedIn={hostedIn} runsOn={at.machine} />
    ) : undefined;
  const terminal = term && view === "terminal";
  return {
    room: r,
    shown: {
      head,
      lede,
      flush: !terminal,
      view: term ? view : undefined,
      onView: term ? setView : undefined,
      body: terminal ? (
        <TerminalWell machine={term.machine} pane={term.pane} label={head.machine ?? ""} />
      ) : (
        <div class="chat-thread" data-ov-staff={r.id}>
          <RoomBody key={r.id} room={r} phone={false} embedded target={head.machine ? `staff room · ${head.machine}` : "staff room"} name="Chief of staff" empty={{ title: "Nothing said to your chief of staff yet.", why }} />
        </div>
      ),
    },
  };
}

/** A thread target's room: a room chat lists, or an object's thread (a board's, an issue's), opened. */
function useThread(t: Extract<PanelTarget, { kind: "thread" }> | null): { shown: Shown; room: ChatRoom | null } {
  const id = t?.room ?? "";
  const [opened, setOpened] = useState<{ id: string; room: ChatRoom | null; error?: string } | null>(null);
  const listed = id ? chat.rooms.find((r) => r.id === id || (r.kind === "object" && r.target === id)) : undefined;
  useEffect(() => {
    if (!id || listed || !chat.on) return;
    let live = true;
    const open = id.startsWith("brd_") ? boards.thread(id) : chat.openObject(id);
    open.then(
      (room) => live && setOpened({ id, room }),
      (e: Error) => live && setOpened({ id, room: null, error: e.message }),
    );
    return () => void (live = false);
  }, [id, !!listed, chat.on]);
  const room = listed ?? (opened?.id === id ? opened.room : null);
  const head: PanelHead = { name: t?.title ?? "", rung: t?.parent !== undefined ? "thread" : "room", person: true };
  if (!t) return { shown: { head, body: null }, room: null };
  if (chat.on === false) return { shown: { head, body: <PanelNote>Rooms come with chat, which is off for you. A team owner turns it on for the team under Teams.</PanelNote> }, room: null };
  if (!room) return { shown: { head, body: <p class="dim">{opened?.error ? `The room didn't load: ${opened.error}` : "Loading the room…"}</p> }, room: null };
  return {
    room,
    shown: {
      head,
      flush: true,
      body: (
        <div class="chat-thread" data-panel-room={room.id}>
          <RoomBody key={room.id} room={room} phone={false} embedded target={`room · ${t.title}`} name={t.title} empty={{ title: "Nothing said here yet.", why: "Everyone in the project reads it. Write @name to notify someone." }} />
        </div>
      ),
    },
  };
}

/** A transcript that keeps its newest line in sight as it grows. */
function Tail({ children, text }: { children: ComponentChildren; text: string }) {
  const ref = useRef<HTMLDivElement>(null);
  useLayoutEffect(() => {
    const box = ref.current?.closest<HTMLElement>("[data-panel-body]");
    if (box) box.scrollTop = box.scrollHeight;
  }, [text]);
  return <div ref={ref}>{children}</div>;
}

/** An agent's tab: its session, found on the page's machines, else a cloud workspace's. */
function useAgent(t: Extract<PanelTarget, { kind: "agent" }> | null, place: Place, view: PanelView, setView: (v: PanelView) => void): Shown {
  const { fleet, project } = place;
  const api = usePlatformApi();
  const key = t ? panelKey(t) : "";
  const host = t ? (fleet.hostById(t.machine) ?? fleet.host(t.machine)) : undefined;
  // A machine the fleet doesn't list (a pane it was given) by its name.
  const hostName = host?.name ?? t?.machine;
  const onHost = hostName ? fleet.panes.filter((p) => p.host === hostName) : [];
  const bySession = t?.session ? onHost.filter((p) => p.session?.name === t.session) : [];
  const pane = (t?.pane !== undefined ? onHost.find((p) => p.id === t.pane) : undefined) ?? bySession.find((p) => kindOf(p) === "agent") ?? bySession[0];
  const hostedId = t && !pane && t.session?.startsWith("ses_") ? t.session : null;
  const hosted = useHosted(hostedId);
  const box = hosted?.card.machine ? fleet.hostById(hosted.card.machine) : undefined;
  const block = hosted?.card.block && /^\d+$/.test(hosted.card.block) ? Number(hosted.card.block) : null;
  // Where its lines are read: its pane, else its cloud workspace's block while the box runs.
  const read = pane ? { host: pane.host, id: pane.id } : box?.state === "connected" && block !== null && !hosted?.card.parked ? { host: box.name, id: block } : null;
  // An agent block's lines (a cloud workspace's too) are its transcript, in Markdown.
  const md = pane ? pane.info.type === "agent" : !!hostedId;
  const lines = useLines(fleet, read, md);

  // Ended: its agent exited, its cloud session is done, or its pane is gone from a machine the page reaches.
  const exited = pane?.info.reason?.kind === "exited" ? pane.info.reason.since_ms : undefined;
  const hostedEnded = hosted && ["done", "failed", "stopped"].includes(hosted.card.state) ? ms(hosted.card.ended_at ?? hosted.card.updated_at) : undefined;
  const gone = !!t && !pane && !hostedId && t.pane !== undefined && host?.state === "connected";
  const ended = exited !== undefined || hostedEnded !== undefined || gone;
  const endedAt = exited ?? hostedEnded;
  const todo = useTodo(ended && t ? t : null, hostName, project);

  const [said, setSaid] = useState<{ key: string; text: string } | null>(null);
  const [busy, setBusy] = useState(false);
  const say = (text: string) => setSaid({ key, text });

  if (!t) return { head: { name: "", rung: "" }, body: null };
  const machineName = host?.name ?? (hosted?.card.workspace_name || pane?.host);
  const name = agentName({ role: t.role, title: t.title ?? (pane ? sessionName(pane) : undefined), session: t.session, machine: machineName ?? null });
  const team = !!pane && !yours(fleet, pane);
  const far = !pane && !hostedId && !gone;
  const age = needAge(endedAt);
  const standing = pane ? standingOf(pane) : null;
  const head: PanelHead = {
    name,
    rung: rungOf(t),
    machine: machineName ? shortHost(machineName) : undefined,
    ...(ended
      ? { state: "idle" as State, word: age ? `ended ${age} ago` : "ended", status: "ended" }
      : standing
        ? { state: MARK[standing] ?? "idle", word: standing === "needs" ? "asks you" : undefined, status: standing }
        : hosted
          ? { state: (hosted.card.state === "working" ? "working" : hosted.card.state === "needs_input" ? "asks" : "idle") as State, status: hosted.card.state }
          : { status: far ? "far" : "reading" }),
  };

  // The terminal: the pane itself, live, in a well, where the page draws one.
  const term = pane && surfacesFor().terminal && api.LiveWork ? { machine: t.machine, pane: pane.id } : null;
  if (term && view === "terminal")
    return { head, view, onView: setView, body: <TerminalWell machine={term.machine} pane={term.pane} label={shortHost(pane!.host)} watch={team} /> };

  // Its lines: read from its machine; a cloud workspace's, from its box, else its last answer.
  let tail = lines;
  let says = "Reading its last lines…";
  if (!read) {
    if (far) {
      tail = null;
      says = farWhy(fleet, host, t.machine);
    } else if (hostedId) {
      tail = hosted ? lastAnswer(hosted) : null;
      says = hosted ? "Nothing said yet." : "Reading the session…";
    } else if (gone) {
      tail = null;
      says = "Its pane is gone from its machine, so its lines are too.";
    }
  }
  const text = tail?.join("\n") ?? "";
  // Claude Code in a terminal: its screen, its tool calls folded as an agent block's are.
  const claude = !md && !!pane && kindOf(pane) === "agent" && isClaudeScreen(text);
  const body = (
    <>
      <Tail text={text}>
        {(md || claude) && tail?.length ? (
          <Transcript text={text} claude={claude} data-panel-transcript={key} data-panel-live={read ? "" : undefined} />
        ) : (
          <Well>
            <pre data-panel-transcript={key} data-panel-live={read ? "" : undefined}>
              {tail?.length ? text : says}
            </pre>
          </Well>
        )}
        {ended && (
          <PanelNote end data-panel-ended>
            read-only · session ended{age ? ` ${age} ago` : ""}
          </PanelNote>
        )}
      </Tail>
      {said?.key === key && (
        <p class="dim" role="status" data-panel-said>
          {said.text}
        </p>
      )}
    </>
  );

  const post = async (path: string, payload: unknown, done: string) => {
    setBusy(true);
    try {
      const res = await fleet.request(pane!.host, "POST", path, payload);
      const err = res.ok ? null : ((await res.json<{ error?: string }>().catch(() => null))?.error ?? `HTTP ${res.status}`);
      say(err ? `Couldn't: ${err}` : done);
      return !err;
    } catch {
      say(`Couldn't: ${pane!.host} isn't reachable now.`);
      return false;
    } finally {
      setBusy(false);
    }
  };

  let foot: ComponentChildren;
  if (ended) foot = todo ? <TodoFoot key={todo.id} item={todo} /> : undefined;
  else if (team) foot = <PanelNote data-panel-readonly>{pane!.person.name}'s session. You see how it stands and its last lines; it takes nothing from you here.</PanelNote>;
  else if (far) foot = <PanelNote data-panel-readonly>{says}</PanelNote>;
  else if (pane) {
    const isBlock = pane.info.type === "agent";
    const agent = kindOf(pane) === "agent";
    const up = fleet.host(pane.host)?.state === "connected" && !pane.stale;
    const off = !!pane.info.mentions_off;
    // An agent block takes it as its composer does; a terminal has it typed, with Enter.
    // Files go to its machine first (M70): an agent block gets their paths with
    // the text, a terminal has them pasted before it.
    const send = async (text: string, files: File[]) => {
      let paths: string[] = [];
      if (files.length) {
        setBusy(true);
        try {
          paths = await store((m, p, b) => fleet.request(pane.host, m, p, b), pane.id, files, say);
        } catch (e) {
          say((e as Error).message);
          return false;
        } finally {
          setBusy(false);
        }
        if (!isBlock && !(await post(`/api/panes/${pane.id}/paste`, { paths, force: true }, "Pasted."))) return false;
      }
      if (isBlock) return post(`/api/blocks/${pane.id}/call/send`, { text, files: paths }, "Sent: it goes in at its next tool call.");
      return post(`/api/panes/${pane.id}/send`, { text, enter: true }, "Typed, with Enter.");
    };
    // Stop as the strip's does: an agent block's turn is cancelled, Claude Code gets Escape; a shell, Ctrl-C.
    const stop = () => (isBlock ? post(`/api/blocks/${pane.id}/call/cancel`, {}, "Stopped.") : post(`/api/panes/${pane.id}/keys`, { keys: [agent ? "Escape" : "C-c"] }, "Stopped."));
    foot = (
      <Composer
        placeholder={!up ? `${shortHost(pane.host)} isn't reachable now` : isBlock || agent ? `Reply to ${name}: it reads it at its next tool call` : `Type a command for the shell on ${shortHost(pane.host)}`}
        target={`to ${shortHost(pane.host)}`}
        disabled={!up}
        files
        onSend={send}
        actions={
          <>
            {agent && (
              <Button disabled={busy || !up} data-panel-silence={off ? "allow" : "silence"} onClick={() => void post(`/api/panes/${pane.id}/mentions`, { on: off }, off ? "Mentions allowed." : "Silenced: mentions wait.")}>
                {off ? "Allow" : "Silence"}
              </Button>
            )}
            <Button disabled={busy || !up} data-panel-stop onClick={() => void stop()}>
              Stop
            </Button>
          </>
        }
      />
    );
  } else if (hosted) {
    const may = hosted.may;
    const stop = async () => {
      setBusy(true);
      try {
        await call<SessionView>("POST", `/api/sessions/${enc(hostedId!)}/stop`);
        say("Stopped.");
      } catch (e) {
        say(`Couldn't: ${(e as Error).message}`);
      } finally {
        setBusy(false);
      }
    };
    // Its box reads its thread: a reply there is its next prompt.
    const send = async (text: string, files: File[]) => {
      try {
        const ids = await Promise.all(files.map((f) => upload(hosted.card.room, f, pastedName(f), () => {}).done.then((x) => x.id)));
        await chat.reply(hosted.card.room, hosted.card.parent, text, false, ids);
        say("Sent in its thread: it goes in at its next turn.");
        return true;
      } catch (e) {
        say(`Couldn't: ${(e as Error).message}`);
        return false;
      }
    };
    foot =
      may && chat.on ? (
        <Composer
          placeholder="Reply in its thread"
          target="its thread"
          files
          onSend={send}
          actions={
            <Button disabled={busy} data-panel-stop onClick={() => void stop()}>
              Stop
            </Button>
          }
        />
      ) : (
        <PanelNote data-panel-readonly>{may ? "Its thread comes with chat, which is off for you." : "You're watching this session: an editor of the project answers it."}</PanelNote>
      );
  }
  return { head, body, foot, view: term ? view : undefined, onView: term ? setView : undefined };
}

/** A platform time in ms (some say seconds). */
const ms = (t: number) => (t < 1e12 ? t * 1000 : t);

/** Why the page can't read a session on `host`: the reader's own machine
 * not connected here, a teammate's that shares no panes, or one it doesn't
 * know. */
function farWhy(fleet: Fleet, host: { name: string; state: string } | undefined, machine: string): string {
  const h = host && fleet.list.find((x) => x.name === host.name);
  if (!h) return `It runs on ${machine}, which this page doesn't reach.`;
  const name = shortHost(h.name);
  if (h.owner) return `It runs on ${name}, ${h.owner}'s machine, whose panes aren't shared with you.`;
  if (h.state !== "connected") return `${name} isn't connected to this page. Connect it to read and talk to this session.`;
  return `${name} is connected, but this page doesn't see this session on it.`;
}

/** An ended session's foot, on the reader's to-do from it: its note and
 * answers in place of the composer, and once answered, its receipt. */
function TodoFoot({ item }: { item: InboxItem }) {
  const [receipt, setReceipt] = useState<Receipt | null>(null);
  if (receipt) return <PanelNote data-panel-todo-receipt>{receiptLine(receipt)}</PanelNote>;
  const answer = async (choice: string, note: string) => {
    await answerItem(item.id, choice, note);
    setReceipt(await receiptOf(item, choice));
  };
  return (
    <div data-panel-todo-foot={item.id}>
      <PanelNote data-panel-todo>Done continues this conversation with your note.</PanelNote>
      <TodoAnswer options={item.answer?.options.length ? item.answer.options : ["Done", "Can't"]} answer={answer} />
    </div>
  );
}

/** The reader's open to-do from an ended session's block, if it has one
 * (its subject names the asker's machine and block): Done carries the
 * conversation on. */
function useTodo(t: Extract<PanelTarget, { kind: "agent" }> | null, host: string | undefined, project: string): InboxItem | null {
  const [items, setItems] = useState<InboxItem[]>([]);
  const on = !!t;
  useEffect(() => {
    if (!on) return;
    let live = true;
    readInbox(project).then(
      (v) => live && setItems(v.items),
      () => {},
    );
    return () => void (live = false);
  }, [on, project, chat.inbox?.heard ?? 0]);
  // Answered here, it leaves the Inbox: the tab keeps it, for its receipt.
  const kept = useRef<{ key: string; item: InboxItem } | null>(null);
  if (!t) return null;
  const key = panelKey(t);
  const found = items.find((i) => i.kind === "todo" && !i.done_at && i.answer && (i.subject.machine === t.machine || i.subject.machine === host) && String(i.subject.pane ?? "").replace(/^%/, "") === String(t.pane ?? ""));
  if (found) kept.current = { key, item: found };
  return found ?? (kept.current?.key === key ? kept.current.item : null);
}

/** A pane's last lines, read from its machine while the tab is open and the page is shown; `md`: they're Markdown. */
function useLines(fleet: Fleet, at: { host: string; id: number } | null, md: boolean): string[] | null {
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
        if (live) setLines(lastLines(all, md, LINES));
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
  }, [key, md]);
  return lines;
}

/** A cloud workspace's session as the platform has it, read again while its tab is open. */
function useHosted(ses: string | null): SessionView | null {
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
function lastAnswer(v: SessionView): string[] {
  const t = chat.on ? chat.threads.get(v.card.room, v.card.parent) : undefined;
  const answer = t?.replies.findLast((m) => saidOf(m) === v.card.session);
  if (answer) return lastLines(answer.text.split("\n"), true, LINES);
  return v.events.slice(-LINES).map((e) => e.text);
}
