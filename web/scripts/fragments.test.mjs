// The shell's fragments (M154, "Secrets stay in fragments"): each old route
// of the platform's lands on its new path, forever; the secret-bearing ones
// are never touched; and every file listening for `hashchange` is in the
// one list (web/src/shell/fragments.ts). Run with
// `node --experimental-strip-types`.
import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync, readdirSync, statSync } from "node:fs";
import { dirname, join, relative } from "node:path";
import { fileURLToPath } from "node:url";
import { FRAGMENTS, HASH_LISTENERS, platformPath } from "../src/shell/fragments.ts";
import { parse } from "../src/platform/paths.ts";

const src = join(dirname(fileURLToPath(import.meta.url)), "..", "src");

test("each old route of the platform's, with real parameters, lands on its new path", () => {
  const cases = [
    ["#chat", "/chat"],
    ["#chat=ch_0123456789abcdef", "/chat/ch_0123456789abcdef"],
    ["#chat=ch_0123456789abcdef&msg=42", "/chat/ch_0123456789abcdef?msg=42"],
    ["#chat=dm_00aa11bb22cc33dd&msg=-3", "/chat/dm_00aa11bb22cc33dd?msg=-3"],
    ["#chat=th_0123456789abcdef0123", "/chat/th_0123456789abcdef0123"],
    ["#chat=activity", "/chat/activity"],
    ["#chat=search/from%3A%40sam%20deploy", "/chat/search?q=from%3A%40sam%20deploy"],
    ["#chat=search/", "/chat/search"],
    // A machine's own thread, from before M125: chat, from the top.
    ["#chat=laptop/pane-3", "/chat"],
  ];
  for (const [hash, path] of cases) {
    assert.equal(platformPath(hash), path, hash);
    // …and the platform page reads that path as an address of its own.
    const [p, q] = path.split("?");
    assert.notEqual(parse(p, q ? `?${q}` : "").page, "missing", path);
  }
});

test("the secret-bearing fragments, and the shell's own, are never redirected", () => {
  for (const hash of [
    "#link=00aa.11bb.22cc.33dd",
    "#join=ABCD-EFGH.secret",
    "#device=WXYZ-1234.secret",
    "#pinvite=team.seed",
    "#askinvite=team.seed",
    "#invite=team.code",
    "#k=0123456789abcdef",
    "#app-redeem=aa.bb.cc",
    "#app=0123456789abcdef",
    "#platform-signin=cmV0dXJu",
    "#swarm",
    "#overview=2",
    "#agents",
    "#pane=3",
    "#getting-started=cloud",
    "#lend=on",
    "#chatty",
    "#cloudy",
    // Managoat's Cloud workspaces went, and their old links with them.
    "#cloud-ws",
    "#cloud-ws=cw_0123abcd",
    "#cloud=run_00ff11ee22",
  ]) {
    assert.equal(platformPath(hash), null, hash);
  }
  for (const f of FRAGMENTS) assert.equal(platformPath(f.route), null, f.route);
});

test("every file listening for hashchange is in the one list, saying why", () => {
  const files = [];
  (function walk(d) {
    for (const n of readdirSync(d)) {
      const p = join(d, n);
      if (statSync(p).isDirectory()) walk(p);
      else if (/\.tsx?$/.test(n) && readFileSync(p, "utf8").includes('addEventListener("hashchange"')) files.push(relative(src, p).split("\\").join("/"));
    }
  })(src);
  const listed = new Set(HASH_LISTENERS.map((l) => l.where));
  for (const f of files) assert.ok(listed.has(f), `${f} listens for hashchange: say why in shell/fragments.ts`);
  for (const l of HASH_LISTENERS) assert.ok(files.includes(l.where), `${l.where} is listed but doesn't listen`);
  for (const f of FRAGMENTS) assert.ok(f.why.length > 10 && f.where.length > 0, f.route);
});
