// The Docs place (Lift 6b, Arugula #1484): one list on the kit's rows, the
// latest changed first, each its title (with a tabs or prototype Doc's
// mark), who made it and when; a team's has a Mine filter (`m`); `j`/`k`
// move and ↵ opens the Doc's page. No tabs, no sort, no search and no
// preview. `/docs` goes to the current project's. On #971's fixture page
// (move-to-project-page.tsx): the platform's answers are this spec's.
// DOCS_PLACE_SHOTS=DIR keeps its pictures, light and dark.
import { expect, test, type Page, type Route } from "@playwright/test";
import { fixturePage } from "./fixture-page";

const ME = "prj_aaaa0001";
const SITE = "prj_aaaa0002";
const SHOTS = process.env.DOCS_PLACE_SHOTS;
const HOUR = 3_600_000;

const roster = [
  { account: "acct_a", name: "Sam", role: "owner" },
  { account: "acct_b", name: "Jo", role: "editor" },
];
const projects = [
  { id: ME, name: "Default", owner: { kind: "personal" }, team: null },
  { id: SITE, name: "Site", owner: { kind: "team", id: "tm_1", name: "Acme" }, team: { id: "tm_1", name: "Acme", role: "owner", members: roster } },
];

const docOf = (project: string, id: string, title: string, author: string, ago: number, layout?: string) => {
  const at = Date.now() - ago;
  return { id, project, title, author, agent: null, sections: 1, written: 1, rev: 1, created_at: at - HOUR, updated_at: at, ...(layout ? { layout } : {}) };
};
type Doc = ReturnType<typeof docOf>;

