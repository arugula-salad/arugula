// The Overview's right third, above the agent panel (Lift 4, #1480): Needs
// you, this project's slice of the Inbox (`by_project`'s items), then To do,
// then a teammate's asks folded into one line each ("2 asks for Jake ›").
// Each is a kit `NeedRow` that expands in place and answers there: an
// approval or a to-do through the Inbox (`POST /api/inbox/{id}`, its
// `choice` and a to-do's note), a pane's permission or one-question ask
// through its machine, as Swarm's cards answer it. One that can't be
// answered here (a form, a deploy, a board's own ask) has Open. Answered, a
// row stays as a dim receipt until you leave the page, so the list doesn't
// jump. Alerts (checks or a run failed) aren't here: they're the board's and
// Running's.
//
// Its keys are the Overview's, in the one key table: `j` `k` move (and
// open the row, and peek its asker), `e` expands, `y` `x` answer, `D` is a
// to-do's Done, `n` the next thing that needs you (asks, then To do), `↵`
// opens it where it lives.

import { useRef, useState } from "preact/hooks";
import type { InboxItem } from "../shared/proto";
import { Button } from "../shared/kit/button";
import { ChevronIcon } from "../shared/ui/icons";
import { NeedList, NeedRow, type Need, type NeedOption, type Receipt } from "../shared/kit/need-row";
import { Row } from "../shared/kit/row";
import { Surface } from "../shared/kit/surface";
import { both, useBindings, type Binding } from "../shared/keys";
import { platformFetch, refusalOf } from "../shared/maintenance";
import { needAge } from "../shared/needs-you";
import type { Ask } from "../shared/ask";
import type { Fleet, FleetPane } from "../shell/fleet";
import { act } from "../shell/swarm/view";
import { permissionTarget } from "../shell/ui/answer-card";
import { asksFor, byTeammate, forTeammate, memberName } from "../platform/inbox-count";
import { go } from "../platform/router";

/** Kinds that aren't Needs you: an alert is the board's and Running's (#1393). */
const ALERTS = new Set(["checks_failed", "run_failed", "agent_failed"]);

/** The word a row says of its kind. */
const KIND: Record<string, string> = {
  agent_ask: "ask",
  board_ask: "ask",
  permission: "permission",
  approval: "approval",
  deploy_approval: "approval",
  changes_requested: "changes",
  todo: "todo",
};

/** The rows past which the list folds ("N more ›"), so the panel keeps its room. */
const SHOWN = 5;

