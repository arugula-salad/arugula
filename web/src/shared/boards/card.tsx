// A board row's drawer (lanes, aru-125; #369), wherever the board is drawn:
// the platform page's and the board block's (./view.tsx). A side sheet with
// the row's item, read-only: its lane, marks and note, its live session,
// the body, the latest comments, the PRs' checks, relations, Docs, the
// sessions and attachments, and the proof. The chips a row shows on the
// board too (its sessions, attachments, owners and labels) are here, and the
// board imports them.
//
// The drawer is drawn by both bundles, so it imports nothing of the shell's:
// a live pane comes through `PlatformApi.LivePane`, where the page has one.

import { useEffect, useState } from "preact/hooks";
import type { RelationKind, BoardLane, BoardAttachment, BoardRow, BoardRowSession, BoardView, ForgeText } from "../proto";
import { chat } from "../chat-store";
import { usePlatformApi } from "../platform-api";
import { Card } from "../ui/card";
import { Markdown } from "../ui/markdown";
import { ArrowIcon, CloseIcon, ExternalIcon, IconButton, PlusIcon } from "../ui/icons";
import { SideSheet } from "../ui/side-sheet";
import { StateChip, Chip, ObjectChip } from "../ui/state-chip";
import { usePhone, useSubscribe } from "../ui/hooks";
import { day } from "../time";
import { boards, type ForgeChecks } from "./store";
import { nudge, type Reorder, type stepPlace } from "./steps";
import { DocLink, DocPicker } from "./docs";
import { MOVE_LANES, closedWithLine, laneSetLine, checkTone, checkWord, liveTone, prsOf, assigneesLine, blockFacts, BLOCK_LABEL, chipOf, settledFacts, holdsRow, isCard, liveWord, queueChip, marksOf, waitingOf, notFinishedLine, type NotFinished, pointerHref, pointerLabel, relationLine, RELATION_KINDS, liveSessions, type LiveSession, topAndMore, initialsOf } from "./text";

export type Act = (write: () => Promise<BoardView>) => Promise<boolean>;

