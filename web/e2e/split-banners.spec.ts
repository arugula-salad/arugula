// After a split (Arugula #972): both projects say so under the bar for a
// week, with Undo while the reader may; an undo says where the work went
// back to, and a refused one says why. An old issue address lands on the
// issue in its new project. On #971's fixture page (move-to-project-page.tsx);
// the platform's answers are this spec's.
import { expect, test, type Page, type Route } from "@playwright/test";
import { fixturePage } from "./fixture-page";

const ME = "prj_aaaa0001";
const RHYTHM = "prj_aaaa0009";
const ISSUE = "iss_0000c0de";
/** Where `SPLIT_SHOTS=dir` puts the notes' pictures. */
const SHOTS = process.env.SPLIT_SHOTS;

const projects = [
  { id: ME, name: "Personal", owner: { kind: "personal" }, team: null },
  { id: RHYTHM, name: "Rhythm", owner: { kind: "team", id: "tm_1", name: "Arugula team" }, team: { id: "tm_1", name: "Arugula team", role: "owner", members: [] } },
];

const moved = { repos: 0, boards: 1, issues: 25, docs: 2, workspaces: 0, environments: 0, runs: 0, sessions: 0, drafts: 0, links: 25 };
const split = (direction: "out" | "in", undone: boolean) => ({
  id: "spl_1",
  direction,
  other: direction === "out" ? { id: RHYTHM, name: "Rhythm" } : { id: ME, name: "Personal" },
  moved,
  moved_at: Date.now() - 60_000,
  ...(undone ? { undone_at: Date.now() } : {}),
  may_undo: !undone,
});

const issue = {
  id: ISSUE,
  project: RHYTHM,
  number: 975,
  title: "Rhythm: dogfood on Android",
  body: "",
  state: "open",
  close_reason: null,
  author: "acct_a",
  agent: null,
  labels: [],
  assignees: [],
  github: null,
  created_at: 1,
  updated_at: 1,
  closed_at: null,
};

interface Seen {
  undos: string[];
}

/** The fixture page at `path`; `undo` answers the undo. */
async function open(page: Page, path: string, undo: () => { status: number; body: unknown }): Promise<Seen> {
  const seen: Seen = { undos: [] };
  let undone = false;
  const json = (r: Route, body: unknown, status = 200) => r.fulfill({ status, contentType: "application/json", body: JSON.stringify(body) });
  // What this spec doesn't answer, nobody does.
  await page.route(/\/api\//, (r) => json(r, { error: "not here" }, 404));
  await page.route(/\/api\/projects$/, (r) => json(r, { me: { account: "acct_a", login: "sam", name: "Sam" }, projects, hosted: false }));
  await page.route(new RegExp(`/api/projects/${ME}/splits$`), (r) => json(r, { splits: [split("out", undone)] }));
  await page.route(new RegExp(`/api/projects/${RHYTHM}/splits$`), (r) => json(r, { splits: [split("in", undone)] }));
  await page.route(/\/api\/projects\/[^/]+\/splits\/spl_1\/undo$/, (r) => {
    seen.undos.push(new URL(r.request().url()).pathname);
    const a = undo();
    if (a.status === 200) undone = true;
    return json(r, a.body, a.status);
  });
  await page.route(new RegExp(`/api/issues/${ISSUE}$`), (r) => json(r, issue));
  const body = await fixturePage("./move-to-project-page.tsx");
  await page.route(/\/p\/prj_/, (r) => (r.request().resourceType() === "document" ? r.fulfill({ contentType: "text/html", body }) : r.fallback()));
  await page.goto(path);
  return seen;
}

const back = { status: 200, body: { id: "spl_1", to: { id: ME, name: "Personal" }, deleted: false, moved, issues: [{ id: ISSUE, from: 975, to: 12 }] } };
const note = (page: Page) => page.locator("[data-split-note]");

test.use({ viewport: { width: 1280, height: 720 } });

test("where the work left: what moved and where, then Undo, until the page is left", async ({ page }) => {
  const seen = await open(page, `/p/${ME}`, () => back);
  await expect(note(page)).toHaveAttribute("data-split-note", "out");
  await expect(note(page)).toHaveText("25 issues, 1 board and 2 Docs moved to Rhythm.Undo");
  if (SHOTS) await page.screenshot({ path: `${SHOTS}/banner-out.png` });

  await page.locator("[data-split-undo]").click();
  await expect(note(page)).toHaveAttribute("data-split-note", "undone");
  await expect(note(page)).toHaveText("Moved back to Personal.");
  await expect(page.locator("[data-split-undo]")).toHaveCount(0);
  if (SHOTS) await page.screenshot({ path: `${SHOTS}/banner-undone.png` });
  expect(seen.undos).toEqual([`/api/projects/${ME}/splits/spl_1/undo`]);

  // Another page: the split is undone, so nothing more is said.
  await page.locator(`a[href^="/p/${ME}"][href*="/issues"]`).first().click();
  await expect(page).toHaveURL(/\/issues/);
  await expect(note(page)).toHaveCount(0);
});

test("where the work arrived: where from, and Undo on the project it left", async ({ page }) => {
  const seen = await open(page, `/p/${RHYTHM}`, () => back);
  await expect(note(page)).toHaveAttribute("data-split-note", "in");
  await expect(note(page)).toHaveText("Moved here from Personal.Undo");
  if (SHOTS) await page.screenshot({ path: `${SHOTS}/banner-in.png` });
  await page.locator("[data-split-undo]").click();
  await expect(note(page)).toHaveText("Moved back to Personal.");
  expect(seen.undos).toEqual([`/api/projects/${ME}/splits/spl_1/undo`]);
});

test("a refused undo says why, in the note's place", async ({ page }) => {
  await open(page, `/p/${ME}`, () => ({ status: 409, body: { error: "that split is undone already", reason: "already_undone" } }));
  await page.locator("[data-split-undo]").click();
  await expect(note(page)).toHaveAttribute("data-split-note", "refused");
  await expect(note(page)).toHaveText("That split is undone already.");
});

test("an old issue address lands on the issue in its new project", async ({ page }) => {
  await open(page, `/p/${ME}`, () => back);
  await expect(note(page)).toBeVisible();
  await page.goto(`/p/${ME}/issues/${ISSUE}`);
  // The new project, with the title's slug: one address, whichever comes first.
  await expect(page).toHaveURL(new RegExp(`/p/${RHYTHM}[^/]*/issues/${ISSUE}-rhythm-dogfood-on-android$`));
  await expect(page.getByText(issue.title).first()).toBeVisible();
  await expect(page.getByText("That issue is in another project.")).toHaveCount(0);
  // In its place in history: Back skips the old address.
  await page.goBack();
  await expect(page).toHaveURL(new RegExp(`/p/${ME}[^/]*$`));
});