/** The fixture page at `path`, with the platform's answers for both projects' Docs. */
async function open(page: Page, path: string, docs: Record<string, Doc[]>) {
  const json = (r: Route, body: unknown, status = 200) => r.fulfill({ status, contentType: "application/json", body: JSON.stringify(body) });
  await page.route(/\/api\//, (r) => json(r, { error: "not here" }, 404));
  await page.route(/\/api\/projects$/, (r) => json(r, { me: { account: "acct_a", login: "sam", name: "Sam" }, projects, hosted: false }));
  await page.route(/\/api\/projects\/([^/]+)\/docs$/, (r) => {
    const project = r.request().url().split("/").at(-2)!;
    return json(r, { project, docs: docs[project] ?? [], writer: true });
  });
  await page.route(/\/api\/projects\/[^/]+\/docs\/trash$/, (r) => json(r, { project: SITE, docs: [], days: 30 }));
  await page.route(/\/api\/projects\/[^/]+\/docs\/(pdc_[a-z]+)$/, (r) => {
    const id = r.request().url().split("/").pop()!;
    const d = Object.values(docs).flat().find((x) => x.id === id);
    if (!d) return json(r, { error: "not here" }, 404);
    return json(r, { doc: d, sections: [{ id: "s1", heading: "Why", intent: "", body: `What ${d.title} is for.`, rev: 1 }], assets: [], writer: true, deleter: true, link: null });
  });
  if (SHOTS) await page.addInitScript(() => localStorage.setItem("arugula-appearance", JSON.stringify({ theme: "system", preset: "arugula", light: null, warmth: 0, depth: 0 })));
  const body = await fixturePage("./move-to-project-page.tsx");
  await page.route(/\/(p\/prj_|docs)/, (r) => (r.request().resourceType() === "document" ? r.fulfill({ contentType: "text/html", body }) : r.fallback()));
  await page.goto(path);
}

const team = () => ({
  [SITE]: [
    docOf(SITE, "pdc_notes", "Pricing notes", "acct_b", 2 * 24 * HOUR),
    docOf(SITE, "pdc_plan", "Launch plan", "acct_a", HOUR),
    docOf(SITE, "pdc_tabs", "Printer settings", "acct_b", 3 * HOUR, "tabs"),
    docOf(SITE, "pdc_proto", "Onboarding sketch", "acct_a", 5 * HOUR, "prototype"),
  ],
});

const rows = (page: Page) => page.locator("[data-docs] [data-doc]");
const titles = (page: Page) => rows(page).locator("[data-doc-title]");

/** Pictures of the page as it is, dark and light, when asked for. The
 * fixture page paints on a change of the system's scheme, not at load. */
async function shoot(page: Page, name: string) {
  if (!SHOTS) return;
  const frame = () => page.evaluate(() => new Promise((done) => requestAnimationFrame(() => requestAnimationFrame(done))));
  await page.emulateMedia({ colorScheme: "light" });
  await frame();
  for (const scheme of ["dark", "light"] as const) {
    await page.emulateMedia({ colorScheme: scheme });
    await frame();
    await page.screenshot({ path: `${SHOTS}/${name}-${scheme}.png` });
  }
}

test.use({ viewport: { width: 1440, height: 900 } });

test("one list, the latest changed first: a row is its title, who made it and when; no tabs, sort, search or preview", async ({ page }) => {
  await open(page, `/p/${SITE}/docs`, team());
  await expect(titles(page)).toHaveText(["Launch plan", "Printer settings", "Onboarding sketch", "Pricing notes"]);
  await expect(page.locator("[data-docs-count]")).toHaveText("4 in Site");
  await expect(rows(page).filter({ hasText: "Pricing notes" })).toContainText("Jo");
  await expect(rows(page).filter({ hasText: "Launch plan" })).toContainText("you");
  // A tabs and a prototype Doc are marked; a plain one isn't (Arugula #830).
  await expect(rows(page).filter({ hasText: "Printer settings" }).getByRole("img", { name: "Tabs Doc" })).toHaveCount(1);
  await expect(rows(page).filter({ hasText: "Onboarding sketch" }).getByRole("img", { name: "Prototype Doc" })).toHaveCount(1);
  await expect(rows(page).filter({ hasText: "Launch plan" }).locator("[data-doc-layout]")).toHaveCount(0);
  // What went with Lift 6b.
  await expect(page.locator("[data-docs-tab], [data-list-preview], [data-list-sort], [data-list-search], [data-doc-preview]")).toHaveCount(0);
  await expect(page.locator("[data-doc-more]")).toHaveCount(0);
  await shoot(page, "docs-list");
});

test("Mine is the reader's Docs and their agents', by its filter or m", async ({ page }) => {
  await open(page, `/p/${SITE}/docs`, team());
  const whose = page.getByRole("radiogroup", { name: "Whose Docs" });
  await whose.getByRole("radio", { name: "Mine" }).click();
  await expect(titles(page)).toHaveText(["Launch plan", "Onboarding sketch"]);
  await expect(page.locator("[data-docs-count]")).toHaveText("2 in Site");
  await page.locator("body").click({ position: { x: 4, y: 890 } });
  await page.keyboard.press("m");
  await expect(whose.getByRole("radio", { name: "All" })).toHaveAttribute("aria-checked", "true");
  await expect(titles(page)).toHaveCount(4);
});

test("a personal project's list has no Mine filter, and says where its Docs are kept", async ({ page }) => {
  await open(page, `/p/${ME}/docs`, { [ME]: [docOf(ME, "pdc_mine", "My notes", "acct_a", HOUR)] });
  await expect(titles(page)).toHaveText(["My notes"]);
  await expect(page.getByRole("radiogroup", { name: "Whose Docs" })).toHaveCount(0);
  await expect(page.locator("[data-docs-cloud-line]")).toContainText("kept in your cloud");
});

test("j and k move, ↵ opens the Doc's page, and a click opens it too", async ({ page }) => {
  await open(page, `/p/${SITE}/docs`, team());
  await expect(titles(page)).toHaveCount(4);
  await page.locator("body").click({ position: { x: 4, y: 890 } });
  await page.keyboard.press("j");
  await expect(rows(page).first()).toHaveClass(/is-selected/);
  await page.keyboard.press("j");
  await page.keyboard.press("k");
  await page.keyboard.press("j");
  await expect(rows(page).nth(1)).toHaveClass(/is-selected/);
  await page.keyboard.press("Enter");
  await expect(page).toHaveURL(new RegExp(`/p/${SITE}[^/]*/docs/pdc_tabs[^/]*$`));
  await page.goBack();
  await rows(page).filter({ hasText: "Pricing notes" }).click();
  await expect(page).toHaveURL(new RegExp(`/p/${SITE}[^/]*/docs/pdc_notes[^/]*$`));
});

test("/docs goes to the current project's Docs", async ({ page }) => {
  await open(page, `/p/${SITE}/docs`, team());
  await expect(titles(page)).toHaveCount(4);
  await page.goto("/docs");
  await expect(page).toHaveURL(new RegExp(`/p/${SITE}[^/]*/docs$`));
  await expect(titles(page)).toHaveCount(4);
});
