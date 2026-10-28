#!/usr/bin/env node
// What's new's headlines, from the headline section of each daemon
// release's notes (docs/releases/X.Y.Z.md). The web build reads them
// through `releases` (web/vite.config.ts), so a release's build has its
// own notes' headlines: the release's bump commit carries its notes onto
// the green commit (scripts/release bump). Run, it checks every release's
// section and lists them (`just whats-new`, in CI).
//
// The section is a fenced block, a JSON list of a few items, which
// `scripts/release publish` leaves out of the GitHub release:
//
//   ```whats-new
//   [
//     { "id": "project-docs", "title": "Project Docs", "line": "One line.",
//       "try": { "route": "/" }, "needs": "control" }
//   ]
//   ```
//
// - `id`: the feature's name for good, lowercase words and dashes.
// - `title`, `line`: a few words, and one sentence.
// - `try` (optional): what *Try it* opens: `{ "command": ID }`, an id of
//   `COMMANDS` in web/src/shell/ui/commands.tsx, or `{ "route": R }`, a
//   shell route (`#swarm`) or a path on the platform page (`/chat`).
// - `needs` (optional, one or a list): who may use it, else the item is
//   left out: `control` (signed in on control's page), `team` (on a
//   team), `hosted` (on the hosted workspaces' list), `flag:NAME` (a Labs
//   flag on for the machine shown).
//
// `scripts/release bump` refuses a daemon release whose notes have none.
import { readdirSync, readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const dir = "docs/releases";

const NEEDS = /^(control|team|hosted|flag:[a-z][a-z0-9-]*)$/;
const ID = /^[a-z0-9]+(-[a-z0-9]+)*$/;
const MAX_ITEMS = 5;

const fail = (file, what) => {
  throw new Error(`${dir}/${file}: ${what}`);
};
const str = (file, i, k, v, max) => {
  if (typeof v !== "string" || !v.trim()) fail(file, `item ${i}: no ${k}`);
  if (v.length > max) fail(file, `item ${i}: ${k} is over ${max} characters`);
  return v;
};

/** The headline items of one release's notes, or null without a section. */
function headlines(file, text) {
  const m = text.match(/^```whats-new\n([\s\S]*?)^```$/m);
  if (!m) return null;
  let list;
  try {
    list = JSON.parse(m[1]);
  } catch (e) {
    fail(file, `the whats-new block isn't JSON: ${e.message}`);
  }
  if (!Array.isArray(list)) fail(file, "the whats-new block is a list");
  if (list.length > MAX_ITEMS) fail(file, `more than ${MAX_ITEMS} headlines`);
  return list.map((it, i) => {
    if (!it || typeof it !== "object") fail(file, `item ${i} isn't an object`);
    const extra = Object.keys(it).filter((k) => !["id", "title", "line", "try", "needs"].includes(k));
    if (extra.length) fail(file, `item ${i}: unknown ${extra.join(", ")}`);
    if (typeof it.id !== "string" || !ID.test(it.id)) fail(file, `item ${i}: id is lowercase words and dashes`);
    const item = { id: it.id, title: str(file, i, "title", it.title, 60), line: str(file, i, "line", it.line, 200) };
    if (it.try !== undefined) {
      const t = it.try;
      const ok =
        t && typeof t === "object" && Object.keys(t).length === 1 &&
        ((typeof t.command === "string" && /^[a-z0-9.-]+$/.test(t.command)) || (typeof t.route === "string" && /^[#/]/.test(t.route)));
      if (!ok) fail(file, `item ${i}: try is { command: ID } or { route: "#…" | "/…" }`);
      item.try = t;
    }
    if (it.needs !== undefined) {
      const needs = Array.isArray(it.needs) ? it.needs : [it.needs];
      for (const n of needs) if (typeof n !== "string" || !NEEDS.test(n)) fail(file, `item ${i}: needs ${JSON.stringify(n)}: control, team, hosted or flag:NAME`);
      if (needs.length) item.needs = needs;
    }
    return item;
  });
}

const parts = (v) => v.split(".").map(Number);
const newer = (a, b) => {
  const [x, y] = [parts(a), parts(b)];
  for (let i = 0; i < 3; i++) if (x[i] !== y[i]) return y[i] - x[i];
  return 0;
};

/** Every release's headlines, newest first; throws on a section that's
 * wrong. `root` is the repository's. */
export function releases(root = join(dirname(fileURLToPath(import.meta.url)), "..")) {
  const out = readdirSync(join(root, dir))
    .filter((f) => /^\d+\.\d+\.\d+\.md$/.test(f))
    .map((file) => ({ version: file.slice(0, -3), items: headlines(file, readFileSync(join(root, dir, file), "utf8")) }))
    .filter((r) => r.items)
    .sort((a, b) => newer(a.version, b.version));
  const ids = new Map();
  for (const r of out)
    for (const it of r.items) {
      if (ids.has(it.id)) throw new Error(`${dir}: the headline id ${it.id} is in ${ids.get(it.id)} and ${r.version}`);
      ids.set(it.id, r.version);
    }
  return out;
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  try {
    for (const r of releases()) console.log(`${r.version}: ${r.items.map((it) => it.id).join(", ") || "(none)"}`);
  } catch (e) {
    console.error(e.message);
    process.exit(1);
  }
}
