#!/usr/bin/env node
// The contract's compatibility rule (contract/README.md, Arugula #807):
// contract/ in the working tree against contract/ at the merge-base with
// origin/main. Additions pass. It fails, naming each, on:
//
// - a removed route (contract/api/routes.json), or a client a route no longer
//   serves;
// - a removed operation (contract/openapi/native.json), or one whose request,
//   query or answer is now another type;
// - a removed type, field, enum value or variant (contract/schema/);
// - a field that became required, or a new required field on a type that was
//   there already;
// - a field whose type changed (its JSON types, $ref or format), being made
//   nullable included.
//
// A deliberate removal, once no supported version reads or sends it, names
// what it removes in a commit message line on the branch:
//
//   Contract-break: chat.ChatRoom.archived, last read by 0.70.3 (out of the window on 2026-12-10)
//
// which allows that finding and any under it (`chat.ChatRoom` covers its
// fields). The findings print as these ids.
//
//   node scripts/contract-compat.mjs [--base REF] [--root DIR]

import { execFileSync } from "node:child_process";
import { existsSync, readFileSync, readdirSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const arg = (name) => {
  const i = process.argv.indexOf(name);
  return i < 0 ? null : process.argv[i + 1];
};
const root = resolve(arg("--root") ?? join(dirname(fileURLToPath(import.meta.url)), ".."));
const git = (...a) => execFileSync("git", ["-C", root, ...a], { encoding: "utf8", maxBuffer: 64 << 20, stdio: ["ignore", "pipe", "pipe"] });
const die = (msg) => {
  console.error(`contract-compat: ${msg}`);
  process.exit(2);
};

let base = arg("--base");
if (!base) {
  try {
    base = git("merge-base", "HEAD", "origin/main").trim();
  } catch {
    die("no merge-base with origin/main: fetch it (`git fetch origin main`) or pass --base REF");
  }
}

/** contract/<path> at the base (null when it isn't there) and now. */
const atBase = (path) => {
  try {
    return JSON.parse(git("show", `${base}:contract/${path}`));
  } catch {
    return null;
  }
};
const atHead = (path) => (existsSync(join(root, "contract", path)) ? JSON.parse(readFileSync(join(root, "contract", path), "utf8")) : null);
const baseModules = () => {
  try {
    return git("ls-tree", "--name-only", `${base}:contract/schema`).split("\n").filter((f) => f.endsWith(".json"));
  } catch {
    return [];
  }
};

const findings = [];
const flag = (id, what) => findings.push({ id, what });

// Routes: each row still there, for every client it served.
const routeKey = (r) => `${r.method} ${r.path}`;
const baseRoutes = atBase("api/routes.json")?.routes ?? [];
const headRoutes = new Map((atHead("api/routes.json")?.routes ?? []).map((r) => [routeKey(r), r]));
for (const r of baseRoutes) {
  const now = headRoutes.get(routeKey(r));
  if (!now) flag(routeKey(r), "removed route");
  else for (const c of r.clients) if (!now.clients.includes(c)) flag(routeKey(r), `route no longer serves ${c}`);
}

// Operations: each still there, taking and answering the same types.
const refOf = (s) => s?.$ref?.replace("#/components/schemas/", "") ?? null;
const slots = (op) => {
  const out = {};
  for (const [ct, c] of Object.entries(op.requestBody?.content ?? {})) out.request = `${ct} ${refOf(c.schema) ?? c.schema?.format ?? ""}`.trim();
  const q = op.parameters?.find((p) => p.in === "query");
  if (q) out.query = refOf(q.schema);
  for (const code of ["2XX", "101"]) {
    for (const [ct, c] of Object.entries(op.responses?.[code]?.content ?? {})) out.response = `${ct} ${refOf(c.schema) ?? ""}`.trim();
  }
  return out;
};
const basePaths = atBase("openapi/native.json")?.paths ?? {};
const headPaths = atHead("openapi/native.json")?.paths ?? {};
for (const [path, methods] of Object.entries(basePaths)) {
  for (const [method, op] of Object.entries(methods)) {
    const id = `${method.toUpperCase()} ${path}`;
    const now = headPaths[path]?.[method];
    if (!now) {
      flag(id, "removed operation");
      continue;
    }
    const [was, is] = [slots(op), slots(now)];
    for (const [slot, v] of Object.entries(was)) {
      if (is[slot] !== v) flag(`${id} ${slot}`, `changed type: was ${v}, now ${is[slot] ?? "none"}`);
    }
  }
}

// Types: one node of a schema, normalized so that how schemars happens to
// write a thing (an enum's values as one `enum` or as `const`s with their
// docs, Option<T> as a nullable type or an anyOf with null) doesn't count
// as a change.
function shape(s) {
  const n = { types: new Set(), values: new Set(), refs: new Set(), format: s.format ?? null, props: null, required: new Set(), children: new Map() };
  const take = (v, i) => {
    if (v.$ref) return n.refs.add(v.$ref.replace("#/$defs/", ""));
    for (const t of [v.type ?? []].flat()) if (t !== "object" || !v.properties) n.types.add(t);
    if ("const" in v) n.values.add(JSON.stringify(v.const));
    for (const e of v.enum ?? []) n.values.add(JSON.stringify(e));
    if (!v.properties) return;
    // An object variant: by its tag (a property that's a const), else by
    // its one key (an externally tagged variant), else by its place.
    const tag = Object.entries(v.properties).find(([, p]) => "const" in p);
    const keys = Object.keys(v.properties);
    const key = tag ? `<${tag[0]}=${tag[1].const}>` : keys.length === 1 ? `<${keys[0]}>` : `<#${i}>`;
    n.children.set(key, v);
  };
  const variants = s.oneOf ?? s.anyOf;
  if (variants) variants.forEach(take);
  else if (s.properties) {
    n.types.add("object");
    n.props = s.properties;
    n.required = new Set(s.required ?? []);
  } else take(s, 0);
  if (s.items && !Array.isArray(s.items)) n.children.set("[]", s.items);
  (s.prefixItems ?? []).forEach((p, i) => n.children.set(`[${i}]`, p));
  if (s.additionalProperties && typeof s.additionalProperties === "object") n.children.set("{}", s.additionalProperties);
  return n;
}

const list = (set) => [...set].sort().join("|") || "none";
const sep = (key) => (key.startsWith("<") || key.startsWith("[") || key.startsWith("{") ? key : `.${key}`);

function compare(id, b, h) {
  const [was, is] = [shape(b), shape(h)];
  const sig = (n) => `${list(n.types)}${n.format ? ` (${n.format})` : ""}`;
  const lostRefs = [...was.refs].filter((r) => !is.refs.has(r));
  if (sig(was) !== sig(is) || lostRefs.length) {
    const say = (n) => [sig(n), ...[...n.refs].sort()].filter((x) => x !== "none").join(" | ") || "anything";
    flag(id, `changed type: was ${say(was)}, now ${say(is)}`);
  }
  // A value the reader no longer takes; no constraint at all takes any.
  if (is.values.size) for (const v of was.values) if (!is.values.has(v)) flag(id, `removed value ${v}`);
  if (was.props && is.props) {
    for (const name of Object.keys(was.props)) {
      if (!(name in is.props)) flag(`${id}.${name}`, "removed field");
      else compare(`${id}.${name}`, was.props[name], is.props[name]);
    }
    for (const name of is.required) {
      if (was.required.has(name)) continue;
      flag(`${id}.${name}`, name in was.props ? "became required" : "added as required: an older sender doesn't send it");
    }
  }
  for (const [key, v] of was.children) {
    if (!is.children.has(key)) flag(`${id}${sep(key)}`, key.startsWith("<") ? "removed variant" : "changed type: no longer has it");
    else compare(`${id}${sep(key)}`, v, is.children.get(key));
  }
}

const headDefs = new Map();
const headDir = join(root, "contract/schema");
for (const f of existsSync(headDir) ? readdirSync(headDir).filter((f) => f.endsWith(".json")) : []) {
  headDefs.set(f.replace(/\.json$/, ""), atHead(`schema/${f}`).$defs ?? {});
}
const anywhere = (name) => [...headDefs.values()].find((d) => name in d)?.[name];
for (const f of baseModules()) {
  const module = f.replace(/\.json$/, "");
  for (const [name, def] of Object.entries(atBase(`schema/${f}`)?.$defs ?? {})) {
    // A type a module stops carrying is only gone if no module has it.
    const now = headDefs.get(module)?.[name] ?? anywhere(name);
    if (!now) flag(`${module}.${name}`, "removed type");
    else compare(`${module}.${name}`, def, now);
  }
}

// What the branch's commit messages allow.
const allowed = [];
const bad = [];
for (const line of git("log", "--format=%B", `${base}..HEAD`).split("\n")) {
  const m = line.match(/^Contract-break:\s*(.*)$/);
  if (!m) continue;
  const [id, why] = [m[1].split(", ")[0].trim(), m[1].split(", ").slice(1).join(", ").trim()];
  if (!id || !why) bad.push(line);
  else allowed.push(id);
}
const covers = (a, id) => id === a || (id.startsWith(a) && /^[.<[{ ]/.test(id.slice(a.length)));

const seen = new Set();
let failed = 0;
for (const { id, what } of findings) {
  if (seen.has(`${id} ${what}`)) continue;
  seen.add(`${id} ${what}`);
  const allow = allowed.find((a) => covers(a, id));
  if (allow) console.log(`contract-compat: ${id}: ${what}, allowed by Contract-break: ${allow}`);
  else {
    console.error(`contract-compat: ${id}: ${what}`);
    failed++;
  }
}
for (const line of bad) console.error(`contract-compat: say what and why: "${line}" (Contract-break: ID, WHY)`);
const short = git("rev-parse", "--short", base).trim();
if (failed || bad.length) {
  if (failed) {
    console.error(`contract-compat: ${failed} incompatible change${failed === 1 ? "" : "s"} against ${short}. Changes are additive: deprecate first, and remove only once no supported version reads or sends it (contract/README.md, "Compatibility"); then name it in a commit message line, Contract-break: ID, WHY`);
  }
  process.exit(1);
}
console.log(`contract-compat: contract/ against ${short}: only additions${seen.size ? ` and ${seen.size} allowed` : ""}`);
