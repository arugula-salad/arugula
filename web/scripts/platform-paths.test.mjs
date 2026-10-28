// The platform page's paths (M150, "Platform URLs are ids with cosmetic
// slugs"): only the id is read, links carry the current slug, and anything
// else is nothing's address. Run with `node --experimental-strip-types`.
import { test } from "node:test";
import assert from "node:assert/strict";
import { TOP, idOf, parse, pathOf, slugOf, withSlug } from "../src/platform/paths.ts";

const P = "prj_8f2ec0ffee00112233445566778899aa";
const names = (id) => ({ [P]: "Arugula Team", ch_0123456789abcdef: "general" })[id];

test("only the id is read: any slug, or none, lands on the same project", () => {
  for (const seg of [P, `${P}-arugula`, `${P}-an-old-name`, encodeURIComponent(`${P}-ünïcode`)]) {
    assert.deepEqual(parse(`/p/${seg}`), { page: "project", project: P }, seg);
  }
});

test("each route parses", () => {
  assert.deepEqual(parse("/"), { page: "home" });
  assert.equal(parse("/agents").page, "missing", "the cloud agent library is gone");
  assert.deepEqual(parse("/inbox"), { page: "inbox" });
  assert.equal(pathOf({ page: "inbox" }), "/inbox");
  assert.deepEqual(parse("/settings"), { page: "settings" });
  assert.deepEqual(parse(`/p/${P}-x/chat`), { page: "chat", project: P });
  assert.deepEqual(parse(`/p/${P}/chat/ch_0123456789abcdef-general`, "?msg=12"), { page: "chat", project: P, room: "ch_0123456789abcdef", msg: 12 });
  assert.deepEqual(parse(`/p/${P}/chat/ch_0123456789abcdef`, "?msg=-3"), { page: "chat", project: P, room: "ch_0123456789abcdef", msg: -3 });
  assert.deepEqual(parse(`/p/${P}/chat/activity`), { page: "chat", project: P, view: "activity" });
  assert.deepEqual(parse(`/p/${P}/chat/saved`), { page: "chat", project: P, view: "saved" });
  assert.equal(pathOf({ page: "chat", project: P, view: "saved" }), `/p/${P}/chat/saved`);
  assert.deepEqual(parse(`/p/${P}/chat/search`, "?q=from%3A%40sam"), { page: "chat", project: P, view: "search", q: "from:@sam" });
  // A DM is its team's project's (UI audit #169); an id-only one finds it.
  assert.deepEqual(parse(`/p/${P}/dm/dm_00aa11bb22cc33dd-sam`), { page: "dm", project: P, room: "dm_00aa11bb22cc33dd" });
  assert.deepEqual(parse("/dm/dm_00aa11bb22cc33dd-sam", "?msg=3"), { page: "find", what: "chat", room: "dm_00aa11bb22cc33dd", msg: 3 });
  assert.deepEqual(parse(`/p/${P}/workspaces/hw_0123456789abcdef0123456789abcdef-app`), { page: "workspace", project: P, workspace: "hw_0123456789abcdef0123456789abcdef" });
  assert.deepEqual(parse(`/p/${P}-x/settings`), { page: "project-settings", project: P });
  assert.deepEqual(parse(`/p/${P}/settings`, "?link=acme%2Fapp"), { page: "project-settings", project: P, link: "acme/app" });
  assert.deepEqual(parse(`/p/${P}/settings`, "?link=..%2F..%2Fx"), { page: "project-settings", project: P }, "not a repo");
  assert.deepEqual(parse("/installed", `?state=${encodeURIComponent(`${P}:acme/app`)}&installation_id=9&setup_action=install`), { page: "installed", project: P, repo: "acme/app" });
  assert.deepEqual(parse("/installed", "?state=https%3A%2F%2Fevil.example%2F"), { page: "installed" }, "only a project and a repo");
  assert.deepEqual(parse("/installed"), { page: "installed" });
  const env = `env_${"0a".repeat(16)}`;
  assert.deepEqual(parse(`/p/${P}/deploys`), { page: "deploys", project: P });
  assert.deepEqual(parse(`/p/${P}/deploys/new`), { page: "new-deploy", project: P });
  assert.deepEqual(parse(`/p/${P}/deploys/new`, `?env=${env}`), { page: "new-deploy", project: P, env });
  assert.deepEqual(parse(`/p/${P}/deploys/new`, "?env=production"), { page: "new-deploy", project: P }, "an id only");
});

test("anything else is missing, never a guess", () => {
  for (const p of ["/p", "/p/arugula", `/p/${P}/nope`, "/dm/sam", `/p/${P}/workspaces/app`, `/p/${P}/workspaces/cw_0123abcd`, `/p/${P}/workspaces/hw_0123456789abcdef0123456789abcdef/tab`, `/p/${P}/past/run_00ff11ee`, "/settings/x", "/chats", `/p/${P}/deploys/old`, `/p/${P}/deploys/new/x`]) {
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
    { page: "dm", project: P, room: "dm_00aa11bb22cc33dd" },
    { page: "workspace", project: P, workspace: "hw_0123456789abcdef0123456789abcdef" },
    { page: "project-settings", project: P, link: "acme/app" },
    { page: "installed", project: P, repo: "acme/app" },
    { page: "deploys", project: P },
    { page: "new-deploy", project: P, env: `env_${"0a".repeat(16)}` },
  ];
  assert.equal(pathOf(routes[1], names), `/p/${P}-arugula-team`);
  assert.equal(pathOf(routes[2], names), `/p/${P}-arugula-team/chat/ch_0123456789abcdef-general?msg=4`);
  for (const r of routes) {
    const path = pathOf(r, names);
    const [p, q] = path.split("?");
    assert.deepEqual(parse(p, q ? `?${q}` : ""), r, path);
  }
});

