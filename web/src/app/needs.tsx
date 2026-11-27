// The Inbox's items as kit `NeedRow`s, answered in place (Lift 4, #1480;
// Lift 6d, #1486): what the Overview's Needs you (this project's slice) and
// the Inbox (every project's) share. A row expands in place and answers
// there: an approval or a to-do through the Inbox (`POST /api/inbox/{id}`,
// its `choice` and a to-do's note), a pane's permission or one-question ask
// through its machine, as Swarm's cards answer it. One that can't be
// answered here (a form, a deploy, a board's own ask, a mention) has Open.
// Answered, a row stays as a dim receipt until you leave the page, so the
// list doesn't jump; a to-do's says its note as control kept it, read from
// its card's record (#1462), never from what was typed or a live event.
//
// The keys are the screen's, in the one key table: `j` `k` move (and open
// the row, and peek its asker where the screen has a panel), `e` expands,
// `y` `x` answer, `D` is a to-do's Done, `n` the next thing that needs you,
// `↵` opens it where it lives (through the back stack: `esc` comes back).

import type { ComponentChildren } from "preact";
import { useRef, useState } from "preact/hooks";
import type { ChatAsk, InboxItem } from "../shared/proto";
import { Button } from "../shared/kit/button";
import { NeedRow, type Need, type NeedOption, type Receipt } from "../shared/kit/need-row";
import type { State } from "../shared/kit/state-mark";
import { both, useBindings, type Binding } from "../shared/keys";
import { platformFetch, refusalOf } from "../shared/maintenance";
import { needAge } from "../shared/needs-you";
import type { Ask } from "../shared/ask";
import type { Fleet, FleetPane } from "../shell/fleet";
import { act } from "../shell/swarm/view";
import { permissionTarget } from "../shell/ui/answer-card";
import { forTeammate, markItem, memberName } from "../platform/inbox";
import { go } from "../platform/router";

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

/** `POST /api/inbox/{id}` with a choice (and a to-do's note). */
async function choose(id: string, choice: string, note?: string): Promise<void> {
  const r = await platformFetch(`/api/inbox/${encodeURIComponent(id)}`, {
    method: "POST",
    credentials: "same-origin",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(note?.trim() ? { choice, note: note.trim() } : { choice }),
  });
  if (!r.ok) throw new Error(refusalOf(r.status, (await r.json().catch(() => ({}))) as { error?: string }));
}

/** "3:41 PM". */
const clock = (ms: number) => new Date(ms).toLocaleTimeString([], { hour: "numeric", minute: "2-digit" });

/** An answered to-do's receipt from its record: its card's ask (`GET
 * /api/chat/rooms/{room}/asks/{seq}`, the item's subject), which keeps
 * its note (#1462). None when it can't be read; the receipt then says the
 * answer alone. */
async function todoRecord(i: InboxItem): Promise<Partial<Receipt> | null> {
  const { room, seq } = i.subject;
  if (!room || seq === undefined) return null;
  try {
    const r = await platformFetch(`/api/chat/rooms/${encodeURIComponent(room)}/asks/${seq}`, { credentials: "same-origin" });
    if (!r.ok) return null;
    const p = ((await r.json()) as ChatAsk).proposal;
    if (!p?.todo) return null;
    return {
      ...(p.state === "done" ? { what: "Done" } : p.state === "cant" ? { what: "Can't" } : {}),
      ...(p.todo.answered_at ? { at: clock(p.todo.answered_at) } : {}),
      ...(p.todo.note ? { note: p.todo.note } : {}),
    };
  } catch {
    return null;
  }
}

/** How an item answers here, if it can. */
function answering(fleet: Fleet | null, i: InboxItem): Answering | null {
  if (i.answer && (i.answer.via === "card" || i.answer.via === "todo") && i.answer.options.length)
    return { options: i.answer.options.map((o) => ({ id: o, label: o })), send: (o, note) => choose(i.id, o.id, i.kind === "todo" ? note : undefined) };
  const p = paneOfItem(fleet, i);
  const ask = p?.info.ask as Ask | null | undefined;
  return p && fleet && ask ? paneAnswer(fleet, p, ask) : null;
}

/** Where a row says it is: its card (`#1443`), the chief of staff, a room, or not on a card. */
function placeOf(i: InboxItem, fleet: Fleet | null): string {
  if (i.from.name?.startsWith("Chief of staff")) return "chief of staff";
  if (i.subject.type === "issue" || i.subject.type === "pr" || i.subject.type === "board_row") return /#\d+/.exec(i.title)?.[0] ?? (decides(i) ? i.subject.type : "");
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
  if (!decides(i) && !i.seen_at) void markItem(i.id, { seen: true }).catch(() => {});
  if (i.subject.url.startsWith("/")) go(i.subject.url);
  else window.open(i.subject.url, "_blank", "noopener,noreferrer");
}

