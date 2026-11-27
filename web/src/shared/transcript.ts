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

// ---- Claude Code in a terminal

/** A Claude Code screen's marks: `⏺` starts what it says or does, `⎿`
 * hangs a tool call's result under it, `❯` (or `>`) is what you wrote. */
const SAYS = /^⏺ ?(.*)$/;
const RESULT = /^ {1,4}⎿/;
const YOU = /^[❯>] (.*)$/;
/** A tool call: `Bash(sleep 4)`, `Read(web/src/x.ts)`, `mcp__arugula__wait(…)`. */
const CALL = /^([A-Za-z][\w:.-]*)\((.*)\)\s*$/;
/** A run it already folded: `Read 2 files (ctrl+o to expand)`, `Searched for 3 patterns`. */
const FOLDED = /^(Read|Searched|Listed|Edited|Wrote|Updated|Ran|Called|Fetched|Found|Created|Deleted)\b/;
/** Its chrome: the banner, the box you write in and what's under it, a
 * turn's time and model, its spinner and timing (`✻ Crunched for 7s`). */
const CHROME = /^(?:\s*[▐▝▘▛▜▟▙█]{2}|[─━═╭╰│]|[✻✳✶✢✽·] \S|\s+\d{1,2}:\d\d (?:AM|PM) \S+$)|ctrl\+[a-z] to /;
const EDITS = new Set(["Edit", "MultiEdit", "Update", "Write", "NotebookEdit"]);

/** A path's last part, as a folded line names a file. */
const base = (p: string) => p.trim().replace(/\/+$/, "").split("/").pop() || p.trim();

/** Claude Code's screen (a terminal's last lines) cut into prose and tool
 * calls, as `partsOf` cuts an agent block's: what it says is prose, what
 * you wrote is a `**You**` line, and each tool call is one line (`Read
 * Crumb.tsx, window.spec.ts`; `Edited 3 files · a.ts, b.ts, c.ts`;
 * `Bash pnpm test`) with its results (`⎿`) folded under it; a run of reads,
 * or of edits, is one line. Its banner, the box you write in, spinners and
 * timings are left out. What it doesn't know stays prose, as it was. */
export function claudeParts(text: string, transcript: "full" | "said" = "full"): Part[] {
  const lines = text.split("\n");
  // The box you write in and its hints, at the end, are the screen's, not the transcript's.
  let end = lines.length;
  for (let i = lines.length - 1; i >= 0 && i >= lines.length - 12; i--) if (/^\s*[─━╭]{3}/.test(lines[i]!)) end = i;
  type Call = { tool: string; args: string; body: string[] };
  const out: (Part | { kind: "call"; call: Call })[] = [];
  let prose: string[] = [];
  let call: Call | null = null;
  const flush = () => {
    if (prose.join("").trim()) out.push({ kind: "prose", text: prose.join("\n").trim() });
    prose = [];
  };
  for (let i = 0; i < end; i++) {
    const l = lines[i]!.trimEnd();
    if (call && (RESULT.test(l) || (/^ {2,}\S/.test(l) && !SAYS.test(l)))) {
      call.body.push(l.replace(/^ {1,4}⎿ {0,2}/, "").replace(/^ {2,5}/, ""));
      continue;
    }
    call = null;
    const says = SAYS.exec(l)?.[1];
    if (says !== undefined) {
      const c = CALL.exec(says);
      if (c || FOLDED.test(says)) {
        flush();
        call = c ? { tool: c[1]!, args: c[2]!, body: [] } : { tool: "", args: says.replace(/\s*\(ctrl\+[a-z] to \w+\)\s*$/, ""), body: [] };
        out.push({ kind: "call", call });
      } else prose.push(says);
      continue;
    }
    if (CHROME.test(l)) continue;
    const you = YOU.exec(l)?.[1];
    if (you !== undefined) {
      flush();
      prose.push(`**You** ${you}`);
      flush();
      continue;
    }
    // A said line's continuation is indented two.
    prose.push(l.replace(/^ {2}/, ""));
  }
  flush();

  // Runs of reads, or of edits, are one line.
  const parts: Part[] = [];
  for (let i = 0; i < out.length; ) {
    const p = out[i]!;
    if (p.kind !== "call") {
      parts.push(p);
      i++;
      continue;
    }
    const kind = (c: Call) => (c.tool === "Read" ? "read" : EDITS.has(c.tool) ? "edit" : null);
    const k = kind(p.call);
    let j = i + 1;
    while (k && j < out.length && out[j]!.kind === "call" && kind((out[j] as { call: Call }).call) === k) j++;
    const run = out.slice(i, j).map((x) => (x as { call: Call }).call);
    const files = [...new Set(run.map((c) => base(c.args)))];
    const line = k === "read" ? `Read ${files.join(", ")}` : k === "edit" ? (files.length > 1 ? `Edited ${files.length} files · ${files.join(", ")}` : `Edited ${files[0]}`) : p.call.tool ? `${p.call.tool} ${p.call.args}` : p.call.args;
    const body = run.flatMap((c) => (run.length > 1 ? [`${c.tool}(${c.args})`, ...c.body.map((b) => `  ${b}`)] : c.body)).join("\n").trim();
    if (transcript === "full") parts.push({ kind: "tool", line: line.trim(), body });
    i = j;
  }
  return parts;
}

/** Whether a terminal's lines are Claude Code's screen: a `⏺` line in them. */
export const isClaudeScreen = (text: string): boolean => /^⏺ /m.test(text);
