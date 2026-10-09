// The journeys on one page (#664): each journey a line through the steps it
// takes, drawn as a metro map, and a grid of which journey reaches which
// part of the product. Built from the reports the runs leave in
// web/journey-reports/ (`J1.json`, `J2a-setup-sam.json`, …), into
// `map.html` beside them.
//
// Every step of every report must land on a station (`STEPS` below): a step
// with none stops the build, so a journey can't drop off the map quietly. A
// journey with no report yet (J3 and J4 run only on a Mac) is drawn dashed.
// Stations are placed by hand; colours and states come from the reports.
//   just journey-map                 (also the end of `just journey`)
//   node --experimental-strip-types e2e/journey/map.ts [DIR]

import { execFileSync } from "node:child_process";
import { existsSync, readdirSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { reports, type Step } from "./record.ts";

type Zone = "get" | "first" | "together";
type Report = { name: string; title: string; actors: string[]; at: string; steps: Step[] };
type State = "led" | "friction" | "unguided" | "none";

/** Station id: where it sits, what it's called, its zone. */
const STATIONS: Record<string, [x: number, y: number, label: string, zone: Zone]> = {
  site: [110, 92, "Install page", "get"],
  download: [300, 92, "Download the app", "get"],
  gatekeeper: [490, 92, "Gatekeeper", "get"],
  older: [680, 92, "Older daemon: update", "get"],
  script: [300, 182, "install.sh", "get"],
  status: [490, 182, "arugula status", "get"],
  open: [110, 342, "App opens", "first"],
  gs: [270, 342, "Getting started", "first"],
  phone: [420, 342, "Phone step", "first"],
  connect: [570, 292, "Connect to control", "first"],
  account: [720, 292, "Account, machine approved", "first"],
  signin: [870, 292, "App signed in", "first"],
  terminal: [1020, 292, "A terminal", "first"],
  skipcloud: [570, 412, "No account", "first"],
  claude: [720, 412, "Claude Code set up", "first"],
  agent: [870, 412, "An agent asks; approved", "first"],
  shareacct: [1170, 212, "Share to an account", "together"],
  team: [1290, 322, "Team and invite", "together"],
  tailshare: [720, 532, "Share to a tailnet login", "together"],
  drive: [1170, 452, "Ask to drive, allowed", "together"],
  samelogin: [1290, 532, "One login, two devices", "together"],
};
/** Labels drawn under their station, where above would collide. */
const BELOW = new Set(["status", "script", "skipcloud", "claude", "agent", "tailshare", "samelogin", "drive", "team"]);

const J1: Record<string, string> = {
  "app-opens": "open",
  "getting-started": "gs",
  "set-it-up": "gs",
  "skip-phone": "phone",
  connect: "connect",
  "open-approval": "connect",
  "sign-up": "account",
  "github-authorize": "account",
  "recovery-codes": "account",
  "approve-machine": "account",
  "confirm-account": "account",
  joined: "account",
  "app-signin": "signin",
  "allow-app": "signin",
  "approve-app": "signin",
  terminal: "terminal",
  restart: "terminal",
};
/** Each journey's step ids, to stations. J1-mac and the J2 setups take J1's. */
const STEPS: Record<string, Record<string, string>> = {
  J1,
  J2a: {
    agree: "shareacct",
    "open-share": "shareacct",
    "share-who": "shareacct",
    "ask-fingerprint": "shareacct",
    "find-fingerprint": "shareacct",
    "send-fingerprint": "shareacct",
    "confirm-share": "shareacct",
    accept: "shareacct",
    "find-machine": "shareacct",
    "try-typing": "drive",
    "ask-to-drive": "drive",
    allow: "drive",
    "riley-types": "drive",
    "take-control": "drive",
    "riley-drives": "drive",
    "sam-sees": "drive",
  },
  J2b: {
    "make-team": "team",
    "send-invite": "team",
    "sam-joins": "team",
    "how-to-add": "team",
    "move-in": "team",
    "riley-finds": "team",
    "riley-types": "drive",
    "sam-sees": "drive",
  },
  J2c: {
    dlex: "team",
    "send-invite": "team",
    "sam-joins": "team",
    "team-page": "team",
    "copied-join": "team",
    "try-move": "team",
    "share-team": "team",
    "riley-finds": "team",
    swarm: "team",
    "riley-tries": "drive",
    "riley-asks": "drive",
    allow: "drive",
    "take-control": "drive",
    "riley-drives": "drive",
    "plus-on-geek": "drive",
  },
  J3: {
    download: "download",
    move: "download",
    refused: "gatekeeper",
    privacy: "gatekeeper",
    "open-anyway": "gatekeeper",
    "open-anyway-again": "gatekeeper",
    password: "gatekeeper",
    offered: "older",
    update: "older",
  },
  J4: { task: "site", "read-page": "site", install: "script", help: "status", status: "status" },
  J5: {
    open: "open",
    "getting-started": "gs",
    "set-it-up": "gs",
    "skip-phone": "phone",
    "skip-cloud": "skipcloud",
    "use-claude": "claude",
    "start-agent": "agent",
    task: "agent",
    approve: "agent",
    done: "agent",
  },
  J6: {
    open: "open",
    "close-setup": "gs",
    share: "tailshare",
    "share-who": "tailshare",
    "tell-ben": "tailshare",
    "ben-opens": "tailshare",
    "ben-watches": "tailshare",
    "ben-tries": "tailshare",
    "make-driver": "drive",
    "ask-to-drive": "drive",
    allow: "drive",
    "take-control": "drive",
    "ben-drives": "drive",
    "cam-opens": "samelogin",
    "ana-sees-shared-login": "samelogin",
  },
};
const FIRST_RUN = ["open", "gs", "phone", "connect", "account", "signin", "terminal"];
/** The lines, in drawing order (it sets where each sits in a bundle). */
const LINES: [name: string, title: string, path: string[]][] = [
  ["J1", "one person, Mac app, from no account to a terminal", FIRST_RUN],
  ["J2a", "two friends: a session shared to an account", [...FIRST_RUN, "shareacct", "drive"]],
  ["J2b", "two friends through a team", [...FIRST_RUN, "team", "drive"]],
  ["J2c", "a machine in one team, a friend's second team", [...FIRST_RUN, "team", "drive"]],
  ["J5", "won't read docs: to a working agent", ["open", "gs", "phone", "skipcloud", "claude", "agent"]],
  ["J6", "two people on a tailnet, no control", ["open", "gs", "tailshare", "drive", "samelogin"]],
  ["J3", "installs the Mac app by hand, over an older daemon", ["site", "download", "gatekeeper", "older"]],
  ["J4", "an agent installs over ssh", ["site", "script", "status"]],
];
/** Parts of the product no journey reaches yet, for the grid. */
const UNCOVERED: [string, string][] = [
  ["Phone paired (QR, notifications)", "Getting started's Phone step: every journey skips it"],
  ["Homebrew install", "on the install page; no journey"],
  ["Linux or Windows app", "the journeys run the Mac app or a browser"],
  ["Read-only share link", "Share's link for people with no login; no journey"],
  ["Dropped by control, join again", "control-dropped.spec covers the screens, not a person's path"],
];

const esc = (s: string) => s.replace(/[&<>"']/g, (c) => `&#${c.charCodeAt(0)};`);
const GAP = 6;

/** The journey a report's steps belong to: a J2 setup and J1-mac are J1's. */
const journeyOf = (name: string) => (name === "J1-mac" || name.includes("-setup-") ? "J1" : name);
/** The grid column a report counts toward: J1-mac under J1, a setup under its J2. */
const columnOf = (name: string) => (name === "J1-mac" ? "J1" : name.replace(/-setup-.*/, ""));

export function build(dir: string): { html: string; runs: number; steps: number; missing: string[] } {
  const runs: Report[] = readdirSync(dir)
    .filter((f) => /^J[0-9][a-z]?(-mac|-setup-[a-z0-9]+)?\.json$/.test(f))
    .sort()
    .map((f) => JSON.parse(readFileSync(join(dir, f), "utf8")) as Report);
  const atStation: Record<string, [string, Step][]> = Object.fromEntries(Object.keys(STATIONS).map((k) => [k, []]));
  const unmapped: string[] = [];
  for (const r of runs) {
    const map = STEPS[journeyOf(r.name)];
    for (const s of r.steps) {
      const sid = map?.[s.id];
      if (sid) atStation[sid].push([r.name, s]);
      else unmapped.push(`${r.name}: ${s.id}`);
    }
  }
  if (unmapped.length) throw new Error(`steps with no station in e2e/journey/map.ts (STEPS): ${unmapped.join(", ")}`);

  const stateOf = (steps: [string, Step][]): State =>
    !steps.length ? "none" : steps.some(([, s]) => s.result !== "ok") ? "unguided" : steps.some(([, s]) => s.issues.length) ? "friction" : "led";
  const reported = new Set(runs.map((r) => columnOf(r.name)));
  const missing = LINES.map(([n]) => n).filter((n) => !reported.has(n));

  // --- the map
  const through: Record<string, string[]> = Object.fromEntries(Object.keys(STATIONS).map((k) => [k, []]));
  for (const [name, , path] of LINES) for (const sid of path) through[sid].push(name);
  const pt = (name: string, sid: string) => {
    const [x, y] = STATIONS[sid];
    const names = through[sid];
    return [x, y + (names.indexOf(name) - (names.length - 1) / 2) * GAP] as const;
  };
  const svg: string[] = [
    `<svg viewBox="0 0 1400 600" role="img" aria-label="Metro map of the journeys: each is a coloured line through the steps it takes, from installing Arugula to two people working together.">`,
    `<rect class="zone" x="40" y="40" width="720" height="190" rx="14"/><text class="zone-label" x="58" y="218">GET IT</text>`,
    `<rect class="zone" x="40" y="252" width="1020" height="210" rx="14"/><text class="zone-label" x="58" y="276">FIRST RUN</text>`,
    `<text class="zone-label" x="1362" y="196" text-anchor="end">TOGETHER</text>`,
  ];
  for (const [name, title, path] of LINES) {
    const pts = path.map((s) => pt(name, s).map((n) => n.toFixed(1)).join(",")).join(" ");
    const dash = missing.includes(name) ? ' stroke-dasharray="2 9"' : "";
    svg.push(`<g class="line" data-line="${name}"><title>${name}: ${esc(title)}${dash ? " (no report yet)" : ""}</title><polyline class="track" points="${pts}"${dash} style="stroke: var(--${name.toLowerCase()})"/></g>`);
  }
  const ends: Record<string, string[]> = {};
  for (const [name, , path] of LINES) (ends[path.at(-1)!] ??= []).push(name);
  for (const [sid, names] of Object.entries(ends)) {
    const [x, y] = STATIONS[sid];
    names.forEach((name, i) => {
      const cx = x + 16 + i * 38;
      svg.push(
        `<g class="tag" data-line="${name}"><rect x="${cx}" y="${y - 8}" width="34" height="16" rx="8" style="fill: var(--${name.toLowerCase()})"/><text x="${cx + 17}" y="${y + 4}" text-anchor="middle">${name}</text></g>`,
      );
    });
  }
  for (const [sid, [x, y, label]] of Object.entries(STATIONS)) {
    const h = 12 + (Math.max(1, through[sid].length) - 1) * GAP;
    const state = stateOf(atStation[sid]);
    const steps = atStation[sid];
    const tip = `${label}: ${steps.length ? steps.slice(0, 6).map(([n, s]) => `${n}: ${s.title}`).join("; ") + (steps.length > 6 ? `; and ${steps.length - 6} more` : "") : "no report reaches it yet"}`;
    svg.push(`<g class="station ${state}${sid === "phone" ? " skipped" : ""}"><title>${esc(tip)}</title><rect x="${x - 8}" y="${(y - h / 2).toFixed(1)}" width="16" height="${h}" rx="8"/></g>`);
    const ty = BELOW.has(sid) ? y + h / 2 + 16 : y - h / 2 - 9;
    svg.push(`<text class="slabel" x="${x}" y="${ty.toFixed(1)}" text-anchor="middle">${esc(label)}</text>`);
    if (state === "friction") svg.push(`<text class="snote friction" x="${x}" y="${(ty + 15).toFixed(1)}" text-anchor="middle">friction</text>`);
    if (state === "unguided") svg.push(`<text class="snote unguided" x="${x}" y="${(ty + 15).toFixed(1)}" text-anchor="middle">unguided</text>`);
    if (sid === "phone") svg.push(`<text class="snote skipped" x="${x}" y="${(y + h / 2 + 16).toFixed(1)}" text-anchor="middle">always skipped</text>`);
  }
  svg.push("</svg>");

  // --- the grid
  const cols = LINES.map(([n]) => n).sort((a, b) => a.localeCompare(b, "en", { numeric: true }));
  const zoneName: Record<Zone, string> = { get: "Get it", first: "First run", together: "Together" };
  const rows: string[] = [];
  let zone: Zone | null = null;
  for (const [sid, [, , label, z]] of Object.entries(STATIONS)) {
    if (z !== zone) rows.push(`<tr class="zone-row"><th colspan="${cols.length + 1}" scope="colgroup">${zoneName[(zone = z)]}</th></tr>`);
    const cells = cols.map((c) => {
      const steps = atStation[sid].filter(([n]) => columnOf(n) === c);
      let s: State | "skipped" = stateOf(steps);
      if (s === "none") return `<td class="c none"><span class="sr">not in this journey</span></td>`;
      let mark = { led: "✓", friction: "!", unguided: "✕" }[s];
      if (sid === "phone") [s, mark] = ["skipped", "skip"];
      return `<td class="c ${s}" title="${esc(steps.slice(0, 5).map(([, st]) => st.title).join("; "))}"><span class="mark">${mark}</span><span class="sr">${s}</span></td>`;
    });
    rows.push(`<tr><th scope="row">${esc(label)}</th>${cells.join("")}</tr>`);
  }
  rows.push(`<tr class="zone-row"><th colspan="${cols.length + 1}" scope="colgroup">No journey yet</th></tr>`);
  for (const [label, why] of UNCOVERED)
    rows.push(`<tr class="gap"><th scope="row">${esc(label)}<span class="why">${esc(why)}</span></th>${cols.map(() => `<td class="c none"><span class="sr">not covered</span></td>`).join("")}</tr>`);

  const all = runs.flatMap((r) => r.steps.map((s) => [r.name, s] as const));
  const friction = all.filter(([, s]) => s.issues.length);
  const unguided = all.filter(([, s]) => s.result !== "ok");
  const at = runs.map((r) => r.at).sort().at(-1) ?? "";
  let rev = "";
  try {
    rev = execFileSync("git", ["rev-parse", "--short", "HEAD"], { encoding: "utf8" }).trim();
  } catch {
    // not in a checkout
  }
  const legend = LINES.map(
    ([n, t]) =>
      `<button type="button" class="chip" data-line="${n}" aria-pressed="false"><span class="swatch" style="background: var(--${n.toLowerCase()})"></span><span class="chip-name">${n}</span><span class="chip-title">${esc(t)}${missing.includes(n) ? " (no report yet)" : ""}</span></button>`,
  ).join("");
  const html = page({
    svg: svg.join("\n"),
    legend,
    head: cols.map((c) => `<th scope="col"><span class="swatch" style="background: var(--${c.toLowerCase()})"></span>${c}</th>`).join(""),
    rows: rows.join("\n"),
    friction: friction.length ? friction.map(([n, s]) => `<li><b>${esc(n)} · ${esc(s.title)}</b>: ${esc(s.issues.join("; "))}</li>`).join("") : "<li>None.</li>",
    facts: `<li>${runs.length} <span>runs</span></li><li>${all.length} <span>steps</span></li><li>${unguided.length} <span>unguided</span></li><li>${friction.length} <span>with friction</span></li><li><span>newest run</span> ${esc(at.slice(0, 16).replace("T", " "))} <span>UTC${rev ? `, built at ${esc(rev)}` : ""}</span></li>${missing.length ? `<li><span>no report yet:</span> ${missing.join(", ")}</li>` : ""}`,
  });
  return { html, runs: runs.length, steps: all.length, missing };
}

function page(p: { svg: string; legend: string; head: string; rows: string; friction: string; facts: string }): string {
  return `<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Arugula Journey Map</title>
<link rel="stylesheet" href="https://fonts.googleapis.com/css2?family=Overpass:wght@500;700;800&family=Overpass+Mono:wght@500&family=Public+Sans:wght@400;600&display=swap">
<style>
/* A wayfinding sheet: the map across the page; facts, legend and the coverage grid under it in one column. */
:root {
  --bg: #f4f6f8; --panel: #ffffff; --fg: #16202b; --muted: #5b6876; --rule: #d5dce3; --zone: #e9eef3;
  --led: #1f8a4c; --friction: #b86e00; --unguided: #c4322b; --skipped: #7a8794;
  --j1: #d8412f; --j2a: #2f6fdb; --j2b: #1b9aa6; --j2c: #6a49c9; --j5: #e08a16; --j6: #2e9a5a; --j3: #c43d8a; --j4: #8a7a2e;
  --display: "Overpass", "Helvetica Neue", Arial, sans-serif;
  --body: "Public Sans", "Helvetica Neue", Arial, sans-serif;
  --mono: "Overpass Mono", ui-monospace, Menlo, monospace;
  color-scheme: light;
}
@media (prefers-color-scheme: dark) {
  :root {
    --bg: #11161c; --panel: #181f27; --fg: #e4e9ee; --muted: #93a1af; --rule: #2b3640; --zone: #1c242d;
    --led: #4cc37f; --friction: #f0a33a; --unguided: #f06a60; --skipped: #8c99a6;
    --j1: #f06a58; --j2a: #6a9cf5; --j2b: #43c3cf; --j2c: #a38bf0; --j5: #f5a93f; --j6: #55c582; --j3: #ec6fb3; --j4: #c8b45a;
    color-scheme: dark;
  }
}
body { margin: 0; background: var(--bg); color: var(--fg); font-family: var(--body); font-size: 15px; line-height: 1.5; }
main { max-width: 1240px; margin: 0 auto; padding: 28px 20px 56px; display: grid; gap: 28px; }
header { display: grid; gap: 8px; }
.eyebrow { font-family: var(--display); font-weight: 700; font-size: 12px; letter-spacing: .14em; text-transform: uppercase; color: var(--muted); }
h1 { font-family: var(--display); font-weight: 800; font-size: clamp(28px, 4vw, 40px); line-height: 1.05; margin: 0; text-wrap: balance; }
.lede { max-width: 68ch; color: var(--muted); margin: 0; }
.facts { display: flex; flex-wrap: wrap; gap: 6px 22px; margin: 4px 0 0; padding: 0; list-style: none; font-variant-numeric: tabular-nums; }
.facts li { font-family: var(--display); font-weight: 700; font-size: 14px; }
.facts li span { font-family: var(--body); font-weight: 400; color: var(--muted); }
h2 { font-family: var(--display); font-weight: 800; font-size: 20px; margin: 0 0 4px; }
.sub { color: var(--muted); margin: 0 0 12px; max-width: 72ch; }
figure { margin: 0; background: var(--panel); border: 1px solid var(--rule); border-radius: 14px; padding: 14px; display: grid; gap: 12px; min-width: 0; }
.mapwrap { overflow-x: auto; }
.mapwrap svg { display: block; width: 100%; min-width: 900px; height: auto; color: var(--fg); }
figcaption { color: var(--muted); font-size: 14px; max-width: 80ch; }
.zone { fill: var(--zone); }
.zone-label { font-family: var(--display); font-weight: 800; font-size: 12px; letter-spacing: .16em; fill: var(--muted); }
.track { fill: none; stroke-width: 5; stroke-linecap: round; stroke-linejoin: round; transition: opacity .15s; }
.tag text { font-family: var(--display); font-weight: 800; font-size: 11px; fill: #fff; }
.station rect { fill: var(--panel); stroke: var(--fg); stroke-width: 2.5; }
.station.friction rect { stroke: var(--friction); stroke-width: 3.5; }
.station.unguided rect { stroke: var(--unguided); stroke-width: 3.5; }
.station.none rect, .station.skipped rect { stroke: var(--skipped); stroke-dasharray: 3 3; }
.slabel { font-family: var(--display); font-weight: 700; font-size: 13px; fill: currentColor; paint-order: stroke; stroke: var(--panel); stroke-width: 4px; stroke-linejoin: round; }
.snote { font-family: var(--display); font-weight: 700; font-size: 11px; letter-spacing: .06em; text-transform: uppercase; paint-order: stroke; stroke: var(--panel); stroke-width: 4px; }
.snote.friction { fill: var(--friction); } .snote.unguided { fill: var(--unguided); } .snote.skipped { fill: var(--skipped); }
svg.isolating .line:not(.on) .track, svg.isolating .tag:not(.on) { opacity: .12; }
@media (hover: hover) { .line:hover .track { stroke-width: 7; } }
.legend { display: flex; flex-wrap: wrap; gap: 6px; }
.chip { display: inline-flex; align-items: center; gap: 8px; font: inherit; font-size: 13px; color: var(--fg); background: transparent; border: 1px solid var(--rule); border-radius: 999px; padding: 4px 12px 4px 6px; cursor: pointer; text-align: left; }
.chip:hover { border-color: var(--muted); }
.chip[aria-pressed="true"] { border-color: var(--fg); background: var(--zone); }
.chip:focus-visible { outline: 2px solid var(--fg); outline-offset: 2px; }
.swatch { display: inline-block; width: 22px; height: 6px; border-radius: 3px; flex: none; }
.chip-name { font-family: var(--display); font-weight: 800; }
.chip-title { color: var(--muted); }
.gridwrap { overflow-x: auto; background: var(--panel); border: 1px solid var(--rule); border-radius: 14px; }
table { border-collapse: collapse; width: 100%; min-width: 720px; font-variant-numeric: tabular-nums; }
th, td { padding: 7px 10px; border-bottom: 1px solid var(--rule); }
thead th { font-family: var(--display); font-weight: 800; font-size: 13px; text-align: center; white-space: nowrap; }
thead th .swatch { width: 16px; height: 5px; display: block; margin: 0 auto 4px; }
tbody th { text-align: left; font-weight: 600; font-size: 14px; min-width: 220px; }
tr.zone-row th { font-family: var(--display); font-weight: 800; font-size: 11px; letter-spacing: .14em; text-transform: uppercase; color: var(--muted); background: var(--zone); padding-block: 5px; }
td.c { text-align: center; font-family: var(--display); font-weight: 800; font-size: 13px; }
td.led .mark { color: var(--led); } td.friction .mark { color: var(--friction); } td.unguided .mark { color: var(--unguided); }
td.skipped .mark { color: var(--skipped); font-size: 11px; letter-spacing: .06em; text-transform: uppercase; }
td.none::before { content: ""; display: inline-block; width: 10px; height: 2px; background: var(--rule); vertical-align: middle; }
tr.gap th { color: var(--muted); }
.why { display: block; font-weight: 400; font-size: 12.5px; }
.sr { position: absolute; width: 1px; height: 1px; overflow: hidden; clip: rect(0 0 0 0); white-space: nowrap; }
.key { display: flex; flex-wrap: wrap; gap: 6px 18px; font-size: 13px; color: var(--muted); margin-top: 10px; }
.key b { font-family: var(--display); }
.key .ok { color: var(--led); } .key .fr { color: var(--friction); } .key .sk { color: var(--skipped); } .key .un { color: var(--unguided); }
.notes { display: grid; grid-template-columns: repeat(auto-fit, minmax(280px, 1fr)); gap: 24px; }
.notes > div { min-width: 0; }
.notes ul { margin: 0; padding-left: 18px; display: grid; gap: 6px; }
code { font-family: var(--mono); font-size: 13px; }
@media (prefers-reduced-motion: reduce) { .track { transition: none; } }
</style>
</head>
<body>
<main>
  <header>
    <div class="eyebrow">Arugula · journeys #551, #664</div>
    <h1>Arugula Journey Map</h1>
    <p class="lede">Every journey is a line, every shared step a station. A station is outlined plainly when the screen led every person through it in the newest runs, in amber when a run noted friction there, in red when a step had nothing on screen leading to it.</p>
    <ul class="facts">${p.facts}</ul>
  </header>
  <figure>
    <div class="legend" role="group" aria-label="Show one journey">${p.legend}</div>
    <div class="mapwrap" id="map">${p.svg}</div>
    <figcaption>J2a, J2b and J2c run J1 for each person first, so they share its whole first run. J5 leaves J1 at the phone step and takes no account; J6 closes setup and stays on the tailnet. J3 and J4 end at the older daemon's update and at <code>arugula status</code>. A dashed line has no report yet (J3 and J4 run only on a Mac: <code>just macos journey-j3</code>, <code>journey-j4</code>). Click a journey to show it alone; hover a station for the steps behind it.</figcaption>
  </figure>
  <section>
    <h2>Coverage</h2>
    <p class="sub">Each station against each journey, and the parts of the product no journey reaches yet. J1 counts its real-Mac run (J1-mac) too; a J2 journey counts its people's first runs.</p>
    <div class="gridwrap"><table>
      <thead><tr><th scope="col" style="text-align:left">Station</th>${p.head}</tr></thead>
      <tbody>${p.rows}</tbody>
    </table></div>
    <div class="key"><span><b class="ok">✓</b> led by the screen</span><span><b class="fr">!</b> led, with friction noted</span><span><b class="un">✕</b> unguided</span><span><b class="sk">SKIP</b> taken only by skipping</span><span>— not in this journey</span></div>
  </section>
  <section class="notes">
    <div><h2>Friction noted</h2><ul>${p.friction}</ul></div>
    <div><h2>How this map is made</h2><ul>
      <li>From the reports each run writes in <code>web/journey-reports/*.json</code>: step id, person, surface, the prompt that led to it, and its result.</li>
      <li><code>web/e2e/journey/map.ts</code> maps every step id to a station; the build stops if a step has none.</li>
      <li>Station positions are placed by hand. Line colours and station states come from the reports.</li>
    </ul></div>
  </section>
</main>
<script>
(() => {
  const svg = document.querySelector("#map svg");
  const chips = [...document.querySelectorAll(".chip")];
  let on = null;
  chips.forEach((c) => c.addEventListener("click", () => {
    on = on === c.dataset.line ? null : c.dataset.line;
    svg.classList.toggle("isolating", on !== null);
    svg.querySelectorAll("[data-line]").forEach((g) => g.classList.toggle("on", g.dataset.line === on));
    chips.forEach((x) => x.setAttribute("aria-pressed", String(x.dataset.line === on)));
  }));
})();
</script>
</body>
</html>
`;
}

// Run as a script: build the map from the reports directory.
if (import.meta.url === `file://${process.argv[1]}`) {
  const dir = process.argv[2] ?? reports();
  if (!existsSync(dir)) {
    console.error(`no reports in ${dir}: run a journey first (just journey)`);
    process.exit(1);
  }
  try {
    const out = build(dir);
    writeFileSync(join(dir, "map.html"), out.html);
    console.log(`Journey map: ${out.runs} runs, ${out.steps} steps${out.missing.length ? `; no report yet for ${out.missing.join(", ")}` : ""}. ${join(dir, "map.html")}`);
  } catch (e) {
    console.error((e as Error).message);
    process.exit(1);
  }
}
