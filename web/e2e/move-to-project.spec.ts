// "Move to project…" (Arugula #971): from a project's settings, its owner
// picks a board (which brings its repo), a Doc and a machine's Doc, sees
// what comes along and what to deal with first, and moves them into a new
// team project, landing there. A 409 from the move plans again and says so.
// On a fixture page (move-to-project-page.tsx); the platform's answers,
// the split's plan and move among them, are this spec's (the planner's
// rules are the cloud's tests). The project switcher's New project from…
// (Arugula #1040) opens the same page for a project the reader owns. The
// project's sessions, by machine (Arugula #1166): a repo brings those it
// placed, others are picked on the reader's own online machines, and the
// page says who will see them.
import { expect, test, type Page, type Route } from "@playwright/test";
import { fixturePage } from "./fixture-page";

const ME = "prj_aaaa0001";
const NEW = "prj_aaaa0009";
/** Where `MOVE_SHOTS=dir` puts the screen's pictures. */
const SHOTS = process.env.MOVE_SHOTS;

const team = (id: string, name: string, role: string) => ({ id, name, role, members: [] });
const projects = (made: boolean) => [
  { id: ME, name: "Default", owner: { kind: "personal" }, team: null },
  { id: "prj_aaaa0002", name: "Arugula", owner: { kind: "team", id: "tm_1", name: "Arugula team" }, team: team("tm_1", "Arugula team", "owner") },
  { id: "prj_aaaa0003", name: "Site", owner: { kind: "team", id: "tm_2", name: "Friends" }, team: team("tm_2", "Friends", "editor") },
  ...(made ? [{ id: NEW, name: "Rhythm", owner: { kind: "team", id: "tm_1", name: "Arugula team" }, team: null }] : []),
];

const RHYTHM = "alecraso/rhythm";
const SOURCE = {
  repos: [
    { id: 1, name: RHYTHM, boards: 1, issues: 2, github: 3, sessions: 2 },
    { id: 2, name: "alecraso/bradbury", boards: 0, issues: 0, github: 0, sessions: 0 },
  ],
  boards: [
    { id: "brd_rhythm", title: "Rhythm → dogfood on Android + iPhone", repo: RHYTHM, rows: 3, closed: false, docs: ["pdc_rhythm"] },
    { id: "brd_ideas", title: "Ideas", rows: 2, closed: false, docs: [] },
  ],
  issues: [{ id: "iss_loose", number: 1, title: "Docs list: share Docs without opening them", closed: true }],
  docs: [
    { id: "pdc_rhythm", title: "Rhythm plan", board: "brd_rhythm" },
    { id: "pdc_pricing", title: "Pricing" },
  ],
  machines: [
    {
      id: "m_mac",
      name: "aarons-mac-mini",
      online: true,
      yours: true,
      owner: "Sam",
      sessions: [
        { id: "1", name: "belt-balancer", repo: RHYTHM, by_hand: false },
        { id: "2", name: "factorio server", by_hand: true },
        { id: "3", name: "dotfiles", by_hand: false },
      ],
    },
    {
      id: "m_old",
      name: "old-linux-box",
      online: false,
      yours: true,
      owner: "Sam",
      sessions: [
        { id: "1", name: "map-render", repo: RHYTHM, by_hand: false },
        { id: "2", name: "homelab", by_hand: true },
      ],
    },
    {
      id: "m_jake",
      name: "jake-mbp",
      online: true,
      yours: false,
      owner: "Jake",
      sessions: [
        { id: "1", name: "site-dev", repo: RHYTHM, by_hand: false },
        { id: "2", name: "L7 drawer", by_hand: true },
      ],
    },
  ],
};
const DOCS = SOURCE.docs;
const MAC_DOC = "d0c0ffee00000001";

type Ask = { to: { kind: "new"; name: string; owner: { kind: string; id?: string } } | { kind: "project"; id: string }; repos: string[]; boards: string[]; issues: string[]; docs: string[]; sessions: string[] };

const page_ = <T>(items: T[]) => ({ count: items.length, items });

/** The planner's answer to `ask`, by its rules, over the fixture; `again`
 * is the project after someone else changed it (one more issue). */