test("every route starts with a segment in TOP, the server's 404 list (UI audit #193)", () => {
  const routes = [
    { page: "project", project: P },
    { page: "chat", project: P },
    { page: "dm", project: P, room: "dm_00aa11bb22cc33dd" },
    { page: "issues", project: P },
    { page: "runs", project: P },
    { page: "installed" },
    { page: "inbox" },
    { page: "settings" },
    { page: "deploy", deploy: `dep_${"0a".repeat(16)}` },
    { page: "find", what: "chat" },
    { page: "find", what: "chat", room: "dm_00aa11bb22cc33dd" },
    { page: "find", what: "issue", issue: "iss_0123456789abcdef" },
    { page: "find", what: "run", run: "run_0123456789abcdef" },
  ];
  for (const r of routes) {
    const first = pathOf(r).split(/[/?]/)[1];
    assert.ok(TOP.includes(first), `${pathOf(r)}: ${first} isn't in TOP`);
    assert.notEqual(parse(pathOf(r).split("?")[0]).page, "missing", pathOf(r));
  }
  for (const first of TOP) assert.ok(/^[a-z]+$/.test(first), first);
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

test("a project's hosted workspaces", () => {
  assert.deepEqual(parse(`/p/${P}/workspaces`), { page: "workspaces", project: P });
  assert.deepEqual(parse(pathOf({ page: "workspaces", project: P })), { page: "workspaces", project: P });
});

test("runs and a run (M215)", () => {
  const R = "run_0123456789abcdef0123456789abcdef";
  const S = "rsp_0123456789abcdef0123456789abcdef";
  assert.deepEqual(parse(`/p/${P}-x/runs`), { page: "runs", project: P });
  assert.deepEqual(parse(`/p/${P}/runs`, `?state=failed&trigger=schedule&spec=${S}`), { page: "runs", project: P, state: "failed", trigger: "schedule", spec: S });
  assert.deepEqual(parse(`/p/${P}/runs`, "?state=Fail%20ed&spec=..%2Fx"), { page: "runs", project: P }, "only words and a spec's id");
  assert.deepEqual(parse(`/p/${P}/runs/${R}`, "?msg=3"), { page: "run", project: P, run: R, msg: 3 });
  assert.equal(parse(`/p/${P}/runs/iss_0123abcd`).page, "missing");
  assert.equal(parse(`/p/${P}/runs/${R}/x`).page, "missing");
  assert.deepEqual(parse(`/runs/${R}`), { page: "find", what: "run", run: R });
  assert.equal(parse("/runs/cw_0123abcd").page, "missing");
  for (const r of [
    { page: "runs", project: P, state: "failed", trigger: "manual", spec: S },
    { page: "run", project: P, run: R, msg: 4 },
    { page: "find", what: "run", run: R },
  ]) {
    assert.deepEqual(parse(...pathOf(r, names).split(/(?=\?)/)), r);
  }
  assert.equal(pathOf({ page: "run", project: P, run: R }, names), `/p/${P}-arugula-team/runs/${R}`);
});

test("id-only addresses, for links that don't know the project", () => {
  assert.deepEqual(parse("/chat"), { page: "find", what: "chat" });
  assert.deepEqual(parse("/chat/ch_0123456789abcdef", "?msg=7"), { page: "find", what: "chat", room: "ch_0123456789abcdef", msg: 7 });
  for (const p of ["/chat/general", "/workspaces", "/workspaces/cw_0123abcd", "/past/run_00ff11ee", "/chat/a/b"]) assert.equal(parse(p).page, "missing", p);
  for (const r of [{ page: "find", what: "chat", room: "ch_0123456789abcdef", msg: 7 }]) {
    const [p, q] = pathOf(r).split("?");
    assert.deepEqual(parse(p, q ? `?${q}` : ""), r);
  }
});

test("an agent session (M257)", () => {
  const S = "ses_0123456789abcdef0123456789abcdef";
  assert.deepEqual(parse(`/p/${P}/sessions/${S}`), { page: "session", project: P, session: S });
  assert.deepEqual(parse(pathOf({ page: "session", project: P, session: S })), { page: "session", project: P, session: S });
  for (const p of [`/p/${P}/sessions/run_00ff11ee`, `/p/${P}/sessions/${S}/x`]) assert.equal(parse(p).page, "missing", p);
  // The project's sessions as a wave (#267).
  assert.deepEqual(parse(pathOf(parse(`/p/${P}/sessions`))), parse(`/p/${P}/sessions`));
  assert.equal(parse(`/p/${P}/sessions`).page, "sessions");
});

test("a project's Docs and one Doc (aru-118)", () => {
  const D = "pdc_0123456789abcdef0123456789abcdef";
  assert.deepEqual(parse(`/p/${P}/docs`), { page: "docs", project: P });
  assert.deepEqual(parse(`/p/${P}/docs/${D}`), { page: "doc", project: P, doc: D });
  assert.deepEqual(parse(`/p/${P}/docs/${D}-release-plan`), { page: "doc", project: P, doc: D });
  assert.equal(pathOf({ page: "doc", project: P, doc: D }, (id) => (id === D ? "Release plan" : undefined)), `/p/${P}/docs/${D}-release-plan`);
  for (const p of [`/p/${P}/docs/brd_00ff11ee`, `/p/${P}/docs/${D}/x`]) assert.equal(parse(p).page, "missing", p);
});
