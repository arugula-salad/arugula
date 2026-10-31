// A forge body (a PR's description, a review, a comment), with GitHub's
// Markdown, block by block: paragraphs, headings, nested and task lists,
// quotes, fenced code, pipe tables and rules. The forms inside a line are
// `inline()`'s, plus links and #N references. Only Preact nodes come out,
// never HTML: a body is written by anyone who can open a PR, so what it
// says stays text (raw HTML shows as it was typed, an image is a link, and
// only http(s) hrefs become links). The one exception is GitHub's folding:
// a `<details>` line, an optional `<summary>…</summary>` and `</details>`
// on lines of their own fold the blocks between them, still drawn as these
// blocks, never as HTML. GitHub's alerts (`> [!NOTE]`, TIP, IMPORTANT,
// WARNING, CAUTION on a quote's first line) are callouts, with an icon and
// a label so the colour isn't all that says which (#254); not in chat. A
// Doc (`embeds`) also draws a ```chart fence as a chart, a ```stats fence
// as tiles (`stats.tsx`) and shows a ```svg fence as a picture, through
// <img> only (`drawing.tsx`); everywhere else those are code. With `image` it draws
// `![alt](assets/<name>)` too (#965): the Doc's own pictures, wherever a line
// of text is (a quote's included), and no other image target.

import type { ComponentChildren } from "preact";
import { useMemo } from "preact/hooks";
import { Chart } from "./chart";
import { Drawing } from "./drawing";
import { bodyLines, readFence } from "./embeds";
import { inline, type ForgeInline, type IsMe } from "./markup";
import { Stats } from "./stats";

