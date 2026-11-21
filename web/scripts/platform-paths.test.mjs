// The platform page's paths (M150, "Platform URLs are ids with cosmetic
// slugs"): only the id is read, links carry the current slug, and anything
// else is nothing's address. Run with `node --experimental-strip-types`.
import { test } from "node:test";
import assert from "node:assert/strict";
import { TOP, chatViewIn, idOf, parse, pathOf, slugOf, withSlug } from "../src/platform/paths.ts";

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
  assert.deepEqual(parse("/boards"), { page: "all-boards" });
  assert.equal(pathOf({ page: "all-boards" }), "/boards");
  // A board selected on every project's Boards (Arugula #1131), All kept.
  assert.deepEqual(parse("/boards/brd_0a01"), { page: "all-boards", board: "brd_0a01" });
  assert.deepEqual(parse("/boards/brd_0a01", "?all=1"), { page: "all-boards", board: "brd_0a01", all: true });
  assert.equal(pathOf({ page: "all-boards", board: "brd_0a01", all: true }), "/boards/brd_0a01?all=1");
  assert.equal(parse("/boards/pdc_0a01").page, "missing");
  assert.deepEqual(parse("/docs"), { page: "docs" });
  assert.equal(pathOf({ page: "docs" }), "/docs");
  // Settings (Arugula #1104): four scopes, and a section in each.
  assert.deepEqual(parse("/settings"), { page: "settings", scope: "you" });
  assert.equal(pathOf({ page: "settings", scope: "you" }), "/settings");
  assert.deepEqual(parse("/settings/you/quiet-hours"), { page: "settings", scope: "you", section: "quiet-hours" });
  assert.equal(pathOf({ page: "settings", scope: "you", section: "quiet-hours" }), "/settings/you/quiet-hours");
  assert.deepEqual(parse("/settings/machine"), { page: "settings", scope: "machine" });
  assert.deepEqual(parse("/settings/machine/0a1b2c3d-geek/machine"), { page: "settings", scope: "machine", machine: "0a1b2c3d", section: "machine" });
  assert.deepEqual(parse("/settings/team/personal/plan"), { page: "settings", scope: "team", owner: "personal", section: "plan" });
  assert.equal(pathOf({ page: "settings", scope: "team", owner: "team_1" }), "/settings/team/team_1");
  assert.equal(pathOf({ page: "settings", scope: "team", owner: "personal", section: "defaults" }), "/settings/team/personal/defaults");
  // The old addresses land in their scopes: a team's Settings tab, Personal's.
  assert.deepEqual(parse("/t/team_1/settings"), { page: "settings", scope: "team", owner: "team_1" });
  assert.deepEqual(parse("/settings/personal"), { page: "settings", scope: "team", owner: "personal" });
  // An old link's `#section` is the section, where the path names none.
  assert.deepEqual(parse("/settings", "", "#tokens"), { page: "settings", scope: "you", section: "tokens" });
  assert.deepEqual(parse(`/p/${P}/settings`, "", "#ci-alerts"), { page: "settings", scope: "project", project: P, section: "ci-alerts" });
  assert.deepEqual(parse("/settings/you/linked", "", "#tokens"), { page: "settings", scope: "you", section: "linked" });
  assert.deepEqual(parse("/settings", "", "#Not a section"), { page: "settings", scope: "you" });
  // Your machines' agents are each machine's now (Arugula #1107): the old addresses open Machines'.
  assert.deepEqual(parse("/settings", "", "#agents"), { page: "settings", scope: "machine", section: "agents" });
  assert.deepEqual(parse("/settings/you/agents"), { page: "settings", scope: "machine", section: "agents" });
  // Preferences is part of Appearance (Arugula #1318): its old addresses land there.
  assert.deepEqual(parse("/settings/you/preferences"), { page: "settings", scope: "you", section: "appearance" });
  assert.deepEqual(parse("/settings", "", "#preferences"), { page: "settings", scope: "you", section: "appearance" });
  assert.equal(pathOf({ page: "settings", scope: "machine", section: "agents" }), "/settings/machine#agents");
  assert.deepEqual(parse("/settings/machine", "", "#agents"), { page: "settings", scope: "machine", section: "agents" });
  assert.equal(pathOf({ page: "settings", scope: "machine", machine: "0a1b2c3d", section: "agents" }), "/settings/machine/0a1b2c3d/agents");
  // A machine's sections, four now: each old one lands on the one holding it, by path or `#section`.
  for (const [old, now] of [["access", "access"], ["guest-ssh", "access"], ["remote-sessions", "access"], ["rules", "agents"], ["grants", "agents"], ["reports", "advanced"], ["developer", "advanced"]]) {
    assert.deepEqual(parse(`/settings/machine/0a1b2c3d/${old}`), { page: "settings", scope: "machine", machine: "0a1b2c3d", section: now }, old);
    assert.deepEqual(parse("/settings/machine", "", `#${old}`), { page: "settings", scope: "machine", section: now }, old);
  }
  assert.deepEqual(parse("/settings/machine/0a1b2c3d/constructor"), { page: "settings", scope: "machine", machine: "0a1b2c3d", section: "constructor" });
  for (const p of ["/settings/x", "/settings/you/a/b", "/settings/you/Bad", "/settings/project", "/settings/project/nope", "/settings/machine/geek", "/settings/team/personal/a/b", "/settings/personal/plan"]) {
    assert.equal(parse(p).page, "missing", p);
  }
  assert.deepEqual(parse("/design"), { page: "design" });
  assert.equal(pathOf({ page: "design" }), "/design");
  assert.deepEqual(parse("/design/x"), { page: "missing", path: "/design/x" });
  assert.deepEqual(parse(`/p/${P}-x/chat`), { page: "chat", project: P });
  assert.deepEqual(parse(`/p/${P}/chat/ch_0123456789abcdef-general`, "?msg=12"), { page: "chat", project: P, room: "ch_0123456789abcdef", msg: 12 });
  assert.deepEqual(parse(`/p/${P}/chat/ch_0123456789abcdef`, "?msg=-3"), { page: "chat", project: P, room: "ch_0123456789abcdef", msg: -3 });
  assert.deepEqual(parse(`/p/${P}/chat/activity`), { page: "chat", project: P, view: "activity" });
  assert.deepEqual(parse(`/p/${P}/chat/saved`), { page: "chat", project: P, view: "saved" });
  assert.equal(pathOf({ page: "chat", project: P, view: "saved" }), `/p/${P}/chat/saved`);
  assert.deepEqual(parse(`/p/${P}/chat/search`, "?q=from%3A%40sam"), { page: "chat", project: P, view: "search", q: "from:@sam" });
  // Search's "All teams" rides in the URL (#883).
  assert.deepEqual(parse(`/p/${P}/chat/search`, "?q=deploy&all=1"), { page: "chat", project: P, view: "search", q: "deploy", all: true });
  assert.equal(pathOf({ page: "chat", project: P, view: "search", q: "deploy", all: true }), `/p/${P}/chat/search?q=deploy&all=1`);
  assert.equal(pathOf({ page: "chat", project: P, view: "search", all: true }), `/p/${P}/chat/search?all=1`);
  assert.equal(pathOf({ page: "chat", project: P, view: "search", q: "deploy", all: false }), `/p/${P}/chat/search?q=deploy`);
  // A DM is its team's project's (UI audit #169); an id-only one finds it.
  assert.deepEqual(parse(`/p/${P}/dm/dm_00aa11bb22cc33dd-sam`), { page: "dm", project: P, room: "dm_00aa11bb22cc33dd" });
  assert.deepEqual(parse("/dm/dm_00aa11bb22cc33dd-sam", "?msg=3"), { page: "find", what: "chat", room: "dm_00aa11bb22cc33dd", msg: 3 });
  assert.deepEqual(parse(`/p/${P}/workspaces/hw_0123456789abcdef0123456789abcdef-app`), { page: "workspace", project: P, workspace: "hw_0123456789abcdef0123456789abcdef" });
  // A project's settings are Settings' project scope (Arugula #1104).
  assert.deepEqual(parse(`/p/${P}-x/settings`), { page: "settings", scope: "project", project: P });
  assert.deepEqual(parse(`/settings/project/${P}-x/repos`), { page: "settings", scope: "project", project: P, section: "repos" });
  assert.equal(pathOf({ page: "settings", scope: "project", project: P, section: "repos" }), `/settings/project/${P}/repos`);
  // A section that's part of another now (Arugula #1105) lands on that one, by path or `#section`.
  assert.deepEqual(parse(`/settings/project/${P}/environments`), { page: "settings", scope: "project", project: P, section: "secrets" });
  assert.deepEqual(parse(`/settings/project/${P}/finish`), { page: "settings", scope: "project", project: P, section: "runs" });
  assert.deepEqual(parse(`/settings/project/${P}/agent-model`), { page: "settings", scope: "project", project: P, section: "agents" });
  assert.deepEqual(parse(`/p/${P}/settings`, "", "#plane"), { page: "settings", scope: "project", project: P, section: "repos" });
  assert.deepEqual(parse(`/p/${P}/settings`, "", "#boards"), { page: "settings", scope: "project", project: P, section: "issues" });
  assert.deepEqual(parse(`/settings/project/${P}/size`), { page: "settings", scope: "project", project: P, section: "runs" });
  assert.deepEqual(parse("/settings/you/finish"), { page: "settings", scope: "you", section: "finish" }, "only a project's");
  // A repo to link opens Linked repos.
  assert.deepEqual(parse(`/p/${P}/settings`, "?link=acme%2Fapp"), { page: "settings", scope: "project", project: P, section: "repos", link: "acme/app" });
  assert.deepEqual(parse(`/p/${P}/settings`, "?link=..%2F..%2Fx"), { page: "settings", scope: "project", project: P }, "not a repo");
  assert.deepEqual(parse("/installed", `?state=${encodeURIComponent(`${P}:acme/app`)}&installation_id=9&setup_action=install`), { page: "installed", project: P, repo: "acme/app" });
  assert.deepEqual(parse("/installed", "?state=https%3A%2F%2Fevil.example%2F"), { page: "installed" }, "only a project and a repo");
  assert.deepEqual(parse("/installed"), { page: "installed" });
  const env = `env_${"0a".repeat(16)}`;
  assert.deepEqual(parse(`/p/${P}/deploys`), { page: "deploys", project: P });
  assert.deepEqual(parse(`/p/${P}/deploys/new`), { page: "new-deploy", project: P });
  assert.deepEqual(parse(`/p/${P}/deploys/new`, `?env=${env}`), { page: "new-deploy", project: P, env });
  assert.deepEqual(parse(`/p/${P}/deploys/new`, "?env=production"), { page: "new-deploy", project: P }, "an id only");
});

