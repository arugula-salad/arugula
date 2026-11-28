// The Inbox's items as kit `NeedRow`s, answered in place (Lift 4, #1480;
// Lift 6d, #1486): what the Overview's Needs you (this project's slice) and
// the Inbox (every project's) share. A row expands in place and answers
// there: an approval or a to-do through the Inbox (`POST /api/inbox/{id}`,
// its `choice` and a to-do's note), a pane's permission, one-question ask
// or gate (Approve, and Expire for a chant gate, #1546; a decision point's
// choices) through its machine, a board's ask as its board's card does
// (`POST /api/boards/{id}/asks/{ask}/answer`, #1580: its why and the
// recommended option read from its board). One that can't be answered
// here (a form, a deploy, a mention) has Open; a board's ask has it too,
// to its row on its board.
// Answered, a row stays as a dim receipt until you leave the page, so the
// list doesn't jump; a to-do's says its note as control kept it, read from
// its card's record (#1462), never from what was typed or a live event.
//
// The keys are the screen's, in the one key table: `j` `k` move (and open
// the row, and peek its asker where the screen has a panel), `e` expands,
// `y` `x` answer, `D` is a to-do's Done, `n` the next thing that needs you,
// `↵` opens it where it lives (through the back stack: `esc` comes back).

import type { ComponentChildren } from "preact";
import { useEffect, useRef, useState } from "preact/hooks";
import type { BoardRow, ChatAsk, InboxItem } from "../shared/proto";
import { askNeed } from "../shared/boards/text";
import { boards } from "../shared/boards/store";
import { Button } from "../shared/kit/button";
import { NeedRow, type Need, type NeedOption, type Receipt } from "../shared/kit/need-row";
import type { State } from "../shared/kit/state-mark";
import { control } from "../shared/controls";
import { useBindings, type Binding } from "../shared/keys";
import { platformFetch } from "../shared/maintenance";
import { needAge } from "../shared/needs-you";
import type { Ask } from "../shared/ask";
import { clock, todoReceipt } from "../shared/chat/ask-card";
import type { Fleet, FleetPane } from "../shell/fleet";
import { act, kindOf, readableCommand, reasonOf, whatItDoes } from "../shell/fleet-model";
import { pointOf } from "../shell/ui/point-choices";
import type { PanelTarget } from "../shared/kit/panel-key";
import { permissionTarget } from "../shell/ui/answer-card";
import { answerItem, forTeammate, markItem, memberName } from "../platform/inbox";
import { names } from "../platform/links";
import { go, pathOf } from "../platform/router";

/** Kinds that are alerts, not asks (#1393): the Inbox lists them, uncounted; Needs you doesn't. */
export const ALERTS = new Set(["checks_failed", "run_failed", "agent_failed"]);

/** The word a row says of its kind. */
const KIND: Record<string, string> = {
  agent_ask: "ask",
  board_ask: "ask",
  permission: "permission",
  approval: "approval",
  deploy_approval: "approval",
  changes_requested: "changes",
  todo: "todo",
  review_request: "review",
  mention: "mention",
  assigned: "assigned",
  dm: "message",
  thread_news: "thread",
  checks_failed: "check",
  run_failed: "run",
  agent_failed: "agent",
};

/** Needs you's items: the Inbox's decisions, its order. */
export const needsOf = (items: InboxItem[]): InboxItem[] => items.filter((i) => i.group === "decide" && !i.done_at && !ALERTS.has(i.kind));

/** Whose a pane is: null for yours, else a name. A teammate's is theirs; a
 * team's box or a hosted session is its starter's, else the team's name
 * (#1392). */
export function paneOwner(fleet: Fleet, p: FleetPane): string | null {
  if (p.person.kind === "me") return null;
  const name = p.person.kind === "person" && !fleet.host(p.host)?.hosted ? p.person.name : (p.owner ?? p.person.name);
  return name === fleet.me ? null : name;
}

/** Whose a pane is, as the panel and a machine's page treat it: yours, or a
 * team's box or the project's hosted one where you may steer it, to write
 * to; a teammate's own, to watch. */
export function yours(fleet: Fleet, p: FleetPane): boolean {
  if (p.person.kind === "me") return true;
  if (p.person.kind === "team" || fleet.host(p.host)?.hosted) return fleet.role(p) !== "viewer";
  return false;
}

/** A session's name as Arugula names an agent: its agent and title
 * (`Claude Code · #983 cloud half`); anything else, what it runs. */