const ITEM = /^( *)([-*+]|\d+[.)])\s+(.*)$/;
const FENCE = /^ *(```|~~~)/;
const HEADING = /^ {0,3}(#{1,6})\s+(.*?)(?:\s+#+)?\s*$/;
const RULE = /^ {0,3}(-{3,}|\*{3,}|_{3,})\s*$/;
const QUOTE = /^ {0,3}>/;
const TABLE_SEP = /^\s*\|?\s*:?-+:?\s*(\|\s*:?-+:?\s*)*\|?\s*$/;
const DETAILS = /^ {0,3}<details( open)?>\s*(?:<summary>(.*?)<\/summary>\s*)?$/i;
const DETAILS_END = /^ {0,3}<\/details>\s*$/i;
const SUMMARY = /^ {0,3}<summary>(.*?)<\/summary>\s*$/i;
const ALERT = /^\s*\[!(note|tip|important|warning|caution)\]\s*$/i;

const indentOf = (l: string) => l.length - l.trimStart().length;
const startsBlock = (l: string) => DETAILS.test(l) || DETAILS_END.test(l) || FENCE.test(l) || HEADING.test(l) || RULE.test(l) || QUOTE.test(l) || ITEM.test(l);

/** What the blocks need besides the lines: `refs` for `inline()`,
 * whether a chart or svg fence is more than code, and, for a thread's
 * message, its @mentions as `Markup` marks them. */
type Ctx = ForgeInline & { embeds?: boolean; chat?: ChatKit; isMe?: IsMe; landed?: (token: string) => boolean };

/** What a chat message adds (#109, #105), from `shared/chat/chat-markdown`:
 * the renderer itself loads none of it, so a Doc's share page carries no
 * emoji table, highlighter or storage. */
export interface ChatKit {
  /** What plain text becomes (`:joy:` as 😂). */
  text?: (s: string) => string;
  /** The message is only emoji: draw it large. */
  jumbo: boolean;
  /** A fenced block, with colours and a copy button. */
  Code: (p: { code: string; lang: string }) => ComponentChildren;
}

/** `refs`: where a `#N` goes (`…/issues/`), or null for text (GitLab).
 * `embeds`: charts and drawings (a Doc's; a forge's body is code). `image`:
 * draws a Doc's picture from its target (`assets/x.png`); a new function
 * draws the body again. `chat`: a chat message's kit (#109; `ChatMarkdown`
 * gives it): emoji, code with colours and a copy button, and the tight spacing a one-line message needs (style.css `.md.md-chat`; not `.chat`, which is the chat page's own box). */
export function Markdown({
  text,
  refs,
  embeds,
  image,
  chat,
  isMe,
  landed,
  machine,
}: {
  text: string;
  refs: string | null;
  embeds?: boolean;
  image?: ForgeInline["image"];
  chat?: ChatKit;
  isMe?: IsMe;
  landed?: (token: string) => boolean;
  machine?: ForgeInline["machine"];
}) {
  // A poll that changes nothing hands back the same text: don't parse it again.
  const out = useMemo(
    () => blocks(bodyLines(text), { refs, embeds, image, chat, isMe, landed, machine, text: chat?.text }),
    [text, refs, embeds, image, chat, isMe, landed, machine],
  );
  return <div class={chat ? (chat.jumbo ? "md md-chat md-jumbo" : "md md-chat") : "md"}>{out}</div>;
}

// Nobody is notified by what a body says, so no mention pill, unless it's
// a thread's message (`isMe`).
const line = (s: string, fx: Ctx) => inline(s, fx.isMe, fx.isMe ? fx.landed : () => false, fx);

/** A fenced block from `i`, unclosed ones running to the end. */
function fenced(lines: string[], i: number, strip: number, k: number, fx: Ctx): { node: ComponentChildren; next: number } {
  const f = readFence(lines, i, strip);
  const code = f.code.join("\n");
  if (fx.embeds && f.lang === "chart") return { node: <Chart key={k} source={code} />, next: f.next };
  if (fx.embeds && f.lang === "stats") return { node: <Stats key={k} source={code} />, next: f.next };
  if (fx.embeds && f.lang === "svg") return { node: <Drawing key={k} svg={code} alt={f.alt} />, next: f.next };
  if (fx.chat) return { node: <fx.chat.Code key={k} code={code} lang={f.lang ?? ""} />, next: f.next };
  return { node: <pre key={k}>{code}</pre>, next: f.next };
}

function blocks(lines: string[], fx: Ctx): ComponentChildren[] {
  const out: ComponentChildren[] = [];
  let i = 0;
  let k = 0;
  while (i < lines.length) {
    const l = lines[i];
    if (!l.trim()) {
      i++;
      continue;
    }
    let m: RegExpMatchArray | null;
    if ((m = l.match(DETAILS))) {
      const d = details(lines, i, fx, k++, !!m[1], m[2]);
      out.push(d.node);
      i = d.next;
    } else if (DETAILS_END.test(l)) {
      // A stray close: nothing to fold.
      i++;
    } else if (FENCE.test(l)) {
      const f = fenced(lines, i, indentOf(l), k++, fx);
      out.push(f.node);
      i = f.next;
    } else if ((m = l.match(HEADING))) {
      // Six levels, three sizes.
      const n = Math.min(m[1].length, 3);
      const H = `h${n}` as "h1";
      out.push(<H key={k++}>{line(m[2], fx)}</H>);
      i++;
    } else if (RULE.test(l)) {
      out.push(<hr key={k++} />);
      i++;
    } else if (QUOTE.test(l)) {
      const q: string[] = [];
      while (i < lines.length && QUOTE.test(lines[i])) q.push(lines[i++].replace(/^ {0,3}> ?/, ""));
      // A quote's fences stay code: the daemon checks fences that start a
      // line (lists reach theirs, a quote's lines start with ">").
      const alert = fx.chat ? null : q[0].match(ALERT);
      if (alert) {
        const kind = alert[1].toLowerCase() as Alert;
        out.push(
          <div key={k++} class={`md-callout md-callout-${kind}`} role="note" data-callout={kind}>
            <p class="md-callout-title">
              {glyph(ALERT_ICON[kind])}
              {kind[0].toUpperCase() + kind.slice(1)}
            </p>
            {blocks(q.slice(1), { ...fx, embeds: false })}
          </div>,
        );
      } else out.push(<blockquote key={k++}>{blocks(q, { ...fx, embeds: false })}</blockquote>);
    } else if (l.includes("|") && i + 1 < lines.length && lines[i + 1].includes("-") && TABLE_SEP.test(lines[i + 1])) {
      const head = cells(l);
      i += 2;
      const rows: string[][] = [];
      while (i < lines.length && lines[i].trim() && lines[i].includes("|")) rows.push(cells(lines[i++]));
      out.push(
        <div key={k++} class="md-table">
          <table>
            <thead>
              <tr>
                {head.map((c, j) => (
                  <th key={j}>{line(c, fx)}</th>
                ))}
              </tr>
            </thead>
            <tbody>
              {rows.map((r, j) => (
                <tr key={j}>
                  {head.map((_, c) => (
                    <td key={c}>{line(r[c] ?? "", fx)}</td>
                  ))}
                </tr>
              ))}
            </tbody>
          </table>
        </div>,
      );
    } else if (ITEM.test(l)) {
      const r = list(lines, i, indentOf(l), fx, k++);
      out.push(r.node);
      i = r.next;
    } else {
      // A paragraph: a single newline is a line break, as on GitHub.
      const para: ComponentChildren[] = [];
      do {
        if (para.length) para.push(<br key={`b${i}`} />);
        para.push(...line(lines[i].trim(), fx));
        i++;
      } while (i < lines.length && lines[i].trim() && !startsBlock(lines[i]));
      out.push(<p key={k++}>{para}</p>);
    }
  }
  return out;
}

type Alert = "note" | "tip" | "important" | "warning" | "caution";

/** A callout's 16px glyph, stroked in its tone. */
const glyph = (d: string[]) => (
  <svg class="md-callout-icon" viewBox="0 0 16 16" width="16" height="16" aria-hidden="true" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round">
    {d.map((p) => (
      <path key={p} d={p} />
    ))}
  </svg>
);

const ALERT_ICON: Record<Alert, string[]> = {
  // An i in a circle, a bulb, a speech bubble with a mark, a triangle and an octagon.
  note: ["M8 1.75a6.25 6.25 0 1 0 0 12.5a6.25 6.25 0 1 0 0-12.5", "M8 7.25v4", "M8 4.75v.01"],
  tip: ["M5.5 10.5a4.25 4.25 0 1 1 5 0c-.6.45-.75 1-.75 1.75h-3.5c0-.75-.15-1.3-.75-1.75", "M6.5 14.25h3"],
  important: ["M2 3.25c0-.7.55-1.25 1.25-1.25h9.5c.7 0 1.25.55 1.25 1.25v7c0 .7-.55 1.25-1.25 1.25H7l-3 2.5v-2.5h-.75C2.55 11.5 2 10.95 2 10.25z", "M8 4.5v3", "M8 9.5v.01"],
  warning: ["M7.13 2.5a1 1 0 0 1 1.74 0l5.5 9.75a1 1 0 0 1-.87 1.5h-11a1 1 0 0 1-.87-1.5z", "M8 6v3.25", "M8 11.5v.01"],
  caution: ["M5.4 1.75h5.2l3.65 3.65v5.2l-3.65 3.65H5.4l-3.65-3.65V5.4z", "M8 4.75v3.75", "M8 10.75v.01"],
};

/** A `<details>` from `i` to its `</details>` (nested ones counted;
 * unclosed, to the end), with its `<summary>`, on its line or the next. */
function details(lines: string[], i: number, fx: Ctx, key: number, open: boolean, summary?: string): { node: ComponentChildren; next: number } {
  i++;
  if (summary === undefined) {
    let j = i;
    while (j < lines.length && !lines[j].trim()) j++;
    const s = lines[j]?.match(SUMMARY);
    if (s) {
      summary = s[1];
      i = j + 1;
    }
  }
  const inner: string[] = [];
  let depth = 1;
  for (; i < lines.length; i++) {
    if (FENCE.test(lines[i])) {
      // A fence's lines are code, even ones that look like a fold.
      const f = readFence(lines, i, indentOf(lines[i]));
      inner.push(...lines.slice(i, f.next));
      i = f.next - 1;
      continue;
    }
    if (DETAILS.test(lines[i])) depth++;
    else if (DETAILS_END.test(lines[i]) && --depth === 0) break;
    inner.push(lines[i]);
  }
  return {
    node: (
      <details key={key} open={open}>
        <summary>{summary?.trim() ? line(summary.trim(), fx) : "Details"}</summary>
        {blocks(inner, fx)}
      </details>
    ),
    next: i + 1,
  };
}

/** A table row's cells, without the pipes at its ends. */
function cells(row: string): string[] {
  return row
    .trim()
    .replace(/^\||\|$/g, "")
    .split(/(?<!\\)\|/)
    .map((c) => c.trim().replace(/\\\|/g, "|"));
}

/** A list at `base` indent: its items, their continuation lines, and the
 * lists nested under them. */
function list(lines: string[], i: number, base: number, fx: Ctx, key: number): { node: ComponentChildren; next: number } {
  const first = lines[i].match(ITEM)!;
  const ordered = /\d/.test(first[2]);
  const items: ComponentChildren[] = [];
  while (i < lines.length) {
    const m = lines[i].match(ITEM);
    if (!m || m[1].length !== base || /\d/.test(m[2]) !== ordered) break;
    let text = m[3];
    let task: boolean | null = null;
    const t = text.match(/^\[([ xX])\]\s+(.*)$/);
    if (t) {
      task = t[1] !== " ";
      text = t[2];
    }
    const inner: ComponentChildren[] = [...line(text, fx)];
    let k = 0;
    i++;
    while (i < lines.length) {
      const l = lines[i];
      let afterBlank = false;
      if (!l.trim()) {
        // A blank line keeps the item going only if what follows is indented under it.
        let j = i;
        while (j < lines.length && !lines[j].trim()) j++;
        if (j >= lines.length || indentOf(lines[j]) <= base) break;
        i = j;
        afterBlank = true;
      }
      const cur = lines[i];
      const ind = indentOf(cur);
      if (ind <= base && ITEM.test(cur)) break;
      if (ind <= base && startsBlock(cur)) break;
      if (FENCE.test(cur) && ind > base) {
        const f = fenced(lines, i, ind, k++, fx);
        inner.push(f.node);
        i = f.next;
      } else if (ind > base && ITEM.test(cur)) {
        const r = list(lines, i, ind, fx, k++);
        inner.push(r.node);
        i = r.next;
      } else if (ind <= base && !afterBlank) {
        // A lazy continuation of the item's paragraph.
        inner.push(<br key={k++} />, ...line(cur.trim(), fx));
        i++;
      } else if (ind <= base) {
        break;
      } else {
        inner.push(afterBlank ? <p key={k++}>{line(cur.trim(), fx)}</p> : <br key={k++} />);
        if (!afterBlank) inner.push(...line(cur.trim(), fx));
        i++;
      }
    }
    items.push(
      task === null ? (
        <li key={items.length}>{inner}</li>
      ) : (
        <li key={items.length} class="md-task">
          <input type="checkbox" disabled checked={task} /> {inner}
        </li>
      ),
    );
    // Items with blank lines between them are still one list.
    let j = i;
    while (j < lines.length && !lines[j].trim()) j++;
    const next = lines[j]?.match(ITEM);
    if (j > i && next && next[1].length === base && /\d/.test(next[2]) === ordered) i = j;
  }
  const start = ordered ? parseInt(first[2], 10) : 1;
  return { node: ordered ? <ol key={key} start={start !== 1 ? start : undefined}>{items}</ol> : <ul key={key}>{items}</ul>, next: i };
}
