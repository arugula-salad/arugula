// A message's text, with a team chat's small Markdown (M74): fenced code
// blocks, `code`, **bold**, *italic* / _italic_, links, and @mentions as
// pills. Only Preact nodes come out, never HTML: whatever someone types
// stays text.

import type { ComponentChildren } from "preact";

/** Whether a mention (`@jake`) is the person reading. */
export type IsMe = (token: string) => boolean;

/** `landed`: whether an @token reached someone (a thread message's, #296).
 * Given, only those are marked; the rest stay plain text, so a highlight
 * never promises a notification nobody got. */
export function Markup({ text, isMe, landed }: { text: string; isMe?: IsMe; landed?: (token: string) => boolean }) {
  const out: ComponentChildren[] = [];
  // Fenced blocks first: nothing inside them is formatted.
  const fence = /```[^\n]*\n?([\s\S]*?)```/g;
  let at = 0;
  let k = 0;
  for (let m = fence.exec(text); m; m = fence.exec(text)) {
    if (m.index > at) out.push(<Lines key={k++} text={text.slice(at, m.index)} isMe={isMe} landed={landed} />);
    out.push(
      <pre key={k++} class="msg-code">
        {m[1].replace(/\n$/, "")}
      </pre>,
    );
    at = m.index + m[0].length;
  }
  if (at < text.length) out.push(<Lines key={k++} text={text.slice(at)} isMe={isMe} landed={landed} />);
  return <>{out}</>;
}

/** Text between code blocks, trimmed of the newlines around the blocks. */
function Lines({ text, isMe, landed }: { text: string; isMe?: IsMe; landed?: (token: string) => boolean }) {
  const t = text.replace(/^\n/, "").replace(/\n$/, "");
  if (!t) return null;
  return <p class="thread-text">{inline(t, isMe, landed)}</p>;
}

// One pattern for every inline form; the first group that matched says
// which. Order matters: code before the rest, so `**x**` in code stays.
const INLINE =
  /(`[^`\n]+`)|(\*\*[^*\n]+\*\*)|((?:^|(?<=[\s(]))[*_][^*_\s][^*_\n]*[*_](?=$|[\s.,;:!?)]))|(https?:\/\/[^\s<>()]+[^\s<>().,;:!?'"])|((?:^|(?<=[^\w]))@[\w.-]*\w)/g;

// The forge pass (PR and issue bodies) adds two forms after those: a
// `[text](url)` link and a `#N` reference.
const FORGE_INLINE = new RegExp(`${INLINE.source}|(\\[([^\\]\\n]+)\\]\\(([^)\\s]+)\\))|((?:^|(?<=[^\\w]))#\\d+\\b)`, "g");

/** The forge-only pass of `inline`: `[text](url)` links and `#N` references.
 * `refs` is the URL a reference's number goes after (`…/issues/`), or null
 * to leave `#N` as text. */
export type ForgeInline = { refs: string | null };

/** Only these hrefs come out of a body: whoever opened the PR wrote it. */
const safeHref = (u: string) => /^https?:\/\/[^\s/]/i.test(u);

export function inline(text: string, isMe?: IsMe, landed?: (token: string) => boolean, forge?: ForgeInline): ComponentChildren[] {
  const out: ComponentChildren[] = [];
  let at = 0;
  let k = 0;
  for (const m of text.matchAll(forge ? FORGE_INLINE : INLINE)) {
    const i = m.index!;
    if (i > at) out.push(text.slice(at, i));
    const s = m[0];
    if (m[1]) out.push(<code key={k++}>{s.slice(1, -1)}</code>);
    else if (m[2]) out.push(forge ? <strong key={k++}>{inline(s.slice(2, -2), isMe, landed, forge)}</strong> : <b key={k++}>{inline(s.slice(2, -2), isMe, landed)}</b>);
    else if (m[3]) out.push(forge ? <em key={k++}>{inline(s.slice(1, -1), isMe, landed, forge)}</em> : <i key={k++}>{inline(s.slice(1, -1), isMe, landed)}</i>);
    else if (m[4])
      out.push(
        <a key={k++} href={s} target="_blank" rel="noopener noreferrer">
          {s}
        </a>,
      );
    else if (m[6]) {
      if (safeHref(m[8]))
        out.push(
          <a key={k++} href={m[8]} target="_blank" rel="noopener noreferrer">
            {inline(m[7], isMe, landed, forge)}
          </a>,
        );
      else out.push(s);
    } else if (m[9]) {
      if (forge?.refs)
        out.push(
          <a key={k++} href={forge.refs + s.slice(1)} target="_blank" rel="noopener noreferrer">
            {s}
          </a>,
        );
      else out.push(s);
    } else if (landed && !landed(s.slice(1).toLowerCase())) out.push(s);
    else {
      const token = s.slice(1).toLowerCase();
      out.push(
        <span key={k++} class={isMe?.(token) ? "mention me" : "mention"}>
          {s}
        </span>,
      );
    }
    at = i + s.length;
  }
  if (at < text.length) out.push(text.slice(at));
  return out;
}

/** The token that @mentions a person, as the daemon matches it
 * (threads.rs `names`): their first name, or the part before the `@`. */
export function mentionToken(name: string): string {
  const first = name.trim().split(/\s+/)[0] ?? "";
  return first.split("@")[0].toLowerCase().replace(/[^\w.-]/g, "");
}
