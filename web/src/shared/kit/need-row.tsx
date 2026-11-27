// NeedRow (kit §4.1, 3): the A2P row (an agent asks a person, or waits on
// them), shared by the Overview, the Inbox, a board, a card and Machines.
// It takes the wire's item as it is (kind, from, for, subject, lines,
// answer, escalated; a to-do's how) and answers in place: a question's
// options, a permission's Allow/Deny, a folder's trust, an approval's
// Start/Not now, a to-do's Done/Can't with a note, or (a question with no
// options) a typed answer, sent as an option with no id. Answered, it is a dim
// receipt. A to-do's note is checked as it's typed: one that looks like a
// secret says so, and isn't sent until it's gone. A teammate's row is
// neutral: no tick, and its mark is dim, not amber (Arugula #1469). It holds
// at a third of a wide screen or a phone (360–480px): put rows in a
// `NeedList`, and the meta wraps under the title.
//
// The keys (`y`, `x`, `D`) are shown on the buttons; the screen's key
// table binds them, and presses a row's answer by its `data-answer` (the
// option's place: 0 is `y` or `D`, 1 is `x`). `extra` holds what a row
// can do besides answer (Open, where it can't be answered here), and
// `children` what a place shows under its lines (a diff, a gate's why).
//
// An agent's own question (M6c) is a row too, given as its `ask`: Claude's
// AskUserQuestion (one or several questions; buttons for one answer,
// checkboxes for several, an "Other" box, option previews in mono), any
// other form drawn from its JSON Schema, or a sign-in link. Its answer is
// the form's fields, as the daemon reads them: `question_<n>` (a label, or
// a list of them) and `question_<n>_custom`, or the schema's own keys.

import type { ComponentChildren } from "preact";
import { useState } from "preact/hooks";
import { headline, type Ask, type AskActions, type Question, type Schema } from "../ask";
import { looksSecret } from "../secret-like";
import { webLink } from "../web-link";
import { Button } from "./button";
import { TextField } from "./field";
import { Row } from "./row";
import type { State } from "./state-mark";
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