/** A session started on the row's issue (#843): its branch without the `arugula/` prefix, and its state, a link to its thread where the reader has one. Dim once it no longer moves the row. */
export function SessionChip({ s, project }: { s: BoardRowSession; project: string }) {
  const api = usePlatformApi();
  const href = api.sessionHref?.(project, s.session);
  const branch = s.branch.replace(/^arugula\//, "");
  const state = s.state.replace(/_/g, " ");
  return (
    <ObjectChip
      class={s.live ? undefined : "dim"}
      title={s.live ? "An agent session working on this issue" : "An agent session that has stopped moving this row"}
      fact={{ tone: s.live ? (s.state === "needs_input" ? "accent" : "muted") : "muted", text: state }}
      data-board-session={s.session}
      data-board-session-live={s.live ? "" : undefined}
    >
      {href ? (
        <a class="board-ptr" href={href} target="_blank" rel="noopener noreferrer">
          {branch}
        </a>
      ) : (
        <span class="board-ptr">{branch}</span>
      )}
    </ObjectChip>
  );
}

/** An attachment as an object chip: what it points at, and its state. An agent block is one chip, its title and its state (no second status line), a link to its pane where the reader can reach it: control's page on the platform page, the pane itself in the shell. `facts` says its branch and the rest after the chip, dim: `settled`, once it has stopped; `all`, in the drawer. */
export function AttachmentChip({ a, platform, facts = "settled", stalled, waiting }: { a: BoardAttachment; platform: (path: string) => string | undefined; facts?: "settled" | "all"; stalled?: string | null; waiting?: { words: string; mine: boolean } | null }) {
  const api = usePlatformApi();
  const c = chipOf(a, stalled, waiting);
  const p = a.pointer;
  // An agent block with no name of its own is named by its pane, as the machine reports it.
  if (p.kind === "block" && !a.block?.title?.trim()) c.label = api.paneTitle(p.machine, p.pane) || c.label;
  const text = a.role === "proof" ? "Proof" : c.label;
  const href = pointerHref({ pointer: p, live: a.live }, platform);
  // The pane: a link to control's page here; in the shell, when the machine is one of the reader's. A block whose machine the reader isn't told (a blank id) links nowhere.
  const pane = p.kind === "block" && p.machine ? (api.controlHref ? { href: api.controlHref(`pane=${p.machine}.${p.pane}`) } : api.machine(p.machine) ? { go: () => api.goToPane(p.machine, p.pane, null) } : null) : null;
  const name = pane?.href ? (
    <a class="board-ptr" href={pane.href} target="_blank" rel="noopener noreferrer" title="Open its pane" data-board-block-open>
      {text}
    </a>
  ) : pane?.go ? (
    <a class="board-ptr" href="#" title="Open its pane" data-board-block-open onClick={(e) => (e.preventDefault(), pane.go!())}>
      {text}
    </a>
  ) : href ? (
    <a class="board-ptr" href={href} target={p.kind === "forge_item" || p.kind === "link" ? "_blank" : undefined} rel="noopener noreferrer">
      {text}
    </a>
  ) : (
    <span class="board-ptr">{text}</span>
  );
  const more = a.block ? (facts === "all" ? blockFacts(a.block) : settledFacts(a.block)) : [];
  return (
    <>
      <ObjectChip
        proof={a.role === "proof"}
        class={a.role}
        title={a.role === "proof" ? c.label : a.block?.state === "working" ? BLOCK_LABEL.working : undefined}
        fact={c.state ? { tone: c.tone, text: c.state } : null}
        data-board-chip-state={c.state ? c.tone : undefined}
        data-board-attachment={a.id}
        data-attach-role={a.role}
        data-board-block={a.block?.state}
      >
        {name}
      </ObjectChip>
      {more.length > 0 && (
        <span class="board-fact dim" data-board-block-facts>
          {more.join(" · ")}
        </span>
      )}
    </>
  );
}

/** A card's assignees as initials in small circles: two, then `+N`. Named in full for a screen reader and on hover. */
export function Owners({ who, all }: { who: string[]; all?: boolean }) {
  const list = who.filter(Boolean);
  if (!list.length) return null;
  const { top, more } = topAndMore(list, all ? list.length : 2);
  return (
    <span class="board-who" role="img" aria-label={assigneesLine(list)} title={list.join(", ")} data-board-who={list.join(", ")}>
      {top.map((w) => (
        <span key={w} class="board-avatar" aria-hidden="true" data-board-avatar>
          {initialsOf(w)}
        </span>
      ))}
      {more > 0 && (
        <span class="board-avatar more" aria-hidden="true" data-board-avatar-more>
          +{more}
        </span>
      )}
    </span>
  );
}

/** An item's labels as chips: three, then `+N`; on a phone (its CSS) only the `+N` of all of them. The drawer lists every one. */
export function Labels({ labels, all }: { labels: string[]; all?: boolean }) {
  if (!labels.length) return null;
  const { top, more } = topAndMore(labels, all ? labels.length : 3);
  return (
    <div class={all ? "board-labels" : "board-line board-labels"} data-board-labels>
      <span class="board-labels-top">
        {top.map((l) => (
          <Chip key={l} kind="tag" class="board-label" data-board-label={l}>
            {l}
          </Chip>
        ))}
        {more > 0 && (
          <Chip kind="tag" class="board-label" title={labels.slice(top.length).join(", ")} data-board-label-more>
            +{more}
          </Chip>
        )}
      </span>
      {!all && (
        <Chip kind="tag" class="board-label board-labels-all" title={labels.join(", ")} data-board-labels-all>
          +{labels.length}
        </Chip>
      )}
    </div>
  );
}

/** What the drawer read of a row's item. `body` null: the reader may not read it here. */
interface Read {
  body: string | null;
  comments: { who: string; text: string; at: number }[];
  checks: { label: string; checks: ForgeChecks["checks"] }[];
}

/** The most comments the drawer shows. */
const DRAWER_COMMENTS = 3;

const commentOf = (t: ForgeText) => ({ who: t.author ?? "", text: t.body, at: t.created_at });

/** A row's item as the drawer reads it, with the APIs the pages use: a native issue's body and thread, a mirrored item's content, the checks of each PR behind the row. Any read that fails leaves its part out. */
async function readRow(r: BoardRow): Promise<Read> {
  const out: Read = { body: null, comments: [], checks: [] };
  const p = r.pointer;
  const reads: Promise<void>[] = [];
  if (p.kind === "issue") {
    reads.push(boards.issue(p.id).then((i) => void (out.body = i.body)).catch(() => {}));
    reads.push(
      chat
        .openObject(p.id)
        .then(async (room) => {
          await chat.load(room.id);
          out.comments = (chat.messages(room.id)?.list ?? []).filter((m) => m.text).slice(-DRAWER_COMMENTS).map((m) => ({ who: m.name, text: m.text, at: m.at }));
        })
        .catch(() => {}),
    );
  } else if (p.kind === "forge_item" && r.live?.provider !== "plane") {
    reads.push(
      boards
        .content(p.id)
        .then((c) => {
          out.body = c.body?.body ?? "";
          out.comments = c.comments.filter((t) => t.kind === "comment" || !t.path).slice(-DRAWER_COMMENTS).map(commentOf);
        })
        .catch(() => {}),
    );
  }
  const prs = prsOf(r).slice(0, 3);
  const checks = await Promise.all(prs.map((x) => boards.checks(x.id).then((c) => ({ label: x.label, checks: c.checks })).catch(() => null)));
  await Promise.all(reads);
  out.checks = checks.flatMap((c) => (c ? [c] : []));
  return out;
}

/** What the card's chip says in full on hover: the words, and who marked it and when. */
export function notFinishedTitle(n: NotFinished): string {
  const who = n.by ? ` Marked by ${n.by}${n.at ? ` on ${day(n.at)}` : ""}.` : "";
  const merged = n.merged ? " A PR of this row merged, and the issue stays open." : "";
  return `${notFinishedLine(n)}.${merged}${who}`;
}

/** The drawer's Not finished section (#841), above the attachments: the reason, who marked it and when, with Clear the mark for those who may edit; with no mark, Mark not finished… opens a line for what is left. Only on a native issue or a Plane item that isn't done. */
function NotFinishedSection({ r, nf, write, act, id }: { r: BoardRow; nf: NotFinished | null; write: boolean; act: (write: () => Promise<BoardView>) => Promise<boolean>; id: string }) {
  const [marking, setMarking] = useState(false);
  const [reason, setReason] = useState("");
  const holdable = r.pointer.kind === "issue" || (r.pointer.kind === "forge_item" && r.live?.provider === "plane");
  if (!holdable || (r.lane === "done" && !nf)) return null;
  // A mark with a writer is a hold; one with none only says a PR merged.
  const held = !!nf && nf.at !== undefined;
  const save = async () => {
    if (await act(() => boards.hold(id, r.id, reason.trim()))) {
      setMarking(false);
      setReason("");
    }
  };
  return (
    <section data-board-drawer-not-finished>
      <h3>Not finished</h3>
      {nf && (
        <p data-board-drawer-not-finished-reason>
          {nf.reason ?? "No reason given."}
          {held && nf.by && (
            <span class="dim">
              {" "}
              · {nf.by}
              {nf.at ? `, ${day(nf.at)}` : ""}
            </span>
          )}
        </p>
      )}
      {!nf && <p class="dim">A merged PR closes this issue. Mark it not finished to keep it open, with what is left.</p>}
      {write && !marking && (
        <div class="board-line">
          {held && (
            <button type="button" class="secondary small" data-board-clear-mark title="Take the mark off. The issue closes now if a PR that names it has merged." onClick={() => void act(() => boards.release(id, r.id))}>
              Clear the mark
            </button>
          )}
          <button type="button" class="secondary small" data-board-mark-start onClick={() => { setReason(nf?.reason ?? ""); setMarking(true); }}>
            {held ? "Change the reason…" : "Mark not finished…"}
          </button>
        </div>
      )}
      {write && marking && (
        <div class="board-line" data-board-mark-form>
          <input
            type="text"
            value={reason}
            maxLength={200}
            placeholder="What is left"
            aria-label="What is left"
            data-board-mark-input
            autoFocus
            onInput={(e) => setReason(e.currentTarget.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter" && reason.trim()) void save();
              if (e.key === "Escape") setMarking(false);
            }}
          />
          <button type="button" class="primary small" data-board-mark-save disabled={!reason.trim()} onClick={() => void save()}>
            Mark
          </button>
          <button type="button" class="secondary small" data-board-mark-cancel onClick={() => setMarking(false)}>
            Cancel
          </button>
        </div>
      )}
    </section>
  );
}

/** The drawer: a row's item, read-only, beside the board (a sheet on a phone). Its title, state and link out; its live session; the body, the latest comments, the PRs' checks, the attachments and the proof's evidence. */
export function Drawer({ steps, r, platform, close, project, others, write, act, id, moveLane, nameOf, who = "steer" }: { /** In Steps, the row's place in its group and its step, and the move ↑ and ↓ send on a phone. */ steps?: { place: ReturnType<typeof stepPlace>; move: (m: Reorder) => void }; /** What the reader does here: `steer` (the default) may take a live session's pane; `read` (Product mode, later) sees each session's state card. */ who?: "steer" | "read"; r: BoardRow; platform: (path: string) => string | undefined; close: () => void; project: string; others: BoardRow[]; write: boolean; act: (write: () => Promise<BoardView>) => Promise<boolean>; id: string; moveLane: (row: string, lane: BoardLane) => void; nameOf: (account: string) => string | undefined }) {
  const [read, setRead] = useState<Read | null>(null);
  const phone = usePhone();
  const href = pointerHref(r, platform);
  const key = `${r.id}:${r.updated_at}`;
  useEffect(() => {
    setRead(null);
    let live = true;
    void readRow(r).then((v) => live && setRead(v));
    return () => void (live = false);
  }, [key]);
  const title = isCard(r.live) ? r.live.title : pointerLabel(r.pointer, r.live) || "Note";
  const evidence = r.attachments.filter((a) => a.role === "proof");
  const readable = r.pointer.kind === "issue" || (r.pointer.kind === "forge_item" && r.live?.provider !== "plane");
  const external = r.pointer.kind === "forge_item" || r.pointer.kind === "link";
  const marks = marksOf(r);
  const waiting = waitingOf(r);
  const holding = r.attachments.some(holdsRow);
  const api = usePlatformApi();
  // The row's sessions, and the one shown: the one that needs you, else the one that moved last, until another is picked.
  const [picked, setPicked] = useState<string | null>(null);
  const lives = liveSessions(r, waiting, marks.stalled?.words);
  const shown = lives.find((x) => x.key === picked) ?? lives[0];
  // Where the page reaches machines itself (the app), a session's question is answered at the top, and on a phone its pane fills the screen.
  const Ask = shown?.kind === "block" && shown.machine ? api.LiveAsk : undefined;
  const full = !!api.LiveAsk && !!api.LivePane && who === "steer" && shown?.kind === "block" && !!shown.machine;
  // The agent blocks are in Live session: Attached keeps the PRs, runs and the rest.
  const attached = r.attachments.filter((a) => !(a.role === "work" && a.pointer.kind === "block"));
  return (
    <SideSheet title={title} close={close} class={full ? "board-drawer board-drawer-live" : "board-drawer"} root={{ "data-board-drawer": r.id }} titleAttrs={{ "data-board-drawer-title": true }} closeAttrs={{ "data-board-drawer-close": true }}>
      {Ask && shown?.kind === "block" && <Ask key={shown.key} machine={shown.machine} pane={shown.pane} />}
      <div class="board-line board-drawer-lane">
        {write ? (
          <label class="dim">
            Lane{" "}
            <select
              data-board-drawer-lane
              value={r.lane}
              onChange={(e) => {
                const lane = e.currentTarget.value as BoardLane;
                // The row's lane is what the board says: a move that waits on a confirm, or is refused, leaves it.
                e.currentTarget.value = r.lane;
                moveLane(r.id, lane);
              }}
            >
              {MOVE_LANES.map((l) => (
                <option key={l.lane} value={l.lane}>
                  {l.title}
                </option>
              ))}
            </select>
          </label>
        ) : (
          <span class="dim" data-board-drawer-lane-name>
            {MOVE_LANES.find((l) => l.lane === r.lane)?.title}
          </span>
        )}
        {laneSetLine(r, nameOf) && (
          <Chip kind="tag" data-board-lane-set>
            {laneSetLine(r, nameOf)}
          </Chip>
        )}
      </div>
      {steps?.place && phone && (
        <div class="board-line board-drawer-steps" data-board-drawer-steps>
          {([true, false] as const).map((up) => {
            const m = nudge(steps.place!.group, steps.place!.step, r.id, up);
            return (
              <IconButton key={String(up)} label={up ? "Move up in its step" : "Move down in its step"} class="secondary small" disabled={!m} data-board-step-move={up ? "up" : "down"} onClick={() => m && steps.move(m)}>
                <ArrowIcon dir={up ? "up" : "down"} size="xs" />
              </IconButton>
            );
          })}
        </div>
      )}
      <div class="board-line" title={r.live?.state === "closed" && r.lane === "done" ? liveWord(r.live) : undefined}>
        {r.live && !(r.live.state === "closed" && r.lane === "done") && <StateChip tone={queueChip(r.live)?.tone ?? liveTone(r.live.state)}>{queueChip(r.live)?.word ?? liveWord(r.live)}</StateChip>}
        {href && (
          <a class="board-ptr" href={href} target={external ? "_blank" : undefined} rel="noopener noreferrer" data-board-drawer-link>
            {pointerLabel(r.pointer, r.live)} <ExternalIcon size="xs" />
          </a>
        )}
      </div>
      {closedWithLine(r.live) && (
        <div class="board-line dim" data-board-closed-with title="The merged pull request that closed this issue, and the words of it that did">
          {closedWithLine(r.live)}
        </div>
      )}
      {((marks.stalled && !holding && !waiting) || (waiting && !holding) || marks.blocked.length > 0 || r.marks?.some((m) => m.kind === "cycle")) && (
        <div class="board-line" data-board-drawer-marks>
          {r.marks?.some((m) => m.kind === "cycle") && (
            <StateChip tone="warn" data-board-mark="cycle" title="This row and others wait on each other, so they share a step">
              cycle
            </StateChip>
          )}
          {marks.stalled && !holding && !waiting && (
            <StateChip tone="warn" data-board-mark="stalled" title="Every agent on this row has been idle past the project's limit, and no pull request is open">
              {marks.stalled.words}
            </StateChip>
          )}
          {waiting && !holding && (
            <StateChip tone={waiting.mine ? "accent" : "muted"} data-board-mark="waiting" title="This row has an open question for a person">
              {waiting.words}
            </StateChip>
          )}
          {marks.blocked.map((m, i) => (
            <StateChip key={i} tone="accent" data-board-mark="blocked_by" title={m.title ?? "You can't read the item that blocks this one"}>
              {m.words}
            </StateChip>
          ))}
        </div>
      )}
      {r.note && <p class="dim">{r.note}</p>}
      {((r.live?.labels?.length ?? 0) > 0 || (r.live?.assignees?.length ?? 0) > 0) && (
        <div class="board-line board-drawer-tags" data-board-drawer-tags>
          <Owners who={r.live?.assignees ?? []} all />
          {r.live?.assignees?.length ? <span class="dim" data-board-drawer-owners>{r.live.assignees.join(', ')}</span> : null}
          <Labels labels={r.live?.labels ?? []} all />
        </div>
      )}
      <LiveSession r={r} project={project} all={lives} s={shown} pick={setPicked} who={who} write={write} act={act} id={id} />
      {readable && (
        <section data-board-drawer-body>
          {!read ? (
            <p class="dim">Loading…</p>
          ) : read.body === null ? (
            <p class="dim">{href ? <a href={href} target="_blank" rel="noopener noreferrer">Open on GitHub</a> : "Not available here."}</p>
          ) : read.body.trim() ? (
            <Markdown text={read.body} refs={null} />
          ) : null}
        </section>
      )}
      {read && read.comments.length > 0 && (
        <section data-board-drawer-comments>
          <h3>Latest comments</h3>
          {read.comments.map((c, i) => (
            <div key={i} class="board-drawer-comment">
              <div class="dim">
                {c.who} · {day(c.at)}
              </div>
              <Markdown text={c.text} refs={null} />
            </div>
          ))}
        </section>
      )}
      {read && read.checks.length > 0 && (
        <section data-board-drawer-checks>
          <h3>Checks</h3>
          {read.checks.map((c) => (
            <div key={c.label}>
              {read.checks.length > 1 && <div class="dim">{c.label}</div>}
              {c.checks.length === 0 && <p class="dim">No checks reported.</p>}
              <ul class="board-checks">
                {c.checks.map((k) => (
                  <li key={k.name} data-board-check={k.name}>
                    <CheckChip tone={checkTone(k)} word={checkWord(k)} />
                    {k.url && /^https:\/\//.test(k.url) ? (
                      <a href={k.url} target="_blank" rel="noopener noreferrer">
                        {k.name}
                      </a>
                    ) : (
                      k.name
                    )}
                  </li>
                ))}
              </ul>
            </div>
          ))}
        </section>
      )}
      {(r.relations?.length || (write && itemId(r))) && <Relations r={r} project={project} others={others} write={write} act={act} id={id} />}
      <RowDocs r={r} project={project} platform={platform} write={write} act={act} id={id} />
      <NotFinishedSection r={r} nf={marks.notFinished} write={write} act={act} id={id} />
      {attached.length > 0 && (
        <section data-board-drawer-attachments>
          <h3>Attached</h3>
          {attached.map((a) => (
            <div key={a.id} class="board-drawer-attached">
              <AttachmentChip a={a} platform={platform} facts="all" stalled={marks.stalled?.words} waiting={waiting} />
              {write && holdsRow(a) && (
                <button type="button" class="secondary small" data-board-release={a.id} title="Take this agent off the row; it keeps running, and another may take the row" onClick={() => void act(() => boards.detach(id, a.id))}>
                  Release
                </button>
              )}
            </div>
          ))}
        </section>
      )}
      {(r.proof || evidence.length > 0) && (
        <section data-board-drawer-proof>
          <h3>Proof</h3>
          {r.proof && (
            <p>
              {r.proof.by === "ci" ? "Proven by the merged PR, with its checks green." : "Proven by evidence."}
            </p>
          )}
          {evidence.map((a) => (
            <AttachmentChip key={a.id} a={a} platform={platform} />
          ))}
        </section>
      )}
    </SideSheet>
  );
}

