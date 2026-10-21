// The platform page's paths (M150, "Platform URLs are ids with cosmetic
// slugs"): only the id is read, links carry the current slug, and anything
// else is nothing's address. Run with `node --experimental-strip-types`.
import { test } from "node:test";
import assert from "node:assert/strict";
import { idOf, parse, pathOf, slugOf, withSlug } from "../src/platform/paths.ts";

const P = "prj_8f2ec0ffee00112233445566778899aa";
const names = (id) => ({ [P]: "Arugula Team", ch_0123456789abcdef: "general" })[id];

test("only the id is read: any slug, or none, lands on the same project", () => {
  for (const seg of [P, `${P}-arugula`, `${P}-an-old-name`, encodeURIComponent(`${P}-ünïcode`)]) {
    assert.deepEqual(parse(`/p/${seg}`), { page: "project", project: P }, seg);
  }
});

test("each route parses", () => {
  assert.deepEqual(parse("/"), { page: "home" });
  assert.deepEqual(parse("/agents"), { page: "agents" });
  assert.deepEqual(parse("/settings"), { page: "settings" });
  assert.deepEqual(parse(`/p/${P}-x/chat`), { page: "chat", project: P });
  assert.deepEqual(parse(`/p/${P}/chat/ch_0123456789abcdef-general`, "?msg=12"), { page: "chat", project: P, room: "ch_0123456789abcdef", msg: 12 });
  assert.deepEqual(parse(`/p/${P}/chat/ch_0123456789abcdef`, "?msg=-3"), { page: "chat", project: P, room: "ch_0123456789abcdef", msg: -3 });
  assert.deepEqual(parse(`/p/${P}/chat/activity`), { page: "chat", project: P, view: "activity" });
  assert.deepEqual(parse(`/p/${P}/chat/search`, "?q=from%3A%40sam"), { page: "chat", project: P, view: "search", q: "from:@sam" });
  assert.deepEqual(parse("/dm/dm_00aa11bb22cc33dd-sam"), { page: "dm", room: "dm_00aa11bb22cc33dd" });
  assert.deepEqual(parse(`/p/${P}/workspaces/cw_0123abcd-app`), { page: "workspace", project: P, workspace: "cw_0123abcd" });
  assert.deepEqual(parse(`/p/${P}/workspaces/cw_0123abcd/cwc_4567ef01`), { page: "workspace", project: P, workspace: "cw_0123abcd", tab: "cwc_4567ef01" });
});

test("anything else is missing, never a guess", () => {
  for (const p of ["/p", "/p/arugula", `/p/${P}/nope`, "/dm/sam", `/p/${P}/workspaces/app`, `/p/${P}/workspaces/cw_0123abcd/tab`, "/settings/x", "/chat"]) {
    assert.equal(parse(p).page, "missing", p);
  }
  assert.equal(parse(`/p/${P}/chat/ch_0123456789abcdef`, "?msg=x").msg, undefined);
});

test("links carry the current slug, and read back as the same route", () => {
  const routes = [
    { page: "home" },
    { page: "project", project: P },
    { page: "chat", project: P, room: "ch_0123456789abcdef", msg: 4 },
    { page: "chat", project: P, view: "search", q: "has:quote deploy" },
    { page: "dm", room: "dm_00aa11bb22cc33dd" },
    { page: "workspace", project: P, workspace: "cw_0123abcd", tab: "cwc_4567ef01" },
  ];
  assert.equal(pathOf(routes[1], names), `/p/${P}-arugula-team`);
  assert.equal(pathOf(routes[2], names), `/p/${P}-arugula-team/chat/ch_0123456789abcdef-general?msg=4`);
  for (const r of routes) {
    const path = pathOf(r, names);
    const [p, q] = path.split("?");
    assert.deepEqual(parse(p, q ? `?${q}` : ""), r, path);
  }
});

test("slugs", () => {
  assert.equal(slugOf("Arugula Team!"), "arugula-team");
  assert.equal(slugOf("Crème brûlée"), "creme-brulee");
  assert.equal(slugOf("日本"), "");
  assert.equal(withSlug("ch_ab12", "日本"), "ch_ab12");
  assert.ok(slugOf("x".repeat(100)).length <= 40);
  assert.equal(idOf("cw_00ff-anything-at-all"), "cw_00ff");
  assert.equal(idOf("cw_zz"), null);
});