export function sessionName(p: FleetPane): string {
  const what = whatItDoes(p);
  if (kindOf(p) !== "agent") return what;
  const cmd = p.info.command ?? "";
  const agent = cmd && readableCommand(cmd) !== cmd ? readableCommand(cmd) : "";
  return agent && what !== agent ? `${agent} · ${what}` : what;
}

/** A pane's tab: its session on its machine (by control's id, else its name here). */
export function paneTarget(fleet: Fleet, p: FleetPane, role: "lead" | "worker" | "agent", title?: string): PanelTarget {
  return { kind: "agent", role, machine: fleet.host(p.host)?.id ?? p.host, ...(p.session ? { session: p.session.name } : {}), pane: p.id, title: title ?? sessionName(p) };
}

/** The pane an item is about (a `pane` subject, #1431), if this page has it. */
export function paneOfItem(fleet: Fleet | null, i: InboxItem): FleetPane | undefined {
  const s = i.subject;
  if (!fleet || s.type !== "pane" || !s.machine || !s.pane) return undefined;
  const id = s.pane.replace(/^%/, "");
  return fleet.panes.find((p) => String(p.id) === id && (p.host === s.machine || fleet.host(p.host)?.id === s.machine));
}

/** How a row answers: its options, and what each does. */
interface Answering {
  options: NeedOption[];
  send(o: NeedOption, note?: string): Promise<void>;
}

/** A pane's ask, answered on its machine: a permission's Allow or Deny, a
 * single question's options. A form, or several questions, opens its pane. */
function paneAnswer(fleet: Fleet, p: FleetPane, ask: Ask): Answering | null {
  const run = async (action: "allow" | "deny" | "answer", extra = {}) => {
    const err = await act(fleet, [p], action, { id: ask.id, ...extra });
    if (err) throw new Error(err);
  };
  if (ask.kind === "permission")
    return {
      options: [
        { id: "allow", label: "Allow" },
        { id: "deny", label: "Deny" },
      ],
      send: (o) => run(o.id as "allow" | "deny"),
    };
  const q = ask.kind === "questions" && ask.questions?.length === 1 ? ask.questions[0] : null;
  if (!q || q.multiSelect) return null;
  return { options: q.options.map((o) => ({ id: o.label, label: o.label })), send: (o) => run("answer", { content: { question_0: o.id } }) };
}

/** A pane waiting at a gate, answered on its machine, as the phone's sheet
 * does (#1546): Approve, and Expire where its machine offers it; a decision
 * point (#621), its choices. */
function gateAnswer(fleet: Fleet, p: FleetPane): Answering | null {
  const r = reasonOf(p);
  if (r?.kind !== "gate" || !r.gate) return null;
  const run = async (action: "allow" | "expire" | "answer", extra = {}) => {
    const err = await act(fleet, [p], action, extra);
    if (err) throw new Error(err);
  };
  const point = pointOf(r.gate);
  if (point) return { options: point.choices.map((c) => ({ id: c.value, label: c.label })), send: (o) => run("answer", { content: { answer: o.id } }) };
  return {
    options: [{ id: "allow", label: "Approve" }, ...(r.actions.includes("expire") ? [{ id: "expire", label: "Expire" }] : [])],
    send: (o) => run(o.id as "allow" | "expire"),
  };
}

/** An answered to-do's receipt from its record: its card's ask (`GET
 * /api/chat/rooms/{room}/asks/{seq}`, the item's subject), which keeps
 * its note (#1462). None when it can't be read; the receipt then says the
 * answer alone. */
async function todoRecord(i: InboxItem): Promise<Partial<Receipt> | null> {
  const { room, seq } = i.subject;
  if (!room || seq === undefined) return null;
  try {
    const r = await platformFetch(`/api/chat/rooms/${encodeURIComponent(room)}/asks/${seq}`, { credentials: "same-origin" });
    return r.ok ? todoReceipt((await r.json()) as ChatAsk) : null;
  } catch {
    return null;
  }
}

/** What answering `i` with `what` left: you, now, and a to-do's record. */
export async function receiptOf(i: InboxItem, what: string): Promise<Receipt> {
  const record = i.kind === "todo" ? await todoRecord(i) : null;
  return { what, by: "you", at: clock(Date.now()), ...record };
}

/** The asker's block a to-do names (its subject's machine and `pane`,
 * whatever its type): its pane, where this page has it, else a panel's tab
 * for it, which says its session ended once its machine says it's gone. */