function planOf(ask: Ask, again: boolean) {
  const rhythm = ask.repos.includes(RHYTHM);
  const first = ask.to.kind === "new" ? 1 : 975;
  const issues = rhythm ? [3, 4, ...(again ? [5] : [])].map((n, k) => ({ id: `iss_${n}`, number: n, to: first + k, title: `Rhythm: step ${n}`, closed: false, with: { kind: "repo", repo: RHYTHM } })) : [];
  if (ask.issues.includes("iss_loose")) issues.push({ id: "iss_loose", number: 1, to: first + issues.length, title: SOURCE.issues[0].title, closed: true } as (typeof issues)[number]);
  const docs = [
    ...(rhythm ? [{ id: "pdc_rhythm", title: "Rhythm plan", with: { kind: "board", board: "brd_rhythm", title: SOURCE.boards[0].title } }] : []),
    ...DOCS.filter((d) => ask.docs.includes(d.id) && !(rhythm && d.id === "pdc_rhythm")),
  ];
  const boards = [...(rhythm ? [{ id: "brd_rhythm", title: SOURCE.boards[0].title, rows: 3, with: { kind: "repo", repo: RHYTHM } }] : []), ...(ask.boards.includes("brd_ideas") ? [{ id: "brd_ideas", title: "Ideas", rows: 2 }] : [])];
  const repos = SOURCE.repos.filter((r) => ask.repos.includes(r.name)).map((r) => ({ id: r.id, name: r.name }));
  const toTeam = ask.to.kind === "new" && ask.to.owner.kind === "team";
  // Sessions a moving repo placed, from any machine, and those picked.
  const machine_sessions = SOURCE.machines
    .map((m) => ({
      machine: m.id,
      name: m.name,
      online: m.online,
      sessions: m.sessions
        .filter((s) => (s.repo && ask.repos.includes(s.repo)) || ask.sessions.includes(`${m.id}/${s.id}`))
        .map((s) => ({ id: s.id, name: s.name, who: toTeam ? "the Arugula team watches" : m.yours ? "only you" : `only ${m.owner}, on their machine`, ...(s.repo && ask.repos.includes(s.repo) ? { with: { kind: "repo", repo: s.repo } } : {}) })),
    }))
    .filter((m) => m.sessions.length);
  const moving = machine_sessions.flatMap((m) => m.sessions.map((s) => s.name));
  const empty = !repos.length && !boards.length && !issues.length && !docs.length && !moving.length;
  const machine_docs = rhythm
    ? [
        { machine: "m_mac", name: "aarons-mac-mini", docs: [{ id: MAC_DOC, title: "Colour (#688)" }] },
        { machine: "m_jake", name: "jakes-laptop", docs: [{ id: "d0c0ffee00000002", title: "Pricing proposal" }] },
      ]
    : [];
  const notes = empty || !toTeam ? [] : [{ key: "people", text: "Everyone in Arugula team will see what moves." }, ...(rhythm ? [{ key: "machine_docs:m_jake", text: "Move its Doc from jakes-laptop so Arugula team can open it." }] : [])];
  if (toTeam && moving.length) notes.push({ key: "sessions-watch", text: `Moving ${moving.length} sessions to ${ask.to.kind === "new" ? ask.to.name : "Arugula"} lets the Arugula team watch them: ${moving.join(", ")}. That includes what they printed before.` });
  return {
    from: { id: ME, name: "Default" },
    to: { ...(ask.to.kind === "project" ? { id: ask.to.id } : {}), name: ask.to.kind === "new" ? ask.to.name : "Arugula", owner: toTeam ? { kind: "team", id: "tm_1", name: "Arugula team" } : { kind: "personal", account: "acct_a" }, first_number: first },
    empty,
    notes,
    along: {
      repos: page_(repos),
      boards: page_(boards),
      issues: page_(issues),
      github: rhythm ? { issues: 2, prs: 1, repos: [{ repo: RHYTHM, issues: 2, prs: 1 }] } : { issues: 0, prs: 0, repos: [] },
      docs: page_(docs),
      machine_docs,
      workspaces: [],
      environments: [],
      sessions: rhythm ? 2 : 0,
      drafts: 0,
      machine_sessions,
      links: { moving: [], crossing: [] },
    },
    source: SOURCE,
  };
}

interface Seen {
  plans: Ask[];
  moves: Ask[];
}