/** Needs you's items: the Inbox's decisions in this project, its order. */
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
export function paneOfItem(fleet: Fleet, i: InboxItem): FleetPane | undefined {
  const s = i.subject;
  if (s.type !== "pane" || !s.machine || !s.pane) return undefined;
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

/** How an item answers here, if it can. */
function answering(fleet: Fleet, i: InboxItem): Answering | null {
  if (i.answer && (i.answer.via === "card" || i.answer.via === "todo") && i.answer.options.length)
    return { options: i.answer.options.map((o) => ({ id: o, label: o })), send: (o, note) => choose(i.id, o.id, i.kind === "todo" ? note : undefined) };
  const p = paneOfItem(fleet, i);
  const ask = p?.info.ask as Ask | null | undefined;
  return p && ask ? paneAnswer(fleet, p, ask) : null;
}

/** Where a row says it is: its card (`#1443`), the chief of staff, or not on a card. */
function placeOf(i: InboxItem, fleet: Fleet): string {
  if (i.from.name?.startsWith("Chief of staff")) return "chief of staff";
  if (i.subject.type === "issue" || i.subject.type === "pr" || i.subject.type === "board_row") return /#\d+/.exec(i.title)?.[0] ?? i.subject.type;
  const p = i.subject.type === "pane" ? paneOfItem(fleet, i) : undefined;
  if (p) return p.info.on_issue?.issue ?? "not on a card";
  return i.from.name ?? "";
}

/** The row's words for an item. */
function needOf(i: InboxItem, me: string | null | undefined, fleet: Fleet, a: Answering | null): Need {
  const p = paneOfItem(fleet, i);
  const ask = p?.info.ask as Ask | null | undefined;
  const lines = i.lines?.length ? i.lines : ask?.kind === "permission" ? [permissionTarget(ask)] : ask?.questions?.[0]?.question && ask.questions[0].question !== i.title ? [ask.questions[0].question] : [];
  // A to-do's how is its last line with newlines in it (#1467): the well's.
  const how = i.kind === "todo" ? lines.find((l) => l.includes("\n")) : undefined;
  return {
    kind: KIND[i.kind] ?? i.kind,
    title: i.title,
    from: i.from,
    for: forTeammate(i, me) ? memberName(i.for!) : "you",
    subject: { kind: i.subject.type, id: placeOf(i, fleet), url: i.subject.url },
    lines: lines.filter((l) => l !== how),
    answer: a ? { via: i.kind === "todo" ? "todo" : "card", options: a.options } : undefined,
    escalated: i.escalated,
    how,
    age: needAge(i.since),
  };
}

/** Opens what an item is about, where it lives: a path here, else its page on GitHub. */
export const openItem = (i: InboxItem) => (i.subject.url.startsWith("/") ? go(i.subject.url) : window.open(i.subject.url, "_blank", "noopener,noreferrer"));

export function OverviewNeeds({ items, me, fleet, toast, peek }: { items: InboxItem[]; me: string | null | undefined; fleet: Fleet; toast: (text: string) => void; peek: (i: InboxItem) => void }) {
  // Answered rows stay where they were, as receipts, while the page is up.
  const [receipts, setReceipts] = useState<ReadonlyMap<string, { item: InboxItem; receipt: Receipt }>>(new Map());
  const [open, setOpen] = useState<string | null>(null);
  const [selected, setSelected] = useState<string | null>(null);
  const [unfolded, setUnfolded] = useState<ReadonlySet<string>>(new Set());
  const [more, setMore] = useState(false);
  const [busy, setBusy] = useState<string | null>(null);

  // The server's list, with the answered ones kept in their place.
  const live = needsOf(items);
  const all = [...live.filter((i) => !receipts.has(i.id)), ...[...receipts.values()].map((r) => r.item)].sort((a, b) => a.since - b.since);
  const mine = all.filter((i) => !forTeammate(i, me));
  const asks = mine.filter((i) => i.kind !== "todo");
  const todos = mine.filter((i) => i.kind === "todo");
  const theirs = byTeammate(all, me);
  const shownAsks = more ? asks : asks.slice(0, SHOWN);
  const folded = asks.length - shownAsks.length;
  // The rows the keys move through, in the order they're drawn.
  const rows = [...shownAsks, ...todos, ...theirs.flatMap(([who, list]) => (unfolded.has(who) ? list : []))];
  const open1 = (i: InboxItem | undefined, expand = true) => {
    if (!i) return;
    setSelected(i.id);
    if (expand) setOpen(i.id);
    peek(i);
    requestAnimationFrame(() => document.querySelector(`[data-need-item="${CSS.escape(i.id)}"]`)?.scrollIntoView({ block: "nearest" }));
  };

  async function answer(i: InboxItem, a: Answering, o: NeedOption, note?: string) {
    if (busy) return;
    setBusy(i.id);
    try {
      await a.send(o, note);
      setReceipts((m) => new Map(m).set(i.id, { item: i, receipt: { what: o.label, by: "you", at: new Date().toLocaleTimeString([], { hour: "numeric", minute: "2-digit" }), ...(note?.trim() ? { note: note.trim() } : {}) } }));
      setOpen(null);
    } catch (e) {
      toast(`Couldn't answer it: ${(e as Error).message}`);
    } finally {
      setBusy(null);
    }
  }

  // The keys read the list as it is now.
  const now = useRef({ rows, selected, open, all, receipts });
  now.current = { rows, selected, open, all, receipts };
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
    open1(r[i === -1 ? (by > 0 ? 0 : r.length - 1) : Math.min(r.length - 1, Math.max(0, i + by))]);
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
    { id: "needs-done", label: "Done, on a to-do", group: "needs", keys: both("Shift+key:d"), hint: { mac: "D", other: "D" }, when: () => now.current.all.find((i) => i.id === now.current.selected)?.kind === "todo", run: () => press(0) },
    {
      id: "needs-next",
      label: "Next thing that needs you",
      group: "needs",
      chip: "next",
      keys: both("key:n"),
      run: () => {
        const r = now.current.rows.filter((i) => !forTeammate(i, me) && !now.current.receipts.has(i.id));
        const i = r.findIndex((x) => x.id === now.current.selected);
        open1(r[(i + 1) % Math.max(1, r.length)]);
      },
    },
    {
      id: "needs-open",
      label: "Open it where it is",
      group: "needs",
      keys: both("Enter"),
      // Not from a button of its own (Copy, an answer), which Enter presses.
      when: () => {
        const a = document.activeElement;
        return !!now.current.selected && (!a || a === document.body || !!a.closest("[data-need-item] > .k-row-line"));
      },
      run: () => {
        const i = now.current.all.find((x) => x.id === now.current.selected);
        if (i) openItem(i);
      },
    },
  ];
  useBindings(keys, []);

  const row = (i: InboxItem) => {
    const kept = receipts.get(i.id);
    const a = kept ? null : answering(fleet, i);
    const need = needOf(i, me, fleet, a);
    return (
      <NeedRow
        key={i.id}
        item={i.id}
        need={need}
        open={open === i.id}
        selected={selected === i.id}
        receipt={kept?.receipt}
        onToggle={() => {
          setOpen(open === i.id ? null : i.id);
          setSelected(i.id);
          peek(i);
        }}
        onAnswer={a ? (o, note) => void answer(i, a, o, note) : undefined}
        extra={
          !a || i.subject.type === "pane" ? (
            <Button keys="↵" onClick={() => openItem(i)}>
              Open
            </Button>
          ) : undefined
        }
      />
    );
  };

  const count = live.filter((i) => i.counts && !forTeammate(i, me) && i.kind !== "todo").length;
  const oldest = needAge(asks.find((i) => !receipts.has(i.id))?.since);
  return (
    <div class="ov-needs" data-ov-needs>
      <Surface kind="section" as="section" aria-label="Needs you" data-ov-needs-asks>
        <header class="ov-head">
          <h2>Needs you</h2>
          <span class="m-meta" data-ov-needs-count>
            {count || ""}
          </span>
          {oldest && <span class="ov-head-end m-meta">oldest {oldest}</span>}
        </header>
        {asks.length === 0 ? (
          <p class="ov-quiet" data-ov-needs-none>
            Nothing needs you.
          </p>
        ) : (
          <NeedList label="Needs you">{shownAsks.map(row)}</NeedList>
        )}
        {folded > 0 && (
          <button type="button" class="ov-more" data-ov-needs-more onClick={() => setMore(true)}>
            {folded} more <ChevronIcon dir="right" size="xs" />
          </button>
        )}
        {todos.length > 0 && (
          <>
            <header class="ov-head" data-ov-todo>
              <h2>To do</h2>
              <span class="m-meta">{todos.filter((i) => !receipts.has(i.id)).length || ""}</span>
            </header>
            <NeedList label="To do">{todos.map(row)}</NeedList>
          </>
        )}
        {theirs.map(([who, list]) => (
          <div key={who} data-ov-asks-for={who}>
            <Row
              label=""
              title={asksFor(list.length, memberName(who))}
              open={unfolded.has(who)}
              onClick={() => setUnfolded((s) => (s.has(who) ? new Set([...s].filter((x) => x !== who)) : new Set([...s, who])))}
            >
              <NeedList label={`For ${memberName(who)}`}>{list.map(row)}</NeedList>
            </Row>
          </div>
        ))}
      </Surface>
    </div>
  );
}
