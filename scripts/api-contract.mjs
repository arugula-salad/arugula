#!/usr/bin/env node
// The native client's API (#599): contract/openapi/native.json, written from
// contract/api/native.json (which route takes and answers which type),
// crates/control/routes.toml (which routes exist, who calls them) and
// contract/schema/ (the types). Without --check it writes the document; with
// it, it says so if the document is stale, and always fails if:
//
// - a route in scope isn't classified, as an operation or as out of scope;
// - an operation names a route routes.toml doesn't have;
// - a type isn't in contract/schema/;
// - a handler's signature doesn't mention the types its operation names.
//
// It's a script, not part of crates/control-wire or a new crate, because
// scripts/ci-hub-changed counts any change to control-wire, routes.toml or a
// crate's Cargo.toml as a hub change, and a hub deploy drops every relayed
// terminal.

import { readFileSync, readdirSync, writeFileSync, mkdirSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const read = (p) => readFileSync(join(root, p), "utf8");
const OUT = "contract/openapi/native.json";
const SCOPED_AUTH = new Set(["session", "session_or_client", "cloud_driver"]);

/** routes.toml's rows. Its shape is flat: `[[route]]`, then `key = "text"` or
 * `key = ["a", "b"]`, each on a line, with comments after a value. */
export function parseRoutes(text) {
  const rows = [];
  let row = null;
  for (const line of text.split("\n")) {
    if (line.trim() === "[[route]]") rows.push((row = {}));
    else if (row) {
      const m = /^(method|path|auth|clients) = (.*)$/.exec(line);
      if (!m) continue;
      if (m[1] === "clients") row.clients = [...m[2].matchAll(/"([^"]+)"/g)].map((c) => c[1]);
      else row[m[1]] = /^"([^"]*)"/.exec(m[2])[1];
    }
  }
  return rows;
}

/** Every type in contract/schema/, under one name each. A name two modules
 * define differently is `module.Name`; `canon(module, name)` says which. */
function loadTypes() {
  const byName = new Map();
  const dir = join(root, "contract/schema");
  for (const f of readdirSync(dir).filter((f) => f.endsWith(".json")).sort()) {
    const module = f.replace(/\.json$/, "");
    for (const [name, schema] of Object.entries(JSON.parse(readFileSync(join(dir, f), "utf8")).$defs ?? {})) {
      if (!byName.has(name)) byName.set(name, []);
      byName.get(name).push({ module, schema, text: JSON.stringify(schema) });
    }
  }
  const clash = (name) => new Set(byName.get(name).map((d) => d.text)).size > 1;
  const canon = (module, name) => (clash(name) ? `${module}.${name}` : name);
  const find = (ref) => {
    const dot = ref.indexOf(".");
    if (dot < 0) return byName.has(ref) && !clash(ref) ? { name: ref, def: byName.get(ref)[0] } : null;
    const [module, name] = [ref.slice(0, dot), ref.slice(dot + 1)];
    const def = byName.get(name)?.find((d) => d.module === module);
    return def ? { name: canon(module, name), def } : null;
  };
  return { find, canon, clash: (n) => byName.has(n) && clash(n) };
}

/** A schema with its `#/$defs/X` as `#/components/schemas/…`, noting each. */
function rewrite(v, module, canon, found) {
  if (Array.isArray(v)) return v.map((i) => rewrite(i, module, canon, found));
  if (v === null || typeof v !== "object") return v;
  const out = {};
  for (const [k, i] of Object.entries(v)) {
    if (k === "$ref" && typeof i === "string" && i.startsWith("#/$defs/")) {
      const name = i.slice("#/$defs/".length);
      found.push(`${canon(module, name)}|${module}|${name}`);
      out.$ref = `#/components/schemas/${canon(module, name)}`;
    } else out[k] = rewrite(i, module, canon, found);
  }
  return out;
}

/** A handler's parameters and return type as written, or null: its file's
 * `pub async fn`s of that name, or failing that any `fn` of that name. */
function signature(file, fn) {
  let text;
  try {
    text = read(file);
  } catch {
    return null;
  }
  const all = (re) =>
    [...text.matchAll(re)].map((m) => {
      // Past the parameters, whose patterns can hold braces (`Person { account }`).
      let at = text.indexOf("(", m.index + m[0].length);
      for (let depth = 1; depth > 0 && at < text.length - 1; ) {
        at += 1;
        if (text[at] === "(") depth += 1;
        if (text[at] === ")") depth -= 1;
      }
      const open = text.indexOf("{", at);
      return text.slice(m.index, open < 0 ? undefined : open);
    });
  const handlers = all(new RegExp(`pub(?:\\([a-z]+\\))? async fn ${fn}\\b`, "g"));
  const found = handlers.length ? handlers : all(new RegExp(`\\bfn ${fn}\\b`, "g"));
  return found.length ? found.join("\n") : null;
}