/** The drawer's Live session (Arugula iss_c62e00eee3b4b953ff054d2951827de8): the row's work session that needs the reader, else the one that moved last (`s`, which the drawer keeps), with the others as chips to switch to. Its state card says its state, where it runs, and its branch and diffstat; where the page draws panes (`PlatformApi.LivePane`) and the reader steers, its pane is under it, with Hand off to me. Open on its machine goes to its pane.
 *
 * Each session is here once, and keeps the hooks of the sections it came from: `data-board-drawer-sessions` on the section when the row has hosted sessions; on a session's chip, or on the state card when it is the only one, a hosted session's `data-board-session` and an agent block's `data-board-attachment`, `data-attach-role` and `data-board-block` (Attached's); the block's facts and its pane's link keep `data-board-block-facts` and `data-board-block-open`, and Release its `data-board-release`. With no session, where the page reaches machines itself, it says so, and starts none. */
function LiveSession({ r, project, all, s, pick, who, write, act, id }: { r: BoardRow; project: string; all: LiveSession[]; s: LiveSession | undefined; pick: (key: string) => void; who: "steer" | "read"; write: boolean; act: Act; id: string }) {
  const api = usePlatformApi();
  useSubscribe(api.subscribe);
  if (!s) {
    if (!api.LiveAsk || r.lane === "done") return null;
    return (
      <section data-board-drawer-live="">
        <h3>Live session</h3>
        <p class="dim" data-board-live-none>
          No session yet.
        </p>
      </section>
    );
  }
  const attachment = (x: LiveSession) => (x.kind === "block" ? r.attachments.find((a) => a.id === x.key) : undefined);
  // An agent block with no name of its own is named by its pane, as the machine reports it.
  const name = (x: LiveSession) => (x.kind === "block" && !attachment(x)?.block?.title?.trim() && api.paneTitle(x.machine, x.pane)) || x.name;
  const where = s.kind === "hosted" ? "on Arugula cloud" : s.machine ? `on ${api.machine(s.machine)?.name ?? "its machine"}` : null;
  // Its pane: in the app, its machine's page (`goToPane`); control's page on the platform page; in the shell, when the machine is one of the reader's. A hosted session's own page.
  const open =
    s.kind === "hosted"
      ? api.sessionHref && { href: api.sessionHref(project, s.session) }
      : s.machine
        ? api.LiveAsk
          ? { go: () => api.goToPane(s.machine, s.pane, null) }
          : api.controlHref
          ? { href: api.controlHref(`pane=${s.machine}.${s.pane}`) }
          : api.machine(s.machine)
            ? { go: () => api.goToPane(s.machine, s.pane, null) }
            : null
        : null;
  const Pane = who === "steer" && s.kind === "block" && s.machine ? api.LivePane : undefined;
  // The hooks of the sections a session came from: on its chip, or on the card when there are no chips.
  const hooks = (x: LiveSession) => {
    if (x.kind === "hosted") return { "data-board-session": x.session, "data-board-session-live": x.live ? "" : undefined };
    return { "data-board-attachment": x.key, "data-attach-role": "work", "data-board-block": attachment(x)?.block?.state };
  };
  const sessionHref = s.kind === "hosted" ? api.sessionHref?.(project, s.session) : undefined;
  const a = attachment(s);
  return (
    <section data-board-drawer-live={s.key} data-board-drawer-sessions={(r.sessions ?? []).length > 0 ? "" : undefined}>
      <h3>Live session</h3>
      {all.length > 1 && (
        <div class="board-line board-chips" data-board-live-sessions>
          {all.map((x) => (
            <ObjectChip key={x.key} class={x.kind === "hosted" && !x.live ? "dim" : undefined} pressed={x.key === s.key} fact={{ tone: x.tone, text: x.word }} data-board-live-pick={x.key} onClick={() => pick(x.key)} {...hooks(x)}>
              {name(x)}
            </ObjectChip>
          ))}
        </div>
      )}
      <Card tone={s.needs ? "accent" : null} class={s.kind === "hosted" && !s.live ? "board-live-state dim" : "board-live-state"} data-board-live-card={s.kind} {...(all.length > 1 ? {} : hooks(s))}>
        <div class="board-line">
          <StateChip tone={s.tone}>{s.word}</StateChip>
          {sessionHref ? (
            <a class="board-ptr" href={sessionHref} target="_blank" rel="noopener noreferrer">
              {name(s)}
            </a>
          ) : (
            <span class="board-ptr">{name(s)}</span>
          )}
          {where && <span class="dim">{where}</span>}
        </div>
        {s.facts.length > 0 && (
          <div class="board-fact dim" data-board-live-facts data-board-block-facts={s.kind === "block" ? "" : undefined}>
            {s.facts.join(" · ")}
          </div>
        )}
      </Card>
      {Pane && s.kind === "block" && <Pane key={s.key} machine={s.machine} pane={s.pane} />}
      {(open || (write && a && holdsRow(a))) && (
        <div class="board-line">
          {open &&
            ("href" in open ? (
              <a class="secondary small" href={open.href} target="_blank" rel="noopener noreferrer" data-board-live-open data-board-block-open={s.kind === "block" ? "" : undefined}>
                Open on its machine <ExternalIcon size="xs" />
              </a>
            ) : (
              <button type="button" class="secondary small" data-board-live-open data-board-block-open={s.kind === "block" ? "" : undefined} onClick={open.go}>
                Open on its machine
              </button>
            ))}
          {write && a && holdsRow(a) && (
            <button type="button" class="secondary small" data-board-release={a.id} title="Take this agent off the row; it keeps running, and another may take the row" onClick={() => void act(() => boards.detach(id, a.id))}>
              Release
            </button>
          )}
        </div>
      )}
    </section>
  );
}

