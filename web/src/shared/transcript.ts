// An agent block's transcript as the panel draws it (Lift 4, #1480): the
// Markdown its machine's `capture --text` gives (crates/daemon's
// agent/transcript.rs: `## You`, its prose, `> ` thoughts, and a tool call
// as `**Kind** \`title\` (status)` with its command, questions and fenced
// output under it), cut into prose and tool calls, so a tool call shows as
// one line that opens. `said` (surfacesFor's transcript) leaves the tool
// calls out. Run with `node --experimental-strip-types`.

export type Part = { kind: "prose"; text: string } | { kind: "tool"; line: string; body: string };

/** A tool call's first line: `**Read** \`web/src/x.ts\` (completed, exit 0)`. */
const TOOL = /^\*\*[^*\n]+\*\* `[^`\n]*` \([^)\n]*\)\s*$/;
const FENCE = /^ {0,3}(`{3,}|~{3,})/;

/** `text` in prose and tool calls, in order. A tool call runs from its line
 * through what hangs under it: its indented command, its questions, its
 * fenced output, and the blank lines between them. */
export function partsOf(text: string, transcript: "full" | "said" = "full"): Part[] {
  const lines = text.split("\n");
  const out: Part[] = [];
  let prose: string[] = [];
  const flush = () => {
    if (prose.join("").trim()) out.push({ kind: "prose", text: prose.join("\n").trim() });
    prose = [];
  };
  for (let i = 0; i < lines.length; ) {
    const l = lines[i]!;
    if (!TOOL.test(l)) {
      prose.push(l);
      i++;
      continue;
    }
    flush();
    const body: string[] = [];
    let fence: string | null = null;
    let j = i + 1;
    for (; j < lines.length; j++) {
      const b = lines[j]!;
      const f = FENCE.exec(b)?.[1];
      if (fence) {
        body.push(b);
        if (f && f[0] === fence[0] && f.length >= fence.length && !b.trim().slice(f.length)) fence = null;
        continue;
      }
      if (f) {
        fence = f;
        body.push(b);
      } else if (!b.trim() || /^ {4}/.test(b) || /^- /.test(b)) body.push(b);
      else break;
    }
    if (transcript === "full") out.push({ kind: "tool", line: l.trim(), body: body.join("\n").replace(/^(\s*\n)+|\s+$/g, "") });
    i = j;
  }
  flush();
  return out;
}

/** The last `n` of `all`. Markdown cut inside a fenced block opens the
 * fence again, so the block's closing line doesn't open one that runs to
 * the end. */
export function lastLines(all: string[], md: boolean, n: number): string[] {
  const tail = all.slice(-n);
  if (!md || all.length <= n) return tail;
  let fence: string | null = null;
  for (const l of all.slice(0, -n)) {
    const m = FENCE.exec(l)?.[1];
    if (m && !fence) fence = m;
    else if (m && fence && m[0] === fence[0] && m.length >= fence.length && !l.trim().slice(m.length)) fence = null;
  }
  return fence ? [fence, ...tail] : tail;
}
