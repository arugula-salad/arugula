// A journey's report (#551): one self-contained HTML page. Totals, a
// swimlane of who did what where (one lane per person and surface, steps
// left to right in the order taken), the first step a newcomer couldn't
// take from the screen alone, and every step with its screenshot.

import type { Step, Surface } from "./record";

type Data = {
  name: string;
  title: string;
  actors: string[];
  at: string;
  steps: Step[];
};

const SURFACES: Surface[] = ["app", "browser", "terminal", "message"];
const esc = (s: string) =>
  s.replace(/[&<>"']/g, (c) => `&#${c.charCodeAt(0)};`);
const secs = (ms: number) =>
  ms < 1000 ? `${ms} ms` : `${(ms / 1000).toFixed(1)} s`;
const MARK = { ok: "✓", unguided: "!", failed: "✕" } as const;
const WORD = { ok: "ok", unguided: "unguided", failed: "failed" } as const;

export function totals(steps: Step[]) {
  let switches = 0;
  let handoffs = 0;
  for (let i = 1; i < steps.length; i++) {
    const [a, b] = [steps[i - 1], steps[i]];
    if (a.surface !== b.surface || a.actor !== b.actor) switches++;
    if (a.actor !== b.actor || b.surface === "message") handoffs++;
  }
  const end = steps.at(-1);
  return {
    steps: steps.length,
    wall: end ? end.start + end.ms : 0,
    switches,
    handoffs,
    compared: steps.reduce((n, s) => n + s.compared.length, 0),
    typed: steps.reduce((n, s) => n + s.typed.length, 0),
    unguided: steps.filter((s) => s.result === "unguided").length,
    failed: steps.filter((s) => s.result === "failed").length,
  };
}

function lanes(d: Data) {
  const used = new Set(d.steps.map((s) => `${s.actor}/${s.surface}`));
  const out: { key: string; actor: string; surface: Surface }[] = [];
  for (const actor of d.actors)
    for (const surface of SURFACES)
      if (used.has(`${actor}/${surface}`))
        out.push({ key: `${actor}/${surface}`, actor, surface });
  return out;
}

/** A wait this long is friction worth a note, even when the step works. */
const SLOW_MS = 10_000;

/** What the graph says beside a step: the problem, or what went right. */
export function annotation(s: Step): {
  kind: "ok" | "slow" | "unguided" | "failed";
  text: string;
} {
  if (s.result === "failed")
    return { kind: "failed", text: `Failed: ${(s.note ?? "").split("\n")[0]}` };
  if (s.result === "unguided")
    return { kind: "unguided", text: `Not led here: ${s.prompt}` };
  if (s.ms >= SLOW_MS)
    return { kind: "slow", text: `Works, but the person waits ${secs(s.ms)}` };
  const bits = [
    s.prompt.startsWith("the person's own doing")
      ? s.prompt.replace("the person's own doing: ", "")
      : `led by "${s.prompt.slice(0, 60)}${s.prompt.length > 60 ? "…" : ""}"`,
  ];
  if (s.compared.length) bits.push(`${s.compared.length} compared, matched`);
  if (s.typed.length) bits.push(`typed ${s.typed.length}`);
  if (s.opened.length) bits.push("opened the browser");
  return { kind: "ok", text: bits.join(" · ") };
}

/** Wrap `text` to lines of at most `n` characters. */
function wrap(text: string, n: number): string[] {
  const out: string[] = [];
  let line = "";
  for (const w of text.split(/\s+/)) {
    if (line && (line + " " + w).length > n) {
      out.push(line);
      line = w;
    } else line = line ? `${line} ${w}` : w;
  }
  if (line) out.push(line);
  return out.slice(0, 3);
}

/** The path as a graph: lanes are columns (each person's surfaces), steps
 * are rows top to bottom in the order taken, and each row is annotated
 * with what went right or what went wrong. Self-contained (inline styles
 * read the page's tokens, with fallbacks for the standalone .svg). */
export function swimlane(d: Data) {
  const ls = lanes(d);
  const COL = 92;
  const TOP = 44;
  const LEFT = 36;
  const notesX = LEFT + ls.length * COL + 20;
  const W = notesX + 640;
  const rowH = (s: Step) => 26 + 16 * wrap(annotation(s).text, 78).length;
  const ys: number[] = [];
  let y = TOP;
  for (const s of d.steps) {
    ys.push(y + rowH(s) / 2);
    y += rowH(s);
  }
  const H = y + 16;
  const cx = (s: Step) =>
    LEFT +
    ls.findIndex((l) => l.key === `${s.actor}/${s.surface}`) * COL +
    COL / 2;
  const v = (name: string, fallback: string) => `var(--${name}, ${fallback})`;
  const COLORS = {
    ok: v("good", "#0ca30c"),
    slow: v("serious", "#ec835a"),
    unguided: v("warn", "#fab219"),
    failed: v("bad", "#d03b3b"),
  };
  const ICON = { ok: "✓", slow: "◷", unguided: "!", failed: "✕" };
  const heads = ls
    .map((l, i) => {
      const x = LEFT + i * COL;
      return (
        `<rect x="${x + 4}" y="${TOP - 8}" width="${COL - 8}" height="${H - TOP - 8}" rx="8" fill="${v(i % 2 ? "lane-b" : "lane-a", "#f2f1ec")}"/>` +
        `<text x="${x + COL / 2}" y="${TOP - 16}" text-anchor="middle" font-size="12" font-weight="600" fill="${v("ink2", "#52514e")}">${esc(d.actors.length > 1 ? `${l.actor}` : l.surface)}</text>` +
        (d.actors.length > 1
          ? `<text x="${x + COL / 2}" y="${TOP - 2}" text-anchor="middle" font-size="10" fill="${v("ink2", "#52514e")}">${l.surface}</text>`
          : "")
      );
    })
    .join("");
  const links = d.steps
    .slice(1)
    .map((s, j) => {
      const a = d.steps[j];
      const moved = a.surface !== s.surface || a.actor !== s.actor;
      const [x1, y1, x2, y2] = [cx(a), ys[j], cx(s), ys[j + 1]];
      const path =
        x1 === x2
          ? `M${x1},${y1 + 13} L${x2},${y2 - 13}`
          : `M${x1},${y1 + 13} C${x1},${(y1 + y2) / 2} ${x2},${(y1 + y2) / 2} ${x2},${y2 - 13}`;
      return `<path d="${path}" fill="none" stroke="${moved ? v("switch", "#2a78d6") : v("link", "#a3a29b")}" stroke-width="2"${moved ? ' stroke-dasharray="5 3"' : ""}/>`;
    })
    .join("");
  const rows = d.steps
    .map((s, i) => {
      const a = annotation(s);
      const lines = wrap(a.text, 78);
      const ink = a.kind === "ok" ? v("ink2", "#52514e") : COLORS[a.kind];
      const glyphInk =
        a.kind === "unguided" || a.kind === "slow" ? "#0b0b0b" : "#ffffff";
      const top = ys[i] - rowH(s) / 2 + 18;
      return (
        `<g><title>${i + 1}. ${esc(s.title)}: ${esc(a.text)}</title>` +
        `<circle cx="${cx(s)}" cy="${ys[i]}" r="12" fill="${COLORS[a.kind]}" stroke="${v("bg", "#fcfcfb")}" stroke-width="2"/>` +
        `<text x="${cx(s)}" y="${ys[i] + 4}" text-anchor="middle" font-size="12" font-weight="800" fill="${glyphInk}">${ICON[a.kind]}</text>` +
        `<text x="${LEFT - 8}" y="${ys[i] + 4}" text-anchor="end" font-size="11" fill="${v("ink2", "#52514e")}">${i + 1}</text>` +
        `<line x1="${cx(s) + 14}" y1="${ys[i]}" x2="${notesX - 6}" y2="${ys[i]}" stroke="${v("line", "#e4e3de")}" stroke-width="1"/>` +
        `<text x="${notesX}" y="${top}" font-size="13" font-weight="600" fill="${v("ink", "#0b0b0b")}">${esc(s.title)} <tspan font-weight="400" fill="${v("ink2", "#52514e")}">${secs(s.ms)}</tspan></text>` +
        lines
          .map(
            (l, k) =>
              `<text x="${notesX}" y="${top + 16 * (k + 1)}" font-size="12" fill="${ink}">${k === 0 ? `${ICON[a.kind]} ` : ""}${esc(l)}</text>`,
          )
          .join("") +
        `</g>`
      );
    })
    .join("");
  return `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 ${W} ${H}" width="${W}" height="${H}" font-family="system-ui, sans-serif" role="img" aria-label="${esc(d.name)}: ${d.steps.length} steps, annotated">${heads}${links}${rows}</svg>`;
}

/** The graph alone, as a file: on a plain background of its own. */
export function graph(d: Data) {
  const t = totals(d.steps);
  const svg = swimlane(d);
  const verdict = t.failed
    ? "stopped at a step it couldn't take"
    : t.unguided
      ? `${t.unguided} step(s) nothing on screen led to`
      : "led all the way";
  const m = /^<svg [^>]*viewBox="0 0 (\d+) (\d+)"[^>]*>([\s\S]*)<\/svg>$/.exec(
    svg,
  )!;
  const [w, h] = [Number(m[1]), Number(m[2]) + 52];
  return (
    `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 ${w} ${h}" width="${w}" height="${h}" font-family="system-ui, sans-serif" role="img" aria-label="${esc(d.name)} journey">` +
    `<rect width="100%" height="100%" fill="#fcfcfb"/>` +
    `<text x="16" y="22" font-size="14" font-weight="700" fill="#0b0b0b">${esc(d.name)}: ${esc(d.title)}: ${esc(verdict)}</text>` +
    `<text x="16" y="40" font-size="12" fill="#52514e">${t.steps} steps · ${secs(t.wall)} · ${t.switches} switches · ${t.handoffs} hand-offs between people · ${t.compared} compared · ${t.typed} typed · ${new Date(d.at).toLocaleString()}</text>` +
    `<g transform="translate(0 52)">${m[3]}</g></svg>`
  );
}

export function report(d: Data): string {
  const t = totals(d.steps);
  const first = d.steps.find((s) => s.result !== "ok");
  const tile = (n: string | number, label: string, warn = false) =>
    `<div class="tile${warn ? " warn" : ""}"><b>${n}</b><span>${label}</span></div>`;
  const verdict = t.failed
    ? "stopped"
    : t.unguided
      ? "not led all the way"
      : "led all the way";
  const step = (s: Step, i: number) => `
    <tr id="s${i}" class="${s.result}">
      <td class="n">${i + 1}</td>
      <td><span class="badge ${s.result}">${MARK[s.result]} ${WORD[s.result]}</span></td>
      <td><b>${esc(s.title)}</b><div class="dim">${esc(s.actor)} · ${s.surface} · <code>${esc(s.id)}</code></div>
        <div class="prompt">${s.result === "unguided" ? "No prompt: " : "Prompt: "}${esc(s.prompt)}</div>
        ${s.note ? `<pre class="note">${esc(s.note)}</pre>` : ""}
        ${s.compared.length ? `<div class="dim">Compared: ${s.compared.map((c) => `<code>${esc(c)}</code>`).join(", ")}</div>` : ""}
        ${s.opened.length ? `<div class="dim">Opened in the browser: ${s.opened.map((c) => `<code>${esc(c)}</code>`).join(", ")}</div>` : ""}
        ${s.typed.length ? `<div class="dim">Typed: ${s.typed.map((c) => `<code>${esc(c)}</code>`).join(", ")}</div>` : ""}</td>
      <td class="num">${secs(s.ms)}</td>
      <td>${s.shot ? `<a href="#shot${i}"><img loading="lazy" src="data:image/jpeg;base64,${s.shot}" alt="Screenshot after step ${i + 1}"></a>` : ""}</td>
    </tr>`;
  const shots = d.steps
    .map((s, i) =>
      s.shot
        ? `<a class="lightbox" id="shot${i}" href="#s${i}"><img src="data:image/jpeg;base64,${s.shot}" alt="Step ${i + 1} full size"></a>`
        : "",
    )
    .join("");
  return `<!doctype html>
<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1">
<title>${esc(d.name)} journey</title>
<style>
:root { --serious:#ec835a; --bg:#fcfcfb; --ink:#0b0b0b; --ink2:#52514e; --line:#e4e3de; --lane-a:#f6f5f1; --lane-b:#fcfcfb;
  --good:#0ca30c; --warn:#fab219; --bad:#d03b3b; --link:#a3a29b; --switch:#2a78d6; }
@media (prefers-color-scheme: dark) { :root:not([data-theme="light"]) { --bg:#1a1a19; --ink:#ffffff; --ink2:#c3c2b7; --line:#34332f; --lane-a:#222220; --lane-b:#1a1a19; --link:#6b6a63; --switch:#3987e5; } }
:root[data-theme="dark"] { --bg:#1a1a19; --ink:#ffffff; --ink2:#c3c2b7; --line:#34332f; --lane-a:#222220; --lane-b:#1a1a19; --link:#6b6a63; --switch:#3987e5; }
body { margin:0; background:var(--bg); color:var(--ink); font:14px/1.5 system-ui, sans-serif; }
main { max-width:1200px; margin:0 auto; padding:24px 16px 64px; }
h1 { font-size:22px; margin:0 0 4px; } h2 { font-size:16px; margin:28px 0 8px; }
.dim { color:var(--ink2); font-size:13px; }
.tiles { display:flex; flex-wrap:wrap; gap:8px; margin:16px 0; }
.tile { border:1px solid var(--line); border-radius:8px; padding:8px 12px; min-width:92px; }
.tile b { display:block; font-size:22px; font-variant-numeric:tabular-nums; } .tile span { color:var(--ink2); font-size:12px; }
.tile.warn { border-color:var(--bad); }
.scroll { overflow-x:auto; border:1px solid var(--line); border-radius:8px; }
.legend { display:flex; gap:16px; flex-wrap:wrap; margin:8px 0; font-size:12px; color:var(--ink2); }
.legend i { display:inline-block; width:12px; height:12px; border-radius:3px; vertical-align:-2px; margin-right:4px; }
.callout { border:1px solid var(--line); border-left:4px solid var(--bad); border-radius:8px; padding:12px 16px; }
.callout.unguided { border-left-color:var(--warn); }
.callout img { max-width:100%; border:1px solid var(--line); border-radius:6px; margin-top:8px; }
table { border-collapse:collapse; width:100%; } td { border-top:1px solid var(--line); padding:8px; vertical-align:top; }
td.n, td.num { font-variant-numeric:tabular-nums; color:var(--ink2); white-space:nowrap; }
td img { width:180px; border:1px solid var(--line); border-radius:4px; }
.badge { white-space:nowrap; font-size:12px; font-weight:600; padding:2px 8px; border-radius:999px; border:1px solid var(--line); }
.badge.ok { color:var(--ink); } .badge.unguided { background:var(--warn); color:#0b0b0b; } .badge.failed { background:var(--bad); color:#fff; }
.prompt { margin-top:4px; font-size:13px; }
pre.note { white-space:pre-wrap; font-size:12px; color:var(--bad); margin:4px 0 0; }
code { font-size:12px; }
.lightbox { display:none; } .lightbox:target { display:flex; position:fixed; inset:0; background:rgba(0,0,0,.8); align-items:center; justify-content:center; padding:16px; z-index:9; }
.lightbox img { max-width:100%; max-height:100%; }
@media (max-width:640px) { td img { width:96px; } }
</style></head><body><main>
<h1>${esc(d.name)}: ${esc(d.title)}</h1>
<div class="dim">${esc(d.at)} · ${esc(verdict)}</div>
<div class="tiles">
${tile(t.steps, "steps")}${tile(secs(t.wall), "wall time")}${tile(t.switches, "switches (surface or person)")}${tile(t.handoffs, "hand-offs between people")}${tile(t.compared, "codes and fingerprints compared")}${tile(t.typed, "commands typed")}${tile(t.unguided, "unguided steps", t.unguided > 0)}${tile(t.failed, "failed", t.failed > 0)}
</div>
${
  first
    ? `<h2>Where a newcomer gets stuck</h2><div class="callout ${first.result}"><b>Step ${d.steps.indexOf(first) + 1}: ${esc(first.title)}</b> <span class="badge ${first.result}">${MARK[first.result]} ${WORD[first.result]}</span>
<div class="prompt">${first.result === "unguided" ? "No prompt: " : "Prompt: "}${esc(first.prompt)}</div>${first.note ? `<pre class="note">${esc(first.note)}</pre>` : ""}
${first.shot ? `<img src="data:image/jpeg;base64,${first.shot}" alt="Screenshot at that step">` : ""}</div>`
    : ""
}
<h2>The path</h2>
<div class="legend"><span><i style="background:var(--good)"></i>✓ led by the screen</span><span><i style="background:var(--serious)"></i>◷ works, but slow</span><span><i style="background:var(--warn)"></i>! nothing on screen led there</span><span><i style="background:var(--bad)"></i>✕ failed</span><span><i style="background:var(--switch)"></i>dashed: a switch of surface or person</span></div>
<div class="scroll">${swimlane(d)}</div>
<h2>Every step</h2>
<table>${d.steps.map(step).join("")}</table>
${shots}
</main></body></html>`;
}
