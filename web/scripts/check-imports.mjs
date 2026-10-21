// The web client's import rule (M148, "The platform page is on
// app.arugula.io, not on the keys' origin"): web/src is shell/, platform/
// and shared/, and the platform page can't reach the code that holds or uses
// device keys.
//
// - platform/ imports only platform/ and shared/ (and packages);
// - shared/ imports only shared/ (and packages);
// - so nothing in platform/ reaches the key modules below, directly or
//   through shared/, which is checked on the whole import graph too.
//
// Every import counts: static, dynamic, `export … from`, `import type`,
// TypeScript's `import("…")` types and CSS `@import`.
//
//   node scripts/check-imports.mjs [src]
//     checks the source tree (default: src next to this script).
//   node scripts/check-imports.mjs --bundle <modules.json> --forbid keys|platform [src]
//     checks a build's module ids (a JSON list of paths, which the platform
//     page's and the shell's builds write): `keys` refuses the key modules
//     and anything else of the shell's, `platform` refuses platform/.
import { readFileSync, readdirSync, statSync } from "node:fs";
import { dirname, join, relative, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";

/** The modules that hold or use device keys, or reach machines with them:
 * the platform page never loads one. Paths under src, a trailing "/" for
 * a folder. */
export const KEY_MODULES = {
  "shell/e2e/": "the key code: device keys, cert signing, the noise channel",
  "shell/block/": "block origins' channel, which signs with device keys",
  "shell/client.ts": "the daemon client",
  "shell/control.ts": "control's trust client (device-signed calls)",
  "shell/fleet.ts": "the fleet",
};

const posix = (p) => p.split(sep).join("/");
const SPEC = /(?:\b(?:import|export)\s[^;]*?\bfrom\s*|\bimport\s*\(\s*|\bimport\s+|@import\s+(?:url\()?)(["'])([^"']+)\1/g;
const EXTS = ["", ".ts", ".tsx", ".js", ".mjs", "/index.ts", "/index.tsx", ".css"];

function walk(dir, out = []) {
  for (const name of readdirSync(dir)) {
    const p = join(dir, name);
    if (statSync(p).isDirectory()) walk(p, out);
    else if (/\.(tsx?|css)$/.test(name)) out.push(p);
  }
  return out;
}

/** Each file's imports inside src: { "platform/a.tsx": ["shared/b.ts", …] }. */
export function graph(src) {
  const files = walk(src);
  const known = new Set(files);
  const g = {};
  for (const f of files) {
    // Comments aren't stripped: an import named in one counts, which errs
    // on the side of refusing.
    const text = readFileSync(f, "utf8");
    const deps = new Set();
    for (const m of text.matchAll(SPEC)) {
      const spec = m[2];
      if (!spec.startsWith(".")) continue;
      const base = resolve(dirname(f), spec);
      const hit = EXTS.map((e) => base + e).find((p) => known.has(p));
      deps.add(hit ? posix(relative(src, hit)) : `?${spec}`);
    }
    g[posix(relative(src, f))] = [...deps];
  }
  return g;
}

const side = (p) => p.split("/")[0];
const keyOf = (p) => Object.keys(KEY_MODULES).find((k) => (k.endsWith("/") ? p.startsWith(k) : p === k));
const why = (p) => (keyOf(p) ? ` (${KEY_MODULES[keyOf(p)]})` : "");

/** The rule over a source tree: a list of what breaks it, empty when none. */
export function checkSource(src) {
  const g = graph(src);
  const bad = [];
  for (const [f, deps] of Object.entries(g)) {
    const from = side(f);
    if (!["shell", "platform", "shared"].includes(from)) {
      bad.push(`${f}: every file is in shell/, platform/ or shared/`);
      continue;
    }
    for (const d of deps) {
      if (d.startsWith("?")) continue;
      const to = side(d);
      if (from === "shared" && to !== "shared") bad.push(`${f} imports ${d}${why(d)}: shared/ imports neither shell/ nor platform/`);
      if (from === "platform" && to === "shell") bad.push(`${f} imports ${d}${why(d)}: platform/ imports only platform/ and shared/`);
    }
  }
  // The whole graph: no platform file reaches a key module by any path.
  for (const f of Object.keys(g).filter((f) => side(f) === "platform")) {
    const seen = new Map([[f, null]]);
    const queue = [f];
    while (queue.length) {
      const at = queue.shift();
      for (const d of g[at] ?? []) {
        if (d.startsWith("?") || seen.has(d)) continue;
        seen.set(d, at);
        if (keyOf(d)) {
          const path = [d];
          for (let p = at; p; p = seen.get(p)) path.unshift(p);
          bad.push(`${f} reaches ${d}${why(d)}: ${path.join(" → ")}`);
        } else queue.push(d);
      }
    }
  }
  return [...new Set(bad)];
}

/** A build's module ids against what it may not hold. */
export function checkBundle(modules, forbid, src) {
  const bad = [];
  for (const id of modules) {
    const clean = posix(id.replace(/^\0/, "").replace(/\?.*$/, ""));
    const rel = posix(relative(src, clean));
    if (rel.startsWith("..")) continue;
    if (forbid === "keys" && side(rel) === "shell") bad.push(`the build holds ${rel}${why(rel) || " (the shell's)"}`);
    if (forbid === "platform" && side(rel) === "platform") bad.push(`the build holds ${rel} (the platform page's)`);
  }
  return bad;
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const args = process.argv.slice(2);
  const at = (flag) => {
    const i = args.indexOf(flag);
    return i < 0 ? null : args.splice(i, 2)[1];
  };
  const bundle = at("--bundle");
  const forbid = at("--forbid");
  const src = resolve(args[0] ?? join(dirname(fileURLToPath(import.meta.url)), "..", "src"));
  const bad = bundle ? checkBundle(JSON.parse(readFileSync(bundle, "utf8")), forbid, src) : checkSource(src);
  for (const b of bad) console.error(b);
  if (bundle && !["keys", "platform"].includes(forbid)) {
    console.error("--bundle needs --forbid keys or --forbid platform");
    process.exit(2);
  }
  process.exit(bad.length ? 1 : 0);
}