test("the Overview names an open card by its board and row, both or neither", () => {
  assert.deepEqual(parse(`/p/${P}`, "?board=brd_0a1b&card=row_2c3d"), { page: "project", project: P, board: "brd_0a1b", card: "row_2c3d" });
  assert.deepEqual(parse(`/p/${P}/overview`, "?board=brd_0a1b&card=row_2c3d"), { page: "project", project: P, board: "brd_0a1b", card: "row_2c3d" });
  assert.deepEqual(parse(`/p/${P}`, "?card=row_2c3d"), { page: "project", project: P });
  assert.deepEqual(parse(`/p/${P}`, "?board=iss_0a1b&card=row_2c3d"), { page: "project", project: P });
  // The Overview zoomed in (Arugula #1287): a board, a card's work, a session on no card.
  assert.deepEqual(parse(`/p/${P}/board/brd_0a1b`), { page: "project", project: P, zoom: { board: "brd_0a1b" } });
  assert.deepEqual(parse(`/p/${P}/board/brd_0a1b-launch`), { page: "project", project: P, zoom: { board: "brd_0a1b" } });
  assert.deepEqual(parse(`/p/${P}/board/brd_0a1b/card/row_2c3d`), { page: "project", project: P, zoom: { board: "brd_0a1b", card: "row_2c3d" } });
  assert.deepEqual(parse(`/p/${P}/work/${encodeURIComponent("geek#7")}`), { page: "project", project: P, zoom: { session: "geek#7" } });
  assert.equal(parse(`/p/${P}/board/iss_0a1b`).page, "missing");
  assert.equal(parse(`/p/${P}/board/brd_0a1b/card/iss_2c3d`).page, "missing");
  assert.equal(parse(`/p/${P}/board/brd_0a1b/row/row_2c3d`).page, "missing");
  for (const zoom of [{ board: "brd_0a1b" }, { board: "brd_0a1b", card: "row_2c3d" }, { session: "geek#7" }, { session: "machine:m_1:ses_2" }]) {
    const path = pathOf({ page: "project", project: P, zoom }, names);
    assert.deepEqual(parse(path), { page: "project", project: P, zoom }, path);
  }
  assert.equal(pathOf({ page: "project", project: P, zoom: { board: "brd_0a1b", card: "row_2c3d" } }, names), `/p/${P}-arugula-team/board/brd_0a1b/card/row_2c3d`);
  // A board's slug where its id reads back from it; none where it can't.
  const boardNames = (id) => (id === "brd_0a1b" || id === "brd_e2e0launch" ? "Launch" : names(id));
  assert.equal(pathOf({ page: "project", project: P, zoom: { board: "brd_0a1b" } }, boardNames), `/p/${P}-arugula-team/board/brd_0a1b-launch`);
  assert.equal(pathOf({ page: "project", project: P, zoom: { board: "brd_e2e0launch" } }, boardNames), `/p/${P}-arugula-team/board/brd_e2e0launch`);
  assert.deepEqual(parse(`/p/${P}/board/brd_e2e0launch`), { page: "project", project: P, zoom: { board: "brd_e2e0launch" } });
});

