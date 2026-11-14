// "Move to project…" (Arugula #971): from a project's settings, its owner
// picks a board (which brings its repo), a Doc and a machine's Doc, sees
// what comes along and what to deal with first, and moves them into a new
// team project, landing there. A 409 from the move plans again and says so.
// On a fixture page (move-to-project-page.tsx); the platform's answers,
// the split's plan and move among them, are this spec's (the planner's
// rules are the cloud's tests).
import { expect, test, type Page, type Route } from "@playwright/test";
import { fixturePage } from "./fixture-page";

const ME = "prj_aaaa0001";
const NEW = "prj_aaaa0009";
/** Where `MOVE_SHOTS=dir` puts the screen's pictures. */
const SHOTS = process.env.MOVE_SHOTS;

const team = (id: string, name: string, role: string) => ({ id, name, role, members: [] });
const projects = (made: boolean) => [
  { id: ME, name: "Personal", owner: { kind: "personal" }, team: null },
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
};
const DOCS = [
  { id: "pdc_rhythm", title: "Rhythm plan" },
  { id: "pdc_pricing", title: "Pricing" },
];
const MAC_DOC = "d0c0ffee00000001";

type Ask = { to: { kind: "new"; name: string; owner: { kind: string; id?: string } } | { kind: "project"; id: string }; repos: string[]; boards: string[]; issues: string[]; docs: string[] };

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
  const empty = !repos.length && !boards.length && !issues.length && !docs.length;
  const machine_docs = rhythm
    ? [
        { machine: "m_mac", name: "aarons-mac-mini", docs: [{ id: MAC_DOC, title: "Colour (#688)" }] },
        { machine: "m_jake", name: "jakes-laptop", docs: [{ id: "d0c0ffee00000002", title: "Pricing proposal" }] },
      ]
    : [];
  const notes = empty || !toTeam ? [] : [{ key: "people", text: "Everyone in Arugula team will see what moves." }, ...(rhythm ? [{ key: "machine_docs:m_jake", text: "Move its Doc from jakes-laptop so Arugula team can open it." }] : [])];
  return {
    from: { id: ME, name: "Personal" },
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
async function open(page: Page, path: string, move: (ask: Ask, n: number) => { status: number; body: unknown }): Promise<Seen> {
  const seen: Seen = { plans: [], moves: [] };
  let made = false;
  const json = (r: Route, body: unknown, status = 200) => r.fulfill({ status, contentType: "application/json", body: JSON.stringify(body) });
  // What this spec doesn't answer, nobody does.
  await page.route(/\/api\//, (r) => json(r, { error: "not here" }, 404));
  await page.route(/\/api\/projects$/, (r) => json(r, { me: { account: "acct_a", login: "sam", name: "Sam" }, projects: projects(made), hosted: false }));
  await page.route(new RegExp(`/api/projects/${ME}/docs$`), (r) => json(r, { docs: DOCS.map((d) => ({ ...d, layout: "doc", rev: 1, created_at: 1, updated_at: 1 })) }));
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

test("something changed since the plan: nothing moves, and the new plan shows", async ({ page }) => {
  const seen = await open(page, `/p/${ME}/settings/move`, () => ({ status: 409, body: { error: "alecraso/rhythm: it changed since the plan, so nothing moved. Plan it again." } }));
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