export function askerOf(fleet: Fleet, i: InboxItem): { pane?: FleetPane; target?: PanelTarget } {
  const { machine, pane } = i.subject;
  const id = Number(String(pane ?? "").replace(/^%/, ""));
  if (i.kind !== "todo" || !machine || !pane || !Number.isInteger(id)) return {};
  const p = fleet.panes.find((x) => x.id === id && (x.host === machine || fleet.host(x.host)?.id === machine));
  return p ? { pane: p } : { target: { kind: "agent", role: "agent", machine, pane: id, title: i.from.name ?? i.from.agent ?? "its agent" } };
}

/** How an item answers here, if it can. */
function answering(fleet: Fleet | null, i: InboxItem): Answering | null {
  const board = i.subject.board;
  if (i.answer?.via === "board_answer" && board)
    return { options: i.answer.options.map((o) => ({ id: o, label: o })), send: (o) => boards.answer(board, i.id, o.id ? { option: o.id } : { text: o.label }).then(() => {}) };
  if (i.answer && (i.answer.via === "card" || i.answer.via === "todo") && i.answer.options.length)
    return { options: i.answer.options.map((o) => ({ id: o, label: o })), send: (o, note) => answerItem(i.id, o.id, i.kind === "todo" ? note : undefined) };
  const p = paneOfItem(fleet, i);
  const ask = p?.info.ask as Ask | null | undefined;
  if (!p || !fleet) return null;
  return ask ? paneAnswer(fleet, p, ask) : gateAnswer(fleet, p);
}

/** Where a row says it is: its card (`#1443`), the chief of staff, a room, or not on a card. */
function placeOf(i: InboxItem, fleet: Fleet | null): string {
  if (i.from.name?.startsWith("Chief of staff")) return "chief of staff";
  // Its number where the title has one; else a plain word, never the kind's own name (#1567).
  if (i.subject.type === "issue" || i.subject.type === "pr" || i.subject.type === "board_row")
    return /#\d+/.exec(i.title)?.[0] ?? (decides(i) ? { issue: "issue", pr: "PR", board_row: "" }[i.subject.type] : "");
  if (i.subject.type === "pane") {
    const p = paneOfItem(fleet, i);
    return p?.info.on_issue?.issue ?? "not on a card";
  }
  return i.subject.type === "room" ? "" : (i.from.name ?? "");
}

/** An item that is the reader's to decide: amber, and answered here. */
const decides = (i: InboxItem) => i.group === "decide" && !ALERTS.has(i.kind);

/** The row's words for an item. */
function needOf(i: InboxItem, me: string | null | undefined, fleet: Fleet | null, a: Answering | null): Need {
  const p = paneOfItem(fleet, i);
  const ask = p?.info.ask as Ask | null | undefined;
  const lines = i.lines?.length ? i.lines : ask?.kind === "permission" ? [permissionTarget(ask)] : ask?.questions?.[0]?.question && ask.questions[0].question !== i.title ? [ask.questions[0].question] : i.snippet ? [i.snippet] : [];
  // A to-do's how is its last line with newlines in it (#1467): the well's.
  const how = i.kind === "todo" ? lines.find((l) => l.includes("\n")) : undefined;
  return {
    kind: KIND[i.kind] ?? i.kind,
    title: i.title,
    from: i.from,
    // What isn't a decision (a mention, a review, an alert) is no one's ask: neutral.
    for: forTeammate(i, me) ? memberName(i.for!) : decides(i) ? "you" : "",
    subject: { kind: i.subject.type, id: placeOf(i, fleet), url: i.subject.url },
    lines: lines.filter((l) => l !== how),
    answer: a ? { via: i.kind === "todo" ? "todo" : "card", options: a.options } : undefined,
    escalated: i.escalated,
    how,
    age: needAge(i.since),
  };
}

/** The mark of a row that isn't an ask or a wait: an alert fails; what names you is quiet. */
const markOf = (i: InboxItem): State | undefined => (ALERTS.has(i.kind) ? "failing" : decides(i) ? undefined : "idle");

/** Opens what an item is about, where it lives: a path here (through the
 * back stack, so `esc` comes back), else its page on GitHub. What only
 * names you is seen once opened. */
export function openItem(i: InboxItem) {
  const url = whereOf(i);
  // A teammate's card has no address (Arugula #1469): nothing opens.
  if (!url) return;
  if (!decides(i) && !i.seen_at) void markItem(i.id, { seen: true }).catch(() => {});
  if (url.startsWith("/")) go(url);
  else window.open(url, "_blank", "noopener,noreferrer");
}