test("anything else is missing, never a guess", () => {
  for (const p of ["/p", "/p/arugula", `/p/${P}/nope`, "/dm/sam", `/p/${P}/workspaces/app`, `/p/${P}/workspaces/cw_0123abcd`, `/p/${P}/workspaces/hw_0123456789abcdef0123456789abcdef/tab`, `/p/${P}/past/run_00ff11ee`, "/settings/x", "/t/team_1", "/t/team_1/nope", "/chats", `/p/${P}/deploys/old`, `/p/${P}/deploys/new/x`]) {
    assert.equal(parse(p).page, "missing", p);
  }
  assert.equal(parse(`/p/${P}/chat/ch_0123456789abcdef`, "?msg=x").msg, undefined);
});

test("links carry the current slug, and read back as the same route", () => {
  const routes = [
    { page: "home" },
    { page: "project", project: P },
    { page: "chat", project: P, room: "ch_0123456789abcdef", msg: 4 },
    { page: "project", project: P, board: "brd_0a1b", card: "row_2c3d" },
    { page: "chat", project: P, view: "search", q: "has:quote deploy" },
    { page: "chat", project: P, view: "search", q: "deploy", all: true },
    { page: "dm", project: P, room: "dm_00aa11bb22cc33dd" },
    { page: "workspace", project: P, workspace: "hw_0123456789abcdef0123456789abcdef" },
    { page: "settings", scope: "project", project: P, section: "repos", link: "acme/app" },
    { page: "settings", scope: "project", project: P, section: "secrets" },
    { page: "move", project: P },
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
    { page: "all-boards" },
    { page: "all-boards", board: "brd_0a01" },
    { page: "docs" },
    { page: "settings", scope: "you" },
    { page: "settings", scope: "you", section: "tokens" },
    { page: "settings", scope: "machine" },
    { page: "settings", scope: "machine", machine: "0a1b2c3d", section: "machine" },
    { page: "settings", scope: "project", project: P },
    { page: "settings", scope: "team", owner: "personal" },
    { page: "settings", scope: "team", owner: "a1b2c3d4e5f60718", section: "members" },
    { page: "design" },
    { page: "deploy", deploy: `dep_${"0a".repeat(16)}` },
    { page: "team", team: "a1b2c3d4e5f60718", tab: "chat" },
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

test("a team's pages (Arugula #895): its id, then its slug, then the tab", () => {
  const T = "a1b2c3d4e5f60718";
  const teamNames = (id) => (id === T ? "Acme Inc." : undefined);
  for (const tab of ["chat", "members"]) {
    assert.deepEqual(parse(`/t/${T}/${tab}`), { page: "team", team: T, tab });
    assert.equal(pathOf({ page: "team", team: T, tab }, teamNames), `/t/${T}-acme-inc/${tab}`);
    assert.deepEqual(parse(pathOf({ page: "team", team: T, tab }, teamNames)), { page: "team", team: T, tab }, "the slug is only for people");
  }
  assert.deepEqual(parse(`/t/${T}-an-old-name/members`), { page: "team", team: T, tab: "members" });
  assert.equal(parse(`/t/${T}`).page, "missing", "a tab is named");
  assert.equal(parse(`/t/${T}/boards`).page, "missing");
  assert.equal(parse(`/t/${T}/chat/more`).page, "missing");
  assert.equal(parse("/t/%20/chat").page, "missing");
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

test("an id-only search keeps All teams when it's put in a project (Arugula #908)", () => {
  const found = parse("/chat/search", "?q=deploy&all=1");
  assert.deepEqual(found, { page: "find", what: "chat", view: "search", q: "deploy", all: true });
  assert.equal(pathOf(chatViewIn(found, P)), `/p/${P}/chat/search?q=deploy&all=1`);
  assert.equal(pathOf(chatViewIn(parse("/chat/search", "?q=deploy"), P)), `/p/${P}/chat/search?q=deploy`);
  assert.equal(pathOf(chatViewIn(parse("/chat/saved"), P)), `/p/${P}/chat/saved`);
  assert.equal(pathOf(chatViewIn(parse("/chat"), P)), `/p/${P}/chat`);
});

test("an agent session (M257)", () => {
  const S = "ses_0123456789abcdef0123456789abcdef";
  assert.deepEqual(parse(`/p/${P}/sessions/${S}`), { page: "session", project: P, session: S });
  assert.deepEqual(parse(pathOf({ page: "session", project: P, session: S })), { page: "session", project: P, session: S });
  for (const p of [`/p/${P}/sessions/run_00ff11ee`, `/p/${P}/sessions/${S}/x`]) assert.equal(parse(p).page, "missing", p);
  // The Sessions page's old address, which goes on to Boards (#1082).
  assert.deepEqual(parse(pathOf(parse(`/p/${P}/sessions`))), parse(`/p/${P}/sessions`));
  assert.equal(parse(`/p/${P}/sessions`).page, "sessions");
  // Its old filters are dropped with it.
  assert.deepEqual(parse(`/p/${P}/sessions`, "?label=ui&going=true"), { page: "sessions", project: P });
});

test("a project's Docs and one Doc (aru-118)", () => {
  const D = "pdc_0123456789abcdef0123456789abcdef";
  assert.deepEqual(parse(`/p/${P}/docs`), { page: "docs", project: P });
  assert.deepEqual(parse(`/p/${P}/docs/${D}`), { page: "doc", project: P, doc: D });
  assert.deepEqual(parse(`/p/${P}/docs/${D}-release-plan`), { page: "doc", project: P, doc: D });
  assert.equal(pathOf({ page: "doc", project: P, doc: D }, (id) => (id === D ? "Release plan" : undefined)), `/p/${P}/docs/${D}-release-plan`);
  for (const p of [`/p/${P}/docs/brd_00ff11ee`, `/p/${P}/docs/${D}/x`]) assert.equal(parse(p).page, "missing", p);
});

test("the shell's hand-off: `/?to=` names a place for the current project", () => {
  assert.deepEqual(parse("/", "?to=docs"), { page: "home", to: "docs" });
  for (const to of ["projects", "boards", "chat"]) assert.deepEqual(parse("/", `?to=${to}`), { page: "home", to });
  assert.deepEqual(parse("/", "?to=inbox"), { page: "home" });
  assert.deepEqual(parse("/", "?to="), { page: "home" });
});