/** A list of the Inbox's rows, answered in place, and its keys. `peek` is
 * told of the row the keys move to; `goes`, the footer shows `↵`'s chip
 * (where going there is the point: the Inbox). Give `order` the rows as
 * they're drawn, every render. */
export function useNeeds({ me, fleet, toast, peek, goes }: { me: string | null | undefined; fleet: Fleet | null; toast: (text: string) => void; peek?: (i: InboxItem) => void; goes?: boolean }) {
  // Answered rows stay where they were, as receipts, while the page is up.
  const [receipts, setReceipts] = useState<ReadonlyMap<string, { item: InboxItem; receipt: Receipt }>>(new Map());
  const [open, setOpen] = useState<string | null>(null);
  const [selected, setSelected] = useState<string | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [gone, setGone] = useState<ReadonlySet<string>>(new Set());

  /** The server's list without what was dismissed, with the answered ones kept in their place. */
  const kept = (live: InboxItem[]) => [...live.filter((i) => !receipts.has(i.id) && !gone.has(i.id)), ...[...receipts.values()].map((r) => r.item)].sort((a, b) => a.since - b.since);

  const pick = (i: InboxItem | undefined, expand = true) => {
    if (!i) return;
    setSelected(i.id);
    if (expand) setOpen(i.id);
    peek?.(i);
    requestAnimationFrame(() => document.querySelector(`[data-need-item="${CSS.escape(i.id)}"]`)?.scrollIntoView({ block: "nearest" }));
  };

  async function answer(i: InboxItem, a: Answering, o: NeedOption, note?: string) {
    if (busy) return;
    setBusy(i.id);
    try {
      await a.send(o, note);
      const record = i.kind === "todo" ? await todoRecord(i) : null;
      setReceipts((m) => new Map(m).set(i.id, { item: i, receipt: { what: o.label, by: "you", at: clock(Date.now()), ...record } }));
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
  const now = useRef({ rows: [] as InboxItem[], selected, open, receipts });
  now.current = { ...now.current, selected, open, receipts };
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
    { id: "needs-down", label: "Next row", group: "needs", chip: "move", keys: both("key:j"), hint: { mac: "j k", other: "j k" }, run: () => step(1) },
    { id: "needs-up", label: "Previous row", group: "needs", keys: both("key:k"), run: () => step(-1) },
    {
      id: "needs-expand",
      label: "Expand or collapse the row",
      group: "needs",
      chip: "expand",
      keys: both("key:e"),
      when: () => !!now.current.selected,
      run: () => setOpen((o) => (o === now.current.selected ? null : now.current.selected)),
    },
    { id: "needs-yes", label: "Answer the first way", group: "needs", chip: "answer", keys: both("key:y"), hint: { mac: "y x", other: "y x" }, when: () => !!(now.current.open ?? now.current.selected), run: () => press(0) },
    { id: "needs-no", label: "Answer the second way", group: "needs", keys: both("key:x"), when: () => !!(now.current.open ?? now.current.selected), run: () => press(1) },
    { id: "needs-done", label: "Done, on a to-do", group: "needs", keys: both("Shift+key:d"), hint: { mac: "D", other: "D" }, when: () => find(now.current.selected)?.kind === "todo", run: () => press(0) },
    {
      id: "needs-next",
      label: "Next thing that needs you",
      group: "needs",
      chip: "next",
      keys: both("key:n"),
      run: () => {
        const r = now.current.rows.filter((i) => decides(i) && !forTeammate(i, me) && !now.current.receipts.has(i.id));
        const i = r.findIndex((x) => x.id === now.current.selected);
        pick(r[(i + 1) % Math.max(1, r.length)]);
      },
    },
    {
      id: "needs-open",
      label: "Open it where it is",
      group: "needs",
      ...(goes ? { chip: "go there", hint: { mac: "↵", other: "↵" } } : {}),
      keys: both("Enter"),
      // Not from a button of its own (Copy, an answer), which Enter presses.
      when: () => {
        const a = document.activeElement;
        return !!now.current.selected && (!a || a === document.body || !!a.closest("[data-need-item] > .k-row-line"));
      },
      run: () => {
        const i = find(now.current.selected);
        if (i) openItem(i);
      },
    },
  ];
  useBindings(keys, []);

  /** One item's row. `extra` replaces what it offers besides its answers. */
  const row = (i: InboxItem, extra?: (i: InboxItem, answers: boolean) => ComponentChildren) => {
    const done = receipts.get(i.id);
    const a = done ? null : answering(fleet, i);
    const need = needOf(i, me, fleet, a);
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
            : !a || i.subject.type === "pane"
              ? (
                <Button keys="↵" onClick={() => openItem(i)}>
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