/** A row's Docs (#517): its spike or research. They move no lane and prove nothing. Writers link a project Doc and take one off. */
function RowDocs({ r, project, platform, write, act, id }: { r: BoardRow; project: string; platform: (path: string) => string | undefined; write: boolean; act: Act; id: string }) {
  const [picking, setPicking] = useState(false);
  const refs = r.docs ?? [];
  if (!refs.length && !write) return null;
  const taken = new Set(refs.flatMap((d) => (d.pointer.kind === "doc" ? [d.pointer.id] : [])));
  return (
    <section data-board-drawer-docs>
      <h3>Docs</h3>
      <div class="board-line board-chips">
        {refs.map((d) => (
          <DocLink key={d.id} d={d} project={project} platform={platform} remove={write ? () => void act(() => boards.removeDoc(id, d.id)) : undefined} />
        ))}
        {write && !picking && (
          <button type="button" class="secondary small" data-board-drawer-doc-add onClick={() => setPicking(true)}>
            <PlusIcon size="xs" />
            Doc
          </button>
        )}
      </div>
      {picking && <DocPicker project={project} taken={taken} pick={(p) => act(() => boards.addDoc(id, p, r.id))} close={() => setPicking(false)} />}
    </section>
  );
}

/** The id of the issue or mirrored item a row is on, if it is on one. */
const itemId = (r: BoardRow): string | undefined => (r.pointer.kind === "issue" || r.pointer.kind === "forge_item" ? r.pointer.id : undefined);

