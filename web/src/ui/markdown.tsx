// A forge body (a PR's description, a review, a comment), with GitHub's
// Markdown, block by block: paragraphs, headings, nested and task lists,
// quotes, fenced code, pipe tables and rules. The forms inside a line are
// `inline()`'s, plus links and #N references. Only Preact nodes come out,
// never HTML: a body is written by anyone who can open a PR, so what it
// says stays text (raw HTML shows as it was typed, an image is a link, and
// only http(s) hrefs become links).

import type { ComponentChildren } from "preact";
import { useMemo } from "preact/hooks";
import { inline, type ForgeInline } from "./markup";

const ITEM = /^( *)([-*+]|\d+[.)])\s+(.*)$/;
const FENCE = /^ *(```|~~~)/;
const HEADING = /^ {0,3}(#{1,6})\s+(.*?)(?:\s+#+)?\s*$/;
const RULE = /^ {0,3}(-{3,}|\*{3,}|_{3,})\s*$/;
const QUOTE = /^ {0,3}>/;
const TABLE_SEP = /^\s*\|?\s*:?-+:?\s*(\|\s*:?-+:?\s*)*\|?\s*$/;

const indentOf = (l: string) => l.length - l.trimStart().length;
const startsBlock = (l: string) => FENCE.test(l) || HEADING.test(l) || RULE.test(l) || QUOTE.test(l) || ITEM.test(l);

/** `refs`: where a `#N` goes (`…/issues/`), or null for text (GitLab). */
export function Markdown({ text, refs }: { text: string; refs: string | null }) {
  // A poll that changes nothing hands back the same text: don't parse it again.
  const out = useMemo(() => blocks(text.replace(/\r/g, "").replace(/\t/g, "    ").split("\n"), { refs }), [text, refs]);
  return <div class="md">{out}</div>;
}

// Nobody is notified by what a body says, so no mention pill.
const line = (s: string, fx: ForgeInline) => inline(s, undefined, () => false, fx);

/** A fenced block from `i`, unclosed ones running to the end. */
function fenced(lines: string[], i: number, strip: number, k: number): { node: ComponentChildren; next: number } {
  const mark = lines[i].trimStart().slice(0, 3);
  const code: string[] = [];
  i++;
  for (; i < lines.length && !lines[i].trimStart().startsWith(mark); i++) code.push(lines[i].slice(Math.min(strip, indentOf(lines[i]))));
  return { node: <pre key={k}>{code.join("\n")}</pre>, next: i + 1 };
}

function blocks(lines: string[], fx: ForgeInline): ComponentChildren[] {
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
    if (FENCE.test(l)) {
      const f = fenced(lines, i, indentOf(l), k++);
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
      out.push(<blockquote key={k++}>{blocks(q, fx)}</blockquote>);
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
function list(lines: string[], i: number, base: number, fx: ForgeInline, key: number): { node: ComponentChildren; next: number } {
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
        const f = fenced(lines, i, ind, k++);
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
