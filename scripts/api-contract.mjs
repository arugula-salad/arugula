#!/usr/bin/env node
// The native client's API (#599): contract/openapi/native.json, written from
// contract/api/native.json (which route takes and answers which type),
// contract/api/routes.json (which routes exist, their auth and who may call
// them) and contract/schema/ (the types). Control serves it from its own
// repo, which writes routes.json from its routes and checks its handlers
// against native.json (arugula-salad/cloud's `just api-routes`); here the
// contract stands on its own. Without --check it writes the document; with
// it, it says so if the document is stale, and always fails if:
//
// - a route in scope isn't classified, as an operation or as out of scope;
// - an operation names a route routes.json doesn't have;
// - a type isn't in contract/schema/.

import { readFileSync, readdirSync, writeFileSync, mkdirSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const read = (p) => readFileSync(join(root, p), "utf8");
const OUT = "contract/openapi/native.json";
const ROUTES = "contract/api/routes.json";
const SCOPED_AUTH = new Set(["session", "session_or_client", "cloud_driver"]);

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

function build() {
  const map = JSON.parse(read("contract/api/native.json"));
  const routes = JSON.parse(read(ROUTES)).routes;
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

  // A route the gate lets a native app call is one the document describes.
  for (const r of routes) {
    if (r.clients.includes("native") && !map.operations[key(r)]) errors.push(`${key(r)} lists native in contract/api/routes.json but isn't in contract/api/native.json`);
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
      errors.push(`${k}: not in contract/api/routes.json`);
      continue;
    }
    if (!row.clients.includes("native")) errors.push(`${k}: its row in contract/api/routes.json doesn't list native`);
    const params = [...row.path.matchAll(/\{(\w+)\}/g)].map((m) => ({ name: m[1], in: "path", required: true, schema: { type: "string" } }));
    const operation = { operationId: k.replace(/[^A-Za-z0-9]+/g, "_").replace(/_$/, ""), "x-arugula-clients": row.clients, security: ["join_proof", "poll_token"].includes(row.auth) ? [] : [{ deviceSignature: [] }, { sessionCookie: [] }], responses: { default: { description: "Refused", content: { "application/json": { schema: { $ref: "#/components/schemas/Refusal" } } } } } };
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
    else if (op.upgrade) operation.responses["101"] = { description: "A WebSocket: Noise frames to and from the machine" };
    else if (op.untyped) operation.responses["2XX"] = { description: "OK" };
    else errors.push(`${k}: say its response, events, bytes, empty, upgrade or untyped`);
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
        deviceSignature: { type: "apiKey", in: "header", name: "x-arugula-auth", description: "A device's signature over the request (crates/control/src/auth.rs). A browser-kind device's counts only if it paired as a native app, on routes marked native, with control's --native-clients on (aru-131)." },
        sessionCookie: { type: "apiKey", in: "cookie", name: "session", description: "Control's page's session." },
      },
      schemas: Object.fromEntries([...schemas].sort(([a], [b]) => (a < b ? -1 : 1))),
    },
  };
  return { doc, errors };
}

const { doc, errors } = build();
const files = { [OUT]: `${JSON.stringify(doc, null, 2)}\n` };
const check = process.argv.includes("--check");
const stale = [];
if (check) {
  for (const [path, text] of Object.entries(files)) {
    let now = null;
    try {
      now = read(path);
    } catch {}
    if (now !== text) stale.push(path);
  }
}
if (errors.length || stale.length) {
  for (const e of errors) console.error(`api-contract: ${e}`);
  for (const path of stale) console.error(`api-contract: ${path} is stale: run \`just api-contract\``);
  process.exit(1);
}
if (!check) {
  for (const [path, text] of Object.entries(files)) {
    mkdirSync(join(root, dirname(path)), { recursive: true });
    writeFileSync(join(root, path), text);
  }
}
const ops = Object.values(doc.paths).flatMap((p) => Object.values(p));
console.log(`api-contract: ${ops.length} operations, ${ops.filter((o) => o["x-arugula-untyped"]).length} not fully typed, ${Object.keys(doc.components.schemas).length} types`);
