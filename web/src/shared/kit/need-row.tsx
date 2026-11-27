// NeedRow (kit §4.1, 3): the A2P row (an agent asks a person, or waits on
// them), shared by the Overview, the Inbox, a board, a card and Machines.
// It takes the wire's item as it is (kind, from, for, subject, lines,
// answer, escalated; a to-do's how) and answers in place: a question's
// options, a permission's Allow/Deny, a folder's trust, an approval's
// Start/Not now, or a to-do's Done/Can't with a note. Answered, it is a dim
// receipt. It holds at a third of a wide screen or a phone (360–480px):
// put rows in a `NeedList`, and the meta wraps under the title.
//
// The keys (`y`, `x`, `D`) are shown on the buttons; the screen's key
// table binds them, and presses a row's answer by its `data-answer` (the
// option's place: 0 is `y` or `D`, 1 is `x`). `extra` holds what a row
// can do besides answer (Open, where it can't be answered here).

import type { ComponentChildren } from "preact";
import { useState } from "preact/hooks";
import { Button } from "./button";
import { TextField } from "./field";
import { Row } from "./row";
import { Well } from "./well";
import "./kit.css";

export type NeedKind = "ask" | "permission" | "trust" | "approval" | "todo" | (string & {});

export interface NeedOption {
  id: string;
  label: string;
}

/** One item of the A2P wire, as the row draws it. */
export interface Need {
  kind: NeedKind;
  /** The one line the row shows. */
  title: string;
  from: { agent?: string; machine?: string; name?: string; account?: string };
  /** Who it's for: "you", or a teammate's name (neutral, no amber). */
  for: string;
  subject?: { kind: string; id: string; url?: string };
  /** The decision's lines: the question, the command, the folder, the why. */
  lines?: string[];
  answer?: { via: "card" | "todo"; options?: NeedOption[] };
  escalated?: boolean;
  /** A to-do's how: one string with newlines, copied as is. */
  how?: string;
  /** How long ago it asked: "7m". */
  age: string;
}

/** What answered it, for the receipt. */
export interface Receipt {
  what: string;
  by: string;
  at: string;
  note?: string;
}

/** The answers a kind offers when the item names none. */
const DEFAULTS: Record<string, [NeedOption, NeedOption]> = {
  permission: [
    { id: "allow", label: "Allow" },
    { id: "deny", label: "Deny" },
  ],
  trust: [
    { id: "trust", label: "Trust" },
    { id: "not-now", label: "Not now" },
  ],
  approval: [
    { id: "start", label: "Start" },
    { id: "not-now", label: "Not now" },
  ],
  todo: [
    { id: "done", label: "Done" },
    { id: "cant", label: "Can't" },
  ],
};

const KEYS = ["y", "x"];

/** "asks you", "waits on you", "asks Jake". */
export function who(n: Pick<Need, "kind" | "for">): string {
  return `${n.kind === "todo" ? "waits on" : "asks"} ${n.for}`;
}

export function NeedRow({
  need,
  item,
  open,
  selected,
  onToggle,
  onAnswer,
  receipt,
  extra,
}: {
  need: Need;
  /** The item's id, as `data-need-item`, for the screen's keys. */
  item?: string;
  /** The keys' row: a ring, as a picked surface has. */
  selected?: boolean;
  open?: boolean;
  onToggle?: () => void;
  /** The option picked, and a to-do's note. */
  onAnswer?: (option: NeedOption, note?: string) => void;
  receipt?: Receipt;
  extra?: ComponentChildren;
}) {
  const [note, setNote] = useState("");
  const [copied, setCopied] = useState(false);
  const mine = need.for === "you";
  const todo = need.kind === "todo";
  const meta: ComponentChildren[] = [];
  if (receipt) meta.push(`${receipt.what} by ${receipt.by} ${receipt.at}${receipt.note ? ` · ${receipt.note}` : ""}`);
  else meta.push(<span class={mine ? "k-needs" : undefined}>{who(need)}</span>);
  if (need.escalated) meta.push("escalated");
  if (need.subject) meta.push(<span class="m-id">{need.subject.id}</span>);
  meta.push(<span class="m-id">{need.age}</span>);

  const options = need.answer?.options?.length ? need.answer.options : (DEFAULTS[need.kind] ?? []);
  const copy = () => {
    void navigator.clipboard?.writeText(need.how ?? "");
    setCopied(true);
  };
  return (
    <Row
      kind={receipt ? "done" : mine ? "need" : "plain"}
      mark={receipt ? "done" : todo ? "waits" : "asks"}
      label={need.kind}
      title={need.title}
      meta={meta}
      open={receipt ? undefined : open}
      onClick={onToggle}
      data-need={need.kind}
      data-need-item={item}
      data-selected={selected || undefined}
    >
      {need.lines?.map((l, i) =>
        need.kind === "permission" && i === 0 ? (
          <Well key={i} label="command">
            <pre>{l}</pre>
          </Well>
        ) : (
          <p key={i}>{l}</p>
        ),
      )}
      {need.how ? (
        <Well label="how">
          <pre>{need.how}</pre>
          <div class="k-row-actions">
            <Button onClick={copy}>{copied ? "Copied" : "Copy"}</Button>
          </div>
        </Well>
      ) : null}
      {todo ? <TextField label="Note" hint="no secrets" value={note} onInput={setNote} onEnter={() => onAnswer?.(options[0]!, note)} /> : null}
      {(mine && options.length) || extra ? (
        <div class="k-row-actions">
          {mine
            ? options.map((o, i) => (
                <Button key={o.id} kind={i === 0 ? "primary" : "plain"} keys={todo ? (i === 0 ? "D" : undefined) : KEYS[i]} data-answer={i} onClick={() => onAnswer?.(o, todo ? note : undefined)}>
                  {o.label}
                </Button>
              ))
            : null}
          {extra}
        </div>
      ) : null}
    </Row>
  );
}

/** Need rows in a column that may be a third of the screen: under 480px
 * each row's meta wraps under its title. */
export function NeedList({ label, children }: { label: string; children: ComponentChildren }) {
  return (
    <div class="k-need-list" role="group" aria-label={label} data-kit="need-list">
      {children}
    </div>
  );
}