/** Where Open goes: a board's ask, its row on its board, where its Needs you shows it open (#1580); anything else, its address. */
function whereOf(i: InboxItem): string {
  const { board, project, id } = i.subject;
  return i.kind === "board_ask" && board && project ? `${pathOf({ page: "board", project, board }, names)}#row=${encodeURIComponent(id)}` : i.subject.url;
}

/** Whether a row offers Open: where it can't be answered here, a pane's (to see it), and a board's ask (its row). */
export const opens = (i: InboxItem, answers: boolean) => (!answers || i.subject.type === "pane" || i.kind === "board_ask") && !!i.subject.url;

/** A tab on this page that an item waits in (a `for_you` tab, Lift 9): its number, and the way there. */
export interface Tabbed {
  n: number;
  go: () => void;
}

/** A list of the Inbox's rows, answered in place, and its keys. `peek` is
 * told of the row the keys move to; `goes`, the footer shows `↵`'s chip
 * (where going there is the point: the Inbox). Give `order` the rows as
 * they're drawn, every render. */
export function useNeeds({ me, fleet, toast, peek, goes, tabOf }: { me: string | null | undefined; fleet: Fleet | null; toast: (text: string) => void; peek?: (i: InboxItem) => void; goes?: boolean; tabOf?: (i: InboxItem) => Tabbed | null }) {
  // Answered rows stay where they were, as receipts, while the page is up.
  const [receipts, setReceipts] = useState<ReadonlyMap<string, { item: InboxItem; receipt: Receipt }>>(new Map());
  const [open, setOpen] = useState<string | null>(null);
  const [selected, setSelected] = useState<string | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [gone, setGone] = useState<ReadonlySet<string>>(new Set());
  // A board ask's row, as its board reads it (by the ask's id): its why and recommendation (#1580).
  const [asked, setAsked] = useState<ReadonlyMap<string, BoardRow>>(new Map());
  const reading = useRef(new Set<string>());

  /** The server's list without what was dismissed, with the answered ones kept in their place. */
  const kept = (live: InboxItem[]) => [...live.filter((i) => !receipts.has(i.id) && !gone.has(i.id)), ...[...receipts.values()].map((r) => r.item)].sort((a, b) => a.since - b.since);

  const pick = (i: InboxItem | undefined, expand = true) => {
    if (!i) return;
    setSelected(i.id);
    if (expand) setOpen(i.id);
    // The page's peek as it is now: the keys are bound once, before its machines are reached.
    now.current.peek?.(i);
    requestAnimationFrame(() => document.querySelector(`[data-need-item="${CSS.escape(i.id)}"]`)?.scrollIntoView({ block: "nearest" }));
  };

  async function answer(i: InboxItem, a: Answering, o: NeedOption, note?: string) {
    if (busy) return;
    setBusy(i.id);
    try {
      await a.send(o, note);
      const receipt = await receiptOf(i, i.kind === "board_ask" ? o.id || o.label : o.label);
      setReceipts((m) => new Map(m).set(i.id, { item: i, receipt }));
      setOpen(null);
    } catch (e) {
      toast(`Couldn't answer it: ${(e as Error).message}`);
    } finally {
      setBusy(null);
    }
  }

  async function dismiss(i: InboxItem) {
    if (busy) return;
    setBusy(i.id);
    try {
      await markItem(i.id, { done: true });
      setGone((s) => new Set([...s, i.id]));
    } catch (e) {
      toast(`Couldn't dismiss it: ${(e as Error).message}`);
    } finally {
      setBusy(null);
    }
  }

  // The keys read the list as it is now.
  const now = useRef({ rows: [] as InboxItem[], selected, open, receipts, peek, tabOf });
  now.current = { ...now.current, selected, open, receipts, peek, tabOf };
  /** Open it: a tab an agent put it in, on this page (Lift 9), else where it is. */
  const goTo = (i: InboxItem) => {
    const t = now.current.tabOf?.(i);
    if (t) t.go();
    else openItem(i);
  };
  const find = (id: string | null) => now.current.rows.find((i) => i.id === id);
  const at = () => now.current.rows.findIndex((i) => i.id === now.current.selected);
  const press = (n: number) => {
    const id = now.current.open ?? now.current.selected;
    if (!id) return;
    document.querySelector<HTMLButtonElement>(`[data-need-item="${CSS.escape(id)}"] [data-answer="${n}"]`)?.click();
  };
  const step = (by: number) => {
    const r = now.current.rows;
    if (!r.length) return;
    const i = at();
    pick(r[i === -1 ? (by > 0 ? 0 : r.length - 1) : Math.min(r.length - 1, Math.max(0, i + by))]);
  };
  const keys: Binding[] = [
    { ...control("needs-down"), run: () => step(1) },
    { ...control("needs-up"), run: () => step(-1) },
    {
      ...control("needs-expand"),
      when: () => !!now.current.selected,
      run: () => setOpen((o) => (o === now.current.selected ? null : now.current.selected)),
    },
    { ...control("needs-yes"), when: () => !!(now.current.open ?? now.current.selected), run: () => press(0) },
    { ...control("needs-no"), when: () => !!(now.current.open ?? now.current.selected), run: () => press(1) },
    { ...control("needs-done"), when: () => find(now.current.selected)?.kind === "todo", run: () => press(0) },
    {
      ...control("needs-next"),
      run: () => {
        const r = now.current.rows.filter((i) => decides(i) && !forTeammate(i, me) && !now.current.receipts.has(i.id));
        const i = r.findIndex((x) => x.id === now.current.selected);
        pick(r[(i + 1) % Math.max(1, r.length)]);
      },
    },
    {
      ...control("needs-open"),
      ...(goes ? {} : { chip: undefined }),
      // Not from a button of its own (Copy, an answer), which Enter presses.
      when: () => {
        const a = document.activeElement;
        return !!now.current.selected && (!a || a === document.body || !!a.closest("[data-need-item] > .k-row-line"));
      },
      run: () => {
        const i = find(now.current.selected);
        if (i) goTo(i);
      },
    },
  ];
  useBindings(keys, []);

  // Each board with an ask not read yet, read once.
  useEffect(() => {
    const unread = now.current.rows.filter((i) => i.kind === "board_ask" && i.subject.board && !reading.current.has(i.id));
    for (const b of new Set(unread.map((i) => i.subject.board!))) {
      for (const i of unread) if (i.subject.board === b) reading.current.add(i.id);
      boards.get(b).then(
        (v) => setAsked((m) => new Map([...m, ...v.zones.flatMap((z) => z.rows.flatMap((r) => (r.ask ? [[r.ask.id, r] as const] : [])))])),
        () => {},
      );
    }
  });

  /** One item's row. `extra` replaces what it offers besides its answers. */
  const row = (i: InboxItem, extra?: (i: InboxItem, answers: boolean) => ComponentChildren) => {
    const done = receipts.get(i.id);
    // A board's ask as its board draws it: its why, and the option it recommends; a teammate's answered here only where its board says you may.
    const r = asked.get(i.id);
    const a = done || (i.kind === "board_ask" && forTeammate(i, me) && !r?.ask?.can_answer) ? null : answering(fleet, i);
    const base = needOf(i, me, fleet, a);
    const ask = r?.ask && askNeed(r.ask, r.live?.number, Date.now());
    const drawn = ask ? { ...base, subject: ask.subject ?? base.subject, lines: ask.lines, answer: a ? ask.answer : undefined } : base;
    // What waits in a tab here says which one (Lift 9).
    const tab = tabOf?.(i);
    const need = tab ? { ...drawn, kind: `tab ${tab.n}` } : drawn;
    const from = i.from.name ?? i.from.agent;
    return (
      <NeedRow
        key={i.id}
        item={i.id}
        need={need}
        mark={markOf(i)}
        meta={decides(i) || done || need.for ? undefined : [from ?? ""].filter(Boolean)}
        open={open === i.id}
        selected={selected === i.id}
        receipt={done?.receipt}
        onToggle={() => {
          setOpen(open === i.id ? null : i.id);
          setSelected(i.id);
          peek?.(i);
        }}
        onAnswer={a ? (o, note) => void answer(i, a, o, note) : undefined}
        data-inbox-item={i.id}
        data-inbox-kind={i.kind}
        extra={
          extra
            ? extra(i, !!a)
            : tab || opens(i, !!a)
              ? (
                <Button keys="↵" onClick={() => goTo(i)}>
                  Open
                </Button>
              )
              : undefined
        }
      />
    );
  };

  return {
    receipts,
    busy,
    kept,
    row,
    dismiss,
    /** The rows the keys move through, in the order they're drawn. */
    order: (rows: InboxItem[]) => void (now.current.rows = rows),
  };
}
