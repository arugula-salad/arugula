// The import rule refuses what it should (M148): each case builds a small
// web/src in a temporary folder and runs the check the build runs.
import { test } from "node:test";
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { checkBundle, checkSource } from "./check-imports.mjs";

const script = join(dirname(fileURLToPath(import.meta.url)), "check-imports.mjs");

/** A src tree with these files, and the rule's answer for it. */
function tree(files) {
  const src = mkdtempSync(join(tmpdir(), "check-imports-"));
  const base = {
    "shell/e2e/keys.ts": "export const sign = 1;",
    "shell/client.ts": 'import { sign } from "./e2e/keys";\nexport const client = sign;',
    "shell/control.ts": 'import { sign } from "./e2e/keys.ts";\nexport const control = sign;',
    "shell/fleet.ts": 'import { client } from "./client";\nexport const fleet = client;',
    "shell/main.tsx": 'import { room } from "../platform/chat/room";\nimport { tokens } from "../shared/tokens";\nconsole.log(room, tokens);',
    "shared/tokens.ts": "export const tokens = {};",
    "shared/style.css": "a { color: red; }",
    "platform/chat/room.tsx": 'import { tokens } from "../../shared/tokens";\nimport "../../shared/style.css";\nexport const room = tokens;',
  };
  for (const [p, text] of Object.entries({ ...base, ...files })) {
    mkdirSync(dirname(join(src, p)), { recursive: true });
    writeFileSync(join(src, p), text);
  }
  try {
    return checkSource(src);
  } finally {
    rmSync(src, { recursive: true, force: true });
  }
}

test("the tree as it should be passes", () => {
  assert.deepEqual(tree({}), []);
});

test("platform/ importing the key code fails", () => {
  const bad = tree({ "platform/chat/room.tsx": 'import { sign } from "../../shell/e2e/keys";\nexport const room = sign;' });
  assert.ok(bad.some((b) => b.includes("platform/chat/room.tsx imports shell/e2e/keys.ts (the key code")), bad.join("\n"));
});

test("platform/ importing the daemon client, control's trust client or the fleet fails, type imports too", () => {
  for (const [spec, what] of [
    ["../../shell/client", "the daemon client"],
    ["../../shell/control", "control's trust client"],
    ["../../shell/fleet", "the fleet"],
  ]) {
    const bad = tree({ "platform/chat/room.tsx": `import type { X } from "${spec}";\nexport type Y = X;` });
    assert.ok(bad.some((b) => b.includes(what)), `${spec}: ${bad.join("\n")}`);
  }
});

test("dynamic imports and export-from count", () => {
  assert.ok(tree({ "platform/a.ts": 'export const k = () => import("../shell/e2e/keys");' }).length);
  assert.ok(tree({ "platform/a.ts": 'export { sign } from "../shell/e2e/keys.ts";' }).length);
  assert.ok(tree({ "platform/a.ts": 'export type K = import("../shell/e2e/keys").K;' }).length);
});

test("shared/ importing shell/ fails, and the platform file reaching through it is named", () => {
  const bad = tree({
    "shared/tokens.ts": 'import { fleet } from "../shell/fleet";\nexport const tokens = fleet;',
  });
  assert.ok(bad.some((b) => b.startsWith("shared/tokens.ts imports shell/fleet.ts")), bad.join("\n"));
  assert.ok(bad.some((b) => b.includes("platform/chat/room.tsx reaches shell/fleet.ts") && b.includes("shared/tokens.ts")), bad.join("\n"));
});

test("shared/ importing platform/ fails", () => {
  assert.ok(tree({ "shared/tokens.ts": 'import { room } from "../platform/chat/room";\nexport const tokens = room;' }).some((b) => b.includes("shared/ imports neither")));
});

test("a file outside the three folders fails", () => {
  assert.ok(tree({ "loose.ts": "export {};" }).some((b) => b.startsWith("loose.ts")));
});

test("the shell may import the platform and shared", () => {
  assert.deepEqual(tree({ "shell/other.ts": 'import { room } from "../platform/chat/room";\nexport const x = room;' }), []);
});

test("the command exits 1 on a bad import and 0 on a good tree", () => {
  const src = mkdtempSync(join(tmpdir(), "check-imports-"));
  try {
    mkdirSync(join(src, "platform"), { recursive: true });
    mkdirSync(join(src, "shell/e2e"), { recursive: true });
    writeFileSync(join(src, "shell/e2e/keys.ts"), "export const sign = 1;");
    writeFileSync(join(src, "platform/a.ts"), 'export const a = 1;');
    execFileSync(process.execPath, [script, src], { stdio: "pipe" });
    writeFileSync(join(src, "platform/a.ts"), 'import { sign } from "../shell/e2e/keys";\nexport const a = sign;');
    assert.throws(() => execFileSync(process.execPath, [script, src], { stdio: "pipe" }), (e) => e.status === 1 && /shell\/e2e\/keys\.ts/.test(e.stderr.toString()));
  } finally {
    rmSync(src, { recursive: true, force: true });
  }
});

test("a bundle holding a key module or the shell's fails; the daemon's holding platform/ fails", () => {
  const src = "/w/web/src";
  assert.deepEqual(checkBundle(["/w/web/src/platform/main.tsx", "/w/web/src/shared/proto.ts", "/w/web/node_modules/preact/dist/preact.mjs", "\0vite/preload-helper.js"], "keys", src), []);
  assert.match(checkBundle(["/w/web/src/shell/e2e/keys.ts?used"], "keys", src)[0], /shell\/e2e\/keys\.ts \(the key code/);
  assert.match(checkBundle(["/w/web/src/shell/ui/menu.tsx"], "keys", src)[0], /the shell's/);
  assert.match(checkBundle(["/w/web/src/platform/pages/chat.tsx"], "platform", src)[0], /platform\/pages\/chat\.tsx/);
  assert.deepEqual(checkBundle(["/w/web/src/shell/client.ts"], "platform", src), []);
});