/** A row's item's relations both ways (#403), with add and remove for those who may write. The item at the other end is picked among the board's other rows. A relation Plane or GitHub keeps can't be removed here. */
function Relations({ r, project, others, write, act, id }: { r: BoardRow; project: string; others: BoardRow[]; write: boolean; act: (write: () => Promise<BoardView>) => Promise<boolean>; id: string }) {
  const [kind, setKind] = useState<RelationKind>("blocks");
  const [dir, setDir] = useState<"out" | "in">("out");
  const [to, setTo] = useState("");
  const mine = itemId(r);
  const words = RELATION_KINDS.find((k) => k.kind === kind)!;
  const choices = others.filter((o) => o.id !== r.id && itemId(o));
  /** A relation write, then the board as it is. */
  const then = (write: () => Promise<unknown>) => async () => {
    await write();
    return boards.get(id);
  };
  const add = async () => {
    const other = choices.find((o) => o.id === to);
    if (!mine || !other) return;
    const [from, target] = dir === "out" ? [mine, itemId(other)!] : [itemId(other)!, mine];
    if (await act(then(() => boards.relate(project, { from, to: target, kind })))) setTo("");
  };
  return (
    <section data-board-drawer-relations>
      <h3>Relations</h3>
      {(r.relations ?? []).map((x) => (
        <div key={x.id} class="board-drawer-relation" data-board-relation={x.kind} data-board-relation-source={x.source}>
          <span>{relationLine(x)}</span>
          {x.source !== "person" && x.source !== "agent" && <span class="dim"> · from {x.source === "plane" ? "Plane" : "GitHub"}</span>}
          {x.removable && (
            <IconButton label="Remove relation" class="bar-btn sq board-more" data-board-relation-remove onClick={() => void act(then(() => boards.unrelate(project, x.id)))}>
              <CloseIcon size="xs" />
            </IconButton>
          )}
        </div>
      ))}
      {write && mine && choices.length > 0 && (
        <div class="board-line board-relation-add" data-board-relation-add>
          <select aria-label="This item" value={dir} onChange={(e) => setDir(e.currentTarget.value as "out" | "in")}>
            <option value="out">{words.out}</option>
            {words.in !== words.out && <option value="in">{words.in}</option>}
          </select>
          <select aria-label="Kind" value={kind} onChange={(e) => setKind(e.currentTarget.value as RelationKind)}>
            {RELATION_KINDS.map((k) => (
              <option key={k.kind} value={k.kind}>
                {k.kind.replace("_", " ")}
              </option>
            ))}
          </select>
          <select aria-label="Other item" value={to} onChange={(e) => setTo(e.currentTarget.value)}>
            <option value="">Pick a row…</option>
            {choices.map((o) => (
              <option key={o.id} value={o.id}>
                {isCard(o.live) ? `#${o.live.number} ${o.live.title}` : pointerLabel(o.pointer, o.live)}
              </option>
            ))}
          </select>
          <button type="button" class="btn" disabled={!to} data-board-relation-submit onClick={() => void add()}>
            Add
          </button>
        </div>
      )}
    </section>
  );
}

/** A check as a chip. */
function CheckChip({ tone, word }: { tone: Parameters<typeof StateChip>[0]["tone"]; word: string }) {
  return <StateChip tone={tone}>{word}</StateChip>;
}