/** The fixture page at `path`, with the platform's answers; `move` answers the split. */
async function open(page: Page, path: string, move: (ask: Ask, n: number) => { status: number; body: unknown }, list = projects): Promise<Seen> {
  const seen: Seen = { plans: [], moves: [] };
  let made = false;
  const json = (r: Route, body: unknown, status = 200) => r.fulfill({ status, contentType: "application/json", body: JSON.stringify(body) });
  // What this spec doesn't answer, nobody does.
  await page.route(/\/api\//, (r) => json(r, { error: "not here" }, 404));
  await page.route(/\/api\/projects$/, (r) => json(r, { me: { account: "acct_a", login: "sam", name: "Sam" }, projects: list(made), hosted: false }));
  await page.route(new RegExp(`/api/projects/${ME}/split/plan$`), (r) => {
    const ask = r.request().postDataJSON() as Ask;
    seen.plans.push(ask);
    return json(r, planOf(ask, seen.moves.length > 0));
  });
  await page.route(new RegExp(`/api/projects/${ME}/split$`), (r) => {
    const ask = r.request().postDataJSON() as Ask;
    seen.moves.push(ask);
    const a = move(ask, seen.moves.length);
    if (a.status === 200) made = true;
    return json(r, a.body, a.status);
  });
  const body = await fixturePage("./move-to-project-page.tsx");
  await page.route(/\/p\/prj_/, (r) => (r.request().resourceType() === "document" ? r.fulfill({ contentType: "text/html", body }) : r.fallback()));
  await page.goto(path);
  return seen;
}

const moved = { status: 200, body: { id: "spl_1", to: { id: NEW, name: "Rhythm" }, moved: { repos: 1, boards: 1, issues: 2, docs: 2, workspaces: 0, environments: 0, runs: 0, sessions: 2, drafts: 0, links: 0 }, issues: [] } };
const row = (page: Page, list: string, key: string) => page.locator(`[data-move-${list}] [data-pick="${key}"]`);
const tick = (page: Page, list: string, key: string) => row(page, list, key).locator("input").check();

test.use({ viewport: { width: 1280, height: 900 } });

test("a board, its repo and a Doc move into a new team project, and a machine's Doc after them", async ({ page }) => {
  const seen = await open(page, `/p/${ME}/settings`, () => moved);
  // The owner's way in: Settings, General.
  await page.locator("[data-project-move-some]").click();
  await expect(page).toHaveURL(new RegExp(`/p/${ME}[^/]*/settings/move$`));
  await expect(page.locator("[data-move-page] h1")).toHaveText("Move to project");
  await expect(page.locator("[data-move-along]")).toHaveCount(0);

  await page.locator("[data-move-name]").fill("Rhythm");
  await page.locator("[data-move-owner]").selectOption("tm_1");
  // The board brings its repo, and the repo's Doc on it comes along.
  await tick(page, "boards", "brd_rhythm");
  await expect(row(page, "repos", RHYTHM).locator("input")).toBeChecked();
  await expect(row(page, "boards", "brd_rhythm")).toContainText(`with ${RHYTHM}`);
  await expect(row(page, "docs", "pdc_rhythm")).toContainText("with its board");
  await tick(page, "docs", "pdc_pricing");

  // What to deal with first, and what comes along, as the planner says.
  await expect(page.locator('[data-move-note="people"]')).toHaveText("Everyone in Arugula team will see what moves.");
  await expect(page.locator("[data-move-along-issues]")).toContainText("2 issues");
  await expect(page.locator("[data-move-along-docs]")).toContainText("2 Docs");
  await expect(page.locator("[data-move-along-github]")).toContainText("3 GitHub issues and pull requests");
  await page.locator("[data-move-along-issues] button").first().click();
  await expect(page.locator("[data-move-along-issues] li").first()).toContainText("#3");
  await expect(page.locator("[data-move-along-issues] li").first()).toContainText("#1");

  // A machine this page reaches gives its Doc; another says where to go.
  await expect(page.locator('[data-pick-group="machine-m_jake"]')).toContainText("These move from jakes-laptop.");
  await expect(page.locator('[data-move-open-on="m_jake"]')).toHaveAttribute("href", /\/machines\/m_jake/);
  await expect(row(page, "docs", `m_jake/d0c0ffee00000002`).locator("input")).toHaveCount(0);
  await tick(page, "docs", `m_mac/${MAC_DOC}`);

  await expect(page.locator("[data-move-go]")).toHaveText("Move to Rhythm");
  if (SHOTS) {
    await page.locator("[data-move-page] h1").scrollIntoViewIfNeeded();
    await page.locator("[data-move-page]").evaluate((e) => e.closest(".platform-main")?.scrollTo(0, 0));
    await page.screenshot({ path: `${SHOTS}/final-move.png` });
    await page.locator("[data-move-docs]").evaluate((e) => e.scrollIntoView());
    await page.screenshot({ path: `${SHOTS}/final-move-docs.png` });
  }
  await expect.poll(() => seen.plans.at(-1)).toMatchObject({ to: { kind: "new", name: "Rhythm", owner: { kind: "team", id: "tm_1" } }, repos: [RHYTHM], docs: ["pdc_pricing"] });

  await page.locator("[data-move-go]").click();
  // The new project, with the machine's Doc given to it after the rest.
  await expect(page).toHaveURL(new RegExp(`/p/${NEW}`));
  await expect(page.locator(`[data-project-home="${NEW}"]`)).toBeVisible();
  expect(seen.moves).toHaveLength(1);
  expect(seen.moves[0]).toMatchObject({ repos: [RHYTHM], docs: ["pdc_pricing"] });
  expect(await page.evaluate(() => (window as unknown as { __movedDocs: unknown }).__movedDocs)).toEqual([["m_mac", MAC_DOC, NEW]]);
});

test("into a new personal project, the reader's own machine gives its Doc too (Arugula #1026)", async ({ page }) => {
  const seen = await open(page, `/p/${ME}/settings/move`, () => moved);
  await page.locator("[data-move-name]").fill("Rhythm");
  await tick(page, "boards", "brd_rhythm");
  // As for a team's: a machine this page reaches gives its Doc; another says where to go.
  await expect(page.locator('[data-pick-group="machine-m_jake"]')).toContainText("These move from jakes-laptop.");
  await expect(row(page, "docs", `m_jake/d0c0ffee00000002`).locator("input")).toHaveCount(0);
  await tick(page, "docs", `m_mac/${MAC_DOC}`);
  await expect.poll(() => seen.plans.at(-1)).toMatchObject({ to: { kind: "new", name: "Rhythm", owner: { kind: "personal" } }, repos: [RHYTHM] });

  await page.locator("[data-move-go]").click();
  await expect(page).toHaveURL(new RegExp(`/p/${NEW}`));
  expect(await page.evaluate(() => (window as unknown as { __movedDocs: unknown }).__movedDocs)).toEqual([["m_mac", MAC_DOC, NEW]]);
});

test("something changed since the plan: nothing moves, and the new plan shows", async ({ page }) => {
  const seen = await open(page, `/p/${ME}/settings/move`, () => ({ status: 409, body: { error: "alecraso/rhythm: it changed since the plan, so nothing moved. Plan it again.", reason: "changed" } }));
  await page.locator("[data-move-name]").fill("Rhythm");
  await tick(page, "repos", RHYTHM);
  await expect(page.locator("[data-move-along-issues]")).toContainText("2 issues");
  await page.locator("[data-move-go]").click();
  await expect(page.locator("[data-move-changed]")).toHaveText("Something changed; here's the new plan.");
  await expect(page.locator("[data-move-along-issues]")).toContainText("3 issues");
  await expect(page).toHaveURL(/\/settings\/move$/);
  await expect(page.locator("[data-move-error]")).toHaveCount(0);
  if (SHOTS) await page.screenshot({ path: `${SHOTS}/final-changed.png`, fullPage: true });
  expect(seen.moves).toHaveLength(1);
});

test("only the project's owner sees the way in, or the page", async ({ page }) => {
  await open(page, "/p/prj_aaaa0003/settings", () => moved);
  await expect(page.locator("[data-project-general]")).toBeVisible();
  await expect(page.locator("[data-project-move-some]")).toHaveCount(0);
  await page.goto("/p/prj_aaaa0003/settings/move");
  await expect(page.getByText("Only the project's owner moves work out of it.")).toBeVisible();
});

test("New project from…, in Go to…'s projects: one of the reader's own projects, then its move, into a new project", async ({ page }) => {
  await open(page, "/p/prj_aaaa0002", () => moved);
  await page.locator("[data-rail] [data-project-switcher]").click();
  await expect(page.locator('[data-command="project/new-from"]')).toContainText("New project from…");
  if (SHOTS) await page.locator("[data-goto]").screenshot({ path: `${SHOTS}/followup-switcher.png` });
  await page.locator('[data-command="project/new-from"]').click();
  // Only those the reader owns: Site is Friends', where they're an editor.
  const source = page.locator("[data-new-project-from-source]");
  await expect(source.locator("option")).toHaveText(["Default", "Arugula"]);
  await expect(source).toHaveValue("prj_aaaa0002");
  if (SHOTS) await page.locator("[data-new-project-from]").screenshot({ path: `${SHOTS}/followup-new-project-from.png` });
  await source.selectOption(ME);
  await page.locator("[data-new-project-from-go]").click();
  await expect(page).toHaveURL(new RegExp(`/p/${ME}[^/]*/settings/move$`));
  await expect(page.locator("[data-move-target]")).toHaveValue("new");
  await expect(page.locator("[data-move-name]")).toBeVisible();
});

test("a reader who owns no project isn't offered New project from…", async ({ page }) => {
  await open(page, "/p/prj_aaaa0003", () => moved, () => projects(false).filter((p) => p.id === "prj_aaaa0003"));
  await page.locator("[data-rail] [data-project-switcher]").click();
  await expect(page.locator('[data-command="project/new"]')).toBeVisible();
  await expect(page.locator('[data-command="project/new-from"]')).toHaveCount(0);
});

test("the project's sessions, by machine: a repo brings those it placed, others are picked, and the page says who sees them (Arugula #1166)", async ({ page }) => {
  const done = { ...moved.body, machine_sessions: { moved: 3, waiting: [{ machine: "m_old", name: "old-linux-box", sessions: ["map-render"] }], failed: [] } };
  const seen = await open(page, `/p/${ME}/settings/move`, () => ({ status: 200, body: done }));
  await page.locator("[data-move-name]").fill("Factorio mods");
  const machine = (id: string) => page.locator(`[data-move-sessions] [data-move-sessions-machine="${id}"]`);
  await expect(machine("m_mac")).toContainText("On aarons-mac-mini");
  await expect(machine("m_old")).toContainText("old-linux-box is offline: its other sessions can be picked once it's back.");
  await expect(machine("m_jake")).toContainText("Jake's machine: Jake moves these from each session's menu.");
  // Others' and offline machines' sessions have no box.
  await expect(row(page, "sessions", "m_old/2").locator("input")).toHaveCount(0);
  await expect(row(page, "sessions", "m_jake/2").locator("input")).toHaveCount(0);
  await expect(row(page, "sessions", "m_mac/2")).toContainText("Set by hand");

  // Picked one by one on the reader's own machine; a repo's, picked first, goes with the repo.
  await tick(page, "sessions", "m_mac/2");
  await tick(page, "sessions", "m_mac/1");
  await expect.poll(() => seen.plans.at(-1)?.sessions).toEqual(["m_mac/1", "m_mac/2"]);
  await tick(page, "repos", RHYTHM);
  for (const k of ["m_mac/1", "m_old/1", "m_jake/1"]) {
    await expect(row(page, "sessions", k).locator("input")).toBeChecked();
    await expect(row(page, "sessions", k).locator("input")).toBeDisabled();
  }
  await expect(row(page, "sessions", "m_old/1")).toContainText(`with ${RHYTHM} · when it's back`);
  await expect.poll(() => seen.plans.at(-1)?.sessions).toEqual(["m_mac/2"]);

  // Who sees them: each beside it when they differ, one line when not.
  const along = page.locator("[data-move-along-sessions]");
  await expect(along).toContainText("4 sessions");
  await expect(page.locator("[data-move-sessions-who]")).toHaveText("Who sees each is beside it.");
  await along.locator("button").first().click();
  await expect(along.locator("li", { hasText: "map-render" })).toContainText("on old-linux-box, when it's back · only you");
  await expect(along.locator("li", { hasText: "site-dev" })).toContainText("on jake-mbp · only Jake, on their machine");
  await page.locator("[data-move-owner]").selectOption("tm_1");
  await expect(page.locator('[data-move-note="sessions-watch"]')).toContainText("lets the Arugula team watch them: belt-balancer, factorio server, map-render, site-dev. That includes what they printed before.");
  await expect(page.locator("[data-move-sessions-who]")).toHaveText("The Arugula team watches.");
  if (SHOTS) {
    await page.locator("[data-move-sessions]").evaluate((e) => e.scrollIntoView());
    await page.screenshot({ path: `${SHOTS}/sessions.png` });
  }

  await page.locator("[data-move-go]").click();
  await expect(page).toHaveURL(new RegExp(`/p/${NEW}`));
  expect(seen.moves[0]).toMatchObject({ repos: [RHYTHM], sessions: ["m_mac/2"] });
  await expect(page.getByText("3 sessions moved now; map-render moves when old-linux-box is back.")).toBeVisible();
});