/** An agent's question as a row's words: its headline, for you. */
export function askNeed(ask: Ask, age: string): Need {
  return { kind: ask.kind === "url" ? "sign-in" : ask.kind === "form" ? "form" : "ask", title: headline(ask), from: { agent: ask.agent }, for: "you", age };
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
  ask,
  actions,
  meta: metaOf,
  mark,
  children,
  inert,
  ...data
}: {
  need: Need;
  /** Held in place while it goes (a list under the pointer): seen, not pressed. */
  inert?: boolean;
  /** An agent's question (or form, or link): drawn in the body and answered there, through `actions`. */
  ask?: Ask;
  actions?: AskActions;
  /** What the line says after the title, instead of who it's for (a place's own words: where, how many); the age still ends it. */
  meta?: ComponentChildren[];
  /** The mark, where the row's kind isn't an ask or a wait (a failure, a finish). */
  mark?: State;
  /** Under the lines, before the answers: what a place shows of it (a diff, a gate's why). */
  children?: ComponentChildren;
  [data: `data-${string}`]: unknown;
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
  if (metaOf) meta.push(...metaOf);
  else if (receipt) meta.push(`${receipt.what} by ${receipt.by} ${receipt.at}${receipt.note ? ` · ${receipt.note}` : ""}`);
  else meta.push(<span class={mine ? "k-needs" : undefined}>{who(need)}</span>);
  if (need.escalated) meta.push("escalated");
  if (need.subject) meta.push(<span class="m-id">{need.subject.id}</span>);
  if (need.age) meta.push(<span class="m-id">{need.age}</span>);

  // The note's check, as it's typed: what it looks like, if a secret.
  const secret = todo ? looksSecret(note) : null;
  const options = need.answer?.options?.length ? need.answer.options : (DEFAULTS[need.kind] ?? []);
  const copy = () => {
    void navigator.clipboard?.writeText(need.how ?? "");
    setCopied(true);
  };
  return (
    <Row
      kind={receipt ? "done" : mine ? "need" : "plain"}
      mark={receipt ? "done" : (mark ?? (todo ? "waits" : "asks"))}
      label={need.kind}
      title={need.title}
      meta={meta}
      open={receipt && !children ? undefined : open}
      onClick={onToggle}
      data-need={need.kind}
      data-need-for={mine ? "you" : need.for}
      data-need-item={item}
      data-selected={selected || undefined}
      data-ask={ask?.id}
      inert={inert}
      {...data}
    >
      {need.lines?.map((l, i) =>
        need.kind === "permission" && i === 0 ? (
          <Well key={i} label="command">
            <pre>{l}</pre>
          </Well>
        ) : (
          <p key={i} data-need-line={i}>
            {l}
          </p>
        ),
      )}
      {children}
      {ask && actions ? <AskBody ask={ask} actions={actions} more={extra} /> : null}
      {need.how ? (
        <Well label="how">
          <pre>{need.how}</pre>
          <div class="k-row-actions">
            <Button onClick={copy}>{copied ? "Copied" : "Copy"}</Button>
          </div>
        </Well>
      ) : null}
      {todo ? (
        <TextField
          label="Note"
          hint="no secrets"
          warn={secret ? `That looks like ${secret}: leave it out` : undefined}
          value={note}
          onInput={setNote}
          onEnter={() => !secret && onAnswer?.(options[0]!, note)}
        />
      ) : null}
      {!ask && mine && !todo && !options.length ? (
        <>
          <TextField label="Your answer" value={note} onInput={setNote} onEnter={() => note.trim() && onAnswer?.({ id: "", label: note.trim() })} />
          <div class="k-row-actions">
            <Button kind="primary" disabled={!note.trim()} onClick={() => onAnswer?.({ id: "", label: note.trim() })}>
              Send
            </Button>
          </div>
        </>
      ) : null}
      {!ask && ((mine && options.length) || extra) ? (
        <div class="k-row-actions">
          {mine
            ? options.map((o, i) => (
                <Button
                  key={o.id}
                  kind={i === 0 ? "primary" : "plain"}
                  keys={todo ? (i === 0 ? "D" : undefined) : KEYS[i]}
                  data-answer={i}
                  disabled={!!secret}
                  onClick={() => onAnswer?.(o, todo ? note : undefined)}
                >
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
export function NeedList({ label, class: place, children }: { label: string; /** Its place on the page (a rail's strip on a phone). */ class?: string; children: ComponentChildren }) {
  return (
    <div class={place ? `k-need-list ${place}` : "k-need-list"} role="group" aria-label={label} data-kit="need-list">
      {children}
    </div>
  );
}

// ---- An agent's question, form or sign-in link, in a row's body.

/** The body of an `ask` row (its row is `data-ask`): a dialog named by its headline. */
function AskBody({ ask, actions, more }: { ask: Ask; actions: AskActions; more?: ComponentChildren }) {
  return (
    <div class="k-ask" role="dialog" aria-label={headline(ask)}>
      {ask.kind === "questions" && ask.questions ? (
        <Questions ask={ask} questions={ask.questions} actions={actions} more={more} />
      ) : ask.kind === "url" ? (
        <SignIn ask={ask} actions={actions} more={more} />
      ) : (
        <SchemaForm ask={ask} actions={actions} more={more} />
      )}
    </div>
  );
}

/** Its answers: Submit, Skip (or the place's words for them), Answer in terminal, Stop, and the place's own. */
function AskButtons({ actions, submit, disabled, labels, more }: { actions: AskActions; submit?: () => void; disabled?: boolean; labels?: { submit: string; decline: string }; more?: ComponentChildren }) {
  return (
    <div class="k-row-actions">
      {submit && (
        <Button kind="primary" data-ask-submit disabled={disabled} onClick={submit}>
          {labels?.submit ?? "Submit"}
        </Button>
      )}
      <Button data-ask-decline onClick={() => actions.decline()}>
        {labels?.decline ?? "Skip"}
      </Button>
      {actions.terminal && (
        <Button title="Claude Code shows its own picker" onClick={() => actions.terminal!()}>
          Answer in terminal
        </Button>
      )}
      {actions.stop && (
        <Button kind="danger" onClick={() => actions.stop!()}>
          Stop
        </Button>
      )}
      {more}
    </div>
  );
}

/** One choice: a radio of one (a button) or a checkbox of several, its label and what it means. */
function Option({ label, description, on, many, pick, show }: { label: string; description?: string; on: boolean; many?: boolean; pick: () => void; show?: () => void }) {
  const words = (
    <>
      <span class="k-ask-label">{label}</span>
      {description && <span class="k-ask-desc">{description}</span>}
    </>
  );
  return many ? (
    <label class="k-ask-option" onFocusIn={show} onPointerDown={show}>
      <input type="checkbox" checked={on} onChange={pick} />
      {words}
    </label>
  ) : (
    <button
      type="button"
      role="radio"
      aria-checked={on}
      class="k-ask-option"
      onFocus={show}
      onClick={() => {
        show?.();
        pick();
      }}
    >
      {words}
    </button>
  );
}

/** Claude's AskUserQuestion: each question its options, an Other box, and a preview of the option picked. */
function Questions({ ask, questions, actions, more }: { ask: Ask; questions: Question[]; actions: AskActions; more?: ComponentChildren }) {
  const [picks, setPicks] = useState<string[][]>(() => questions.map(() => []));
  const [other, setOther] = useState<string[]>(() => questions.map(() => ""));
  const [preview, setPreview] = useState<{ n: number; text: string } | null>(null);
  const toggle = (n: number, q: Question, label: string) => {
    const cur = picks[n]!;
    const next = q.multiSelect ? (cur.includes(label) ? cur.filter((l) => l !== label) : [...cur, label]) : cur[0] === label ? [] : [label];
    setPicks(picks.map((p, i) => (i === n ? next : p)));
  };
  const answered = questions.some((_, n) => picks[n]!.length > 0 || other[n]!.trim() !== "");
  const submit = () => {
    const content: Record<string, unknown> = {};
    questions.forEach((q, n) => {
      if (picks[n]!.length) content[`question_${n}`] = q.multiSelect ? picks[n] : picks[n]![0];
      if (other[n]!.trim()) content[`question_${n}_custom`] = other[n]!.trim();
    });
    actions.answer(content);
  };
  // One question is the row's title: its box says only how to answer it.
  const one = questions.length === 1;
  return (
    <>
      {!one && ask.message && <p data-ask-message>{ask.message}</p>}
      {questions.map((q, n) => (
        <fieldset key={n} class="k-ask-q" data-question={n}>
          {(!one || q.multiSelect) && (
            <legend>
              {!one && (
                <span data-ask-text title={q.header}>
                  {q.question}
                </span>
              )}
              {q.multiSelect && <span class="k-ask-hint m-meta">any of these</span>}
            </legend>
          )}
          <div class="k-ask-options" role={q.multiSelect ? "group" : "radiogroup"} aria-label={q.question}>
            {q.options.map((o) => (
              <Option
                key={o.label}
                label={o.label}
                description={o.description}
                on={picks[n]!.includes(o.label)}
                many={q.multiSelect}
                pick={() => toggle(n, q, o.label)}
                show={o.preview ? () => setPreview({ n, text: o.preview! }) : undefined}
              />
            ))}
          </div>
          {preview?.n === n && (
            <Well label="preview">
              <pre data-ask-preview>{preview.text}</pre>
            </Well>
          )}
          <span class="k-field-box">
            <input
              type="text"
              data-ask-other
              aria-label={`Other: ${q.question}`}
              placeholder={q.multiSelect ? "Other (added to your picks)" : picks[n]!.length ? "A note on your answer (optional)" : "Other"}
              value={other[n]}
              onInput={(e) => {
                const v = e.currentTarget.value;
                setOther(other.map((x, i) => (i === n ? v : x)));
              }}
              onKeyDown={(e) => e.key === "Enter" && answered && submit()}
            />
          </span>
        </fieldset>
      ))}
      <AskButtons actions={actions} submit={submit} disabled={!answered} more={more} />
    </>
  );
}

interface Choice {
  value: string | number;
  label: string;
  description?: string;
}

/** A field's choices: `oneOf`/`anyOf` consts, or `enum` with `enumNames`. */
function choices(f: Schema | undefined): Choice[] | null {
  if (!f) return null;
  const consts = f.oneOf ?? f.anyOf;
  if (consts) return consts.map((c) => ({ value: c.const, label: c.title ?? String(c.const), description: c.description }));
  if (f.enum) return f.enum.map((v, i) => ({ value: v, label: f.enumNames?.[i] ?? String(v) }));
  return null;
}

/** Any other form, from its JSON Schema: choices, checkboxes, numbers, text. */
function SchemaForm({ ask, actions, more }: { ask: Ask; actions: AskActions; more?: ComponentChildren }) {
  const required = new Set(ask.schema?.required ?? []);
  // The daemon's JSON doesn't keep the server's field order (keys come
  // sorted), so what's required comes first.
  const props = Object.entries(ask.schema?.properties ?? {}).sort(([a], [b]) => Number(required.has(b)) - Number(required.has(a)));
  const [values, setValues] = useState<Record<string, unknown>>(() => {
    const v: Record<string, unknown> = {};
    for (const [k, f] of props) if (f.default !== undefined) v[k] = f.default;
    return v;
  });
  const set = (k: string, v: unknown) => setValues({ ...values, [k]: v });
  const filled = (v: unknown) => v !== undefined && v !== "" && !(Array.isArray(v) && v.length === 0);
  const ready = [...required].every((k) => filled(values[k]) || ask.schema?.properties?.[k]?.type === "boolean");
  const submit = () => {
    const content: Record<string, unknown> = {};
    for (const [k, v] of Object.entries(values)) if (filled(v)) content[k] = v;
    actions.answer(content);
  };
  return (
    <>
      {ask.message !== headline(ask) || props.length === 0 ? <p data-ask-message>{ask.message}</p> : null}
      {/* #1067: a Doc's share card says what sharing means, under its question. */}
      {ask.source === "doc" && ask.schema?.description && <p class="k-ask-desc">{ask.schema.description}</p>}
      {props.map(([key, f]) => {
        const label = (
          <span class="k-ask-label">
            {f.title ?? key}
            {required.has(key) && <span class="k-needs"> *</span>}
          </span>
        );
        const desc = f.description && <span class="k-ask-desc">{f.description}</span>;
        const many = f.type === "array" ? choices(f.items) : null;
        const opts = choices(f);
        if (many || opts) {
          const cur = (values[key] as (string | number)[] | undefined) ?? [];
          return (
            <fieldset key={key} class="k-ask-q" data-field={key}>
              <legend>{label}</legend>
              {desc}
              <div class="k-ask-options" role={many ? "group" : "radiogroup"}>
                {(many ?? opts)!.map((c) => (
                  <Option
                    key={String(c.value)}
                    label={c.label}
                    description={c.description}
                    many={!!many}
                    on={many ? cur.includes(c.value) : values[key] === c.value}
                    pick={() => (many ? set(key, cur.includes(c.value) ? cur.filter((x) => x !== c.value) : [...cur, c.value]) : set(key, values[key] === c.value ? undefined : c.value))}
                  />
                ))}
              </div>
            </fieldset>
          );
        }
        if (f.type === "boolean") {
          return (
            <label key={key} class="k-ask-option is-check" data-field={key}>
              <input type="checkbox" checked={values[key] === true} onChange={(e) => set(key, e.currentTarget.checked)} />
              {label}
              {desc}
            </label>
          );
        }
        const value = values[key] === undefined ? "" : String(values[key]);
        const long = f.format === "markdown" || (f.maxLength ?? 0) > 200;
        return (
          <label key={key} class="k-field" data-field={key}>
            {label}
            {desc}
            <span class={long ? "k-field-box is-long" : "k-field-box"}>
              {f.type === "integer" || f.type === "number" ? (
                <input
                  type="number"
                  name={key}
                  min={f.minimum}
                  max={f.maximum}
                  step={f.type === "integer" ? 1 : undefined}
                  value={value}
                  onInput={(e) => {
                    const raw = e.currentTarget.value;
                    set(key, raw === "" ? undefined : Number(raw));
                  }}
                />
              ) : long ? (
                <textarea name={key} rows={6} value={value} onInput={(e) => set(key, e.currentTarget.value)} />
              ) : (
                <input type="text" name={key} value={value} onInput={(e) => set(key, e.currentTarget.value)} />
              )}
            </span>
          </label>
        );
      })}
      {/* M36: a forge draft is sent (with your login) or discarded. #234: an
          agent's invite is sent, or declined with the reason you gave. */}
      <AskButtons
        actions={ask.source === "invite" ? { ...actions, decline: () => actions.answer({ decline: true, ...(filled(values.reason) ? { reason: values.reason } : {}) }) } : actions}
        submit={submit}
        disabled={!ready}
        more={more}
        labels={
          ask.source === "forge"
            ? { submit: "Send", decline: "Discard" }
            : ask.source === "invite"
              ? { submit: "Invite", decline: "Decline" }
              : ask.source === "doc"
                ? { submit: "Share", decline: "Not now" }
                : undefined
        }
      />
    </>
  );
}

/** A sign-in link: open it (and wait for it to finish), or dismiss it. */
function SignIn({ ask, actions, more }: { ask: Ask; actions: AskActions; more?: ComponentChildren }) {
  const url = webLink(ask.url);
  return (
    <>
      {ask.message !== headline(ask) && <p data-ask-message>{ask.message}</p>}
      {url && (
        <p class="m-code" data-ask-url>
          {url}
        </p>
      )}
      <div class="k-row-actions">
        {ask.accepted ? (
          <span class="m-meta">Waiting for it to finish…</span>
        ) : (
          <Button
            kind="primary"
            onClick={() => {
              if (url) window.open(url, "_blank", "noopener");
              actions.answer({});
            }}
          >
            Open link
          </Button>
        )}
        <Button onClick={() => actions.decline()}>Dismiss</Button>
        {more}
      </div>
    </>
  );
}