function build() {
  const map = JSON.parse(read("contract/api/native.json"));
  const routes = parseRoutes(read("crates/control/routes.toml"));
  const types = loadTypes();
  const errors = [];
  const key = (r) => `${r.method} ${r.path}`;
  const listed = new Map(routes.map((r) => [key(r), r]));
  const scope = map.scope.map((s) => new RegExp(s));
  const out = map.out.map((o) => ({ ...o, re: new RegExp(o.match) }));

  // Every route in scope is classified.
  for (const r of routes) {
    if (!SCOPED_AUTH.has(r.auth) || !scope.some((s) => s.test(r.path))) continue;
    const isOp = key(r) in map.operations;
    const isOut = out.some((o) => o.re.test(r.path));
    if (!isOp && !isOut) {
      errors.push(`${key(r)} is in scope but not classified: add it to contract/api/native.json's operations, or to "out" with why`);
    }
    if (isOp && isOut) errors.push(`${key(r)} is both an operation and out of scope`);
  }

  const schemas = new Map();
  const queue = [];
  const use = (ref, where) => {
    const t = types.find(ref);
    if (!t) {
      errors.push(`${where}: no type ${ref} in contract/schema/${types.clash(ref) ? " (two modules define it: write module.Name)" : ""}`);
      return null;
    }
    queue.push(`${t.name}|${t.def.module}|${ref.split(".").pop()}`);
    return { $ref: `#/components/schemas/${t.name}` };
  };

  const paths = {};
  for (const [k, op] of Object.entries(map.operations)) {
    const row = listed.get(k);
    if (!row) {
      errors.push(`${k}: not in crates/control/routes.toml`);
      continue;
    }
    const [file, fn] = op.handler.split("#");
    const sig = signature(file, fn);
    if (sig === null) errors.push(`${k}: no fn ${fn} in ${file}`);
    else {
      for (const t of [op.request, op.query, op.response].filter(Boolean)) {
        const bare = t.split(".").pop();
        if (!new RegExp(`\\b${bare}\\b`).test(sig)) errors.push(`${k}: ${file}#${fn}'s signature doesn't mention ${bare}`);
      }
    }
    const params = [...row.path.matchAll(/\{(\w+)\}/g)].map((m) => ({ name: m[1], in: "path", required: true, schema: { type: "string" } }));
    const operation = { operationId: k.replace(/[^A-Za-z0-9]+/g, "_").replace(/_$/, ""), "x-arugula-clients": row.clients, security: [{ deviceSignature: [] }, { sessionCookie: [] }], responses: { default: { description: "Refused", content: { "application/json": { schema: { $ref: "#/components/schemas/Refusal" } } } } } };
    if (op.query) {
      const schema = use(op.query, k);
      if (schema) params.push({ name: "query", in: "query", style: "form", explode: true, schema });
    }
    if (params.length) operation.parameters = params;
    if (op.request) {
      const schema = use(op.request, k);
      if (schema) operation.requestBody = { required: true, content: { "application/json": { schema } } };
    }
    if (op.upload) operation.requestBody = { required: true, content: { "application/octet-stream": { schema: { type: "string", format: "binary" } } } };
    if (op.response) {
      const schema = use(op.response, k);
      if (schema) operation.responses["2XX"] = { description: "OK", content: { "application/json": { schema } } };
    } else if (op.events) {
      const schema = use(op.events, k);
      if (schema) operation.responses["2XX"] = { description: "Server-sent events, one JSON object each", content: { "text/event-stream": { schema } } };
    } else if (op.bytes) operation.responses["2XX"] = { description: op.bytes };
    else if (op.empty) operation.responses["2XX"] = { description: "OK, with no body" };
    else if (op.untyped) operation.responses["2XX"] = { description: "OK" };
    else errors.push(`${k}: say its response, events, bytes, empty or untyped`);
    if (op.untyped) operation["x-arugula-untyped"] = op.untyped;
    (paths[row.path] ??= {})[row.method.toLowerCase()] = operation;
  }

  // The types the operations reach, and what they carry.
  while (queue.length) {
    const [name, module, bare] = queue.shift().split("|");
    if (schemas.has(name)) continue;
    const def = types.find(`${module}.${bare}`).def;
    const refs = [];
    schemas.set(name, rewrite(def.schema, module, types.canon, refs));
    queue.push(...refs);
  }

  schemas.set("Refusal", {
    description: "What every refusal answers (crates/control-common/src/error.rs).",
    type: "object",
    properties: { error: { type: "string" }, reason: { type: "string", description: "A code a client can act on." } },
    required: ["error"],
  });

  const doc = {
    openapi: "3.1.0",
    info: {
      title: "Arugula native client API",
      version: "0",
      description: "What a native client calls on control. Generated by scripts/api-contract.mjs: edit contract/api/native.json. Not a public API: it can change with any release, by adding to it (aru-076, aru-131).",
    },
    servers: [{ url: "https://control.arugula.io" }],
    paths: Object.fromEntries(Object.entries(paths).sort(([a], [b]) => (a < b ? -1 : 1))),
    components: {
      securitySchemes: {
        deviceSignature: { type: "apiKey", in: "header", name: "x-arugula-auth", description: "A device's signature over the request (crates/control/src/auth.rs). A browser-kind device's counts only on routes marked native (#600), which aren't yet." },
        sessionCookie: { type: "apiKey", in: "cookie", name: "session", description: "Control's page's session." },
      },
      schemas: Object.fromEntries([...schemas].sort(([a], [b]) => (a < b ? -1 : 1))),
    },
  };
  return { doc, errors };
}

const { doc, errors } = build();
const text = `${JSON.stringify(doc, null, 2)}\n`;
const check = process.argv.includes("--check");
let stale = false;
if (check) {
  let now = null;
  try {
    now = read(OUT);
  } catch {}
  stale = now !== text;
}
if (errors.length || stale) {
  for (const e of errors) console.error(`api-contract: ${e}`);
  if (stale) console.error(`api-contract: ${OUT} is stale: run \`just api-contract\``);
  process.exit(1);
}
if (!check) {
  mkdirSync(join(root, dirname(OUT)), { recursive: true });
  writeFileSync(join(root, OUT), text);
}
const ops = Object.values(doc.paths).flatMap((p) => Object.values(p));
console.log(`api-contract: ${ops.length} operations, ${ops.filter((o) => o["x-arugula-untyped"]).length} not fully typed, ${Object.keys(doc.components.schemas).length} types`);
