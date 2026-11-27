// A team's deleted Docs (Arugula #1133): a delete's toast has Undo, and a
// folded Deleted row at the list's foot, there only when there are any,
// holds the deleted Docs, each opening in place with who deleted it, when,
// the days it has left and Restore (for owners, editors and its maker). On #971's fixture page (move-to-project-page.tsx):
// the platform's answers, the Docs and the trash, are this spec's.
// DOCS_DELETED_SHOTS=DIR keeps its pictures, light and dark.
import { expect, test, type Page, type Route } from "@playwright/test";
import { fixturePage } from "./fixture-page";

const ME = "prj_aaaa0001";
const SITE = "prj_aaaa0002";
const SHOTS = process.env.DOCS_DELETED_SHOTS;
const DAY = 86_400_000;

const roster = [
  { account: "acct_a", name: "Sam", role: "owner" },
  { account: "acct_b", name: "Jo", role: "editor" },
];
const projects = (role: string) => [
  { id: ME, name: "Default", owner: { kind: "personal" }, team: null },
  { id: SITE, name: "Site", owner: { kind: "team", id: "tm_1", name: "Acme" }, team: { id: "tm_1", name: "Acme", role, members: roster } },
];

const docOf = (id: string, title: string, author: string, at: number) => ({ id, project: SITE, title, author, agent: null, sections: 1, written: 1, rev: 1, created_at: at, updated_at: at });
type Doc = ReturnType<typeof docOf>;
type Trashed = { doc: Doc; trashed_at: number; trashed_by: string; restorable: boolean };

/** The fixture page at the team's Docs, with the platform's answers: `live` and `trash` change as the page deletes and restores. */
async function open(page: Page, { role = "owner", trash = [] as Trashed[] } = {}) {
  const now = Date.now();
  const live: Doc[] = [docOf("pdc_aaaa0001", "Launch plan", "acct_a", now - 3_600_000), docOf("pdc_aaaa0002", "Pricing notes", "acct_b", now - 2 * DAY)];
  const json = (r: Route, body: unknown, status = 200) => r.fulfill({ status, contentType: "application/json", body: JSON.stringify(body) });
  const view = (d: Doc) => ({ doc: d, sections: [{ id: "s1", heading: "Why", intent: "", body: `What ${d.title} is for.`, rev: 1 }], assets: [], writer: role !== "viewer", deleter: role !== "viewer", link: null });
  // What this spec doesn't answer, nobody does.
  await page.route(/\/api\//, (r) => json(r, { error: "not here" }, 404));
  await page.route(/\/api\/projects$/, (r) => json(r, { me: { account: "acct_a", login: "sam", name: "Sam" }, projects: projects(role), hosted: false }));
  await page.route(/\/api\/projects\/[^/]+\/docs$/, (r) => json(r, { project: SITE, docs: live, writer: role !== "viewer" }));
  await page.route(/\/api\/projects\/[^/]+\/docs\/trash$/, (r) => json(r, { project: SITE, docs: trash, days: 30 }));
  await page.route(/\/api\/projects\/[^/]+\/docs\/(pdc_[0-9a-f]+)$/, (r) => {
    const id = r.request().url().split("/").pop()!;
    const d = live.find((x) => x.id === id);
    if (!d) return json(r, { error: "not here" }, 404);
    if (r.request().method() !== "DELETE") return json(r, view(d));
    live.splice(live.indexOf(d), 1);
    trash.unshift({ doc: d, trashed_at: Date.now(), trashed_by: "acct_a", restorable: true });
    return json(r, { deleted: id });
  });
  await page.route(/\/api\/projects\/[^/]+\/docs\/(pdc_[0-9a-f]+)\/restore$/, (r) => {
    const id = r.request().url().split("/").at(-2)!;
    const t = trash.find((x) => x.doc.id === id);
    if (!t) return json(r, { error: "not here" }, 404);
    trash.splice(trash.indexOf(t), 1);
    live.push(t.doc);
    return json(r, view(t.doc));
  });
  // The pictures' light and dark follow the system's: the default palette is dark either way.
  if (SHOTS) await page.addInitScript(() => localStorage.setItem("arugula-appearance", JSON.stringify({ palette: "system" })));
  const body = await fixturePage("./move-to-project-page.tsx");
  await page.route(/\/p\/prj_/, (r) => (r.request().resourceType() === "document" ? r.fulfill({ contentType: "text/html", body }) : r.fallback()));
  await page.goto(`/p/${SITE}/docs`);
  await expect(row(page, "Launch plan")).toBeVisible();
}

const row = (page: Page, title: string) => page.locator("[data-doc]").filter({ hasText: title });
const deleted = (page: Page) => page.locator("[data-docs-deleted]");
/** A row's own line, the button that opens or folds it. */
const line = (r: ReturnType<Page["locator"]>) => r.locator("> .k-row-line");
const trashed = (page: Page, title: string) => page.locator("[data-trashed-doc]").filter({ hasText: title });

/** Pictures of the page as it is, light and dark, when asked for. */
async function shoot(page: Page, name: string) {
  if (!SHOTS) return;
  const scheme = async (colorScheme: "light" | "dark") => {
    await page.emulateMedia({ colorScheme });
    await page.evaluate(() => new Promise((done) => requestAnimationFrame(() => requestAnimationFrame(done))));
  };
  // The fixture page draws its palette on a change of the system's scheme, not at load: one round first.
  await scheme("dark");
  await scheme("light");
  await page.screenshot({ path: `${SHOTS}/${name}-light.png` });
  await scheme("dark");
  await page.screenshot({ path: `${SHOTS}/${name}-dark.png` });
  await scheme("light");
}

test.use({ viewport: { width: 1280, height: 800 } });

test("no Deleted row shows with nothing deleted", async ({ page }) => {
  await open(page);
  await expect(deleted(page)).toHaveCount(0);
});

test("a delete's toast has Undo, which brings the Doc back; until then it's under a folded Deleted", async ({ page }) => {
  await open(page);
  await row(page, "Launch plan").click();
  await expect(page).toHaveURL(new RegExp(`/p/${SITE}[^/]*/docs/pdc_aaaa0001[^/]*$`));
  await page.locator("[data-doc-page] [data-doc-more]").click();
  await page.getByRole("menuitem", { name: "Delete…" }).click();
  await expect(page.locator("[data-doc-confirm]")).toContainText("under Deleted at the foot of the Docs list for 30 days");
  await page.locator("[data-confirm-remove]").click();

  // Back on the list, with the Doc under Deleted, folded.
  await expect(page).toHaveURL(new RegExp(`/p/${SITE}[^/]*/docs$`));
  const toast = page.locator("[data-platform-toast]");
  await expect(toast).toContainText("Deleted “Launch plan”");
  await expect(row(page, "Launch plan")).toHaveCount(0);
  await expect(line(deleted(page))).toHaveAttribute("aria-expanded", "false");
  await expect(line(deleted(page))).toContainText("Deleted · 1");
  await expect(page.locator("[data-trashed-doc]")).toHaveCount(0);
  await shoot(page, "docs-undo-toast");

  // Undo brings it back, and the page back to it.
  await toast.locator("[data-toast-action]").click();
  await expect(toast).toHaveCount(0);
  await expect(page).toHaveURL(new RegExp(`/p/${SITE}[^/]*/docs/pdc_aaaa0001[^/]*$`));
  await page.goBack();
  await expect(row(page, "Launch plan")).toBeVisible();
  await expect(deleted(page)).toHaveCount(0);
});

test("a deleted Doc opens in its row with who, when, the days left and Restore", async ({ page }) => {
  const trash = [{ doc: docOf("pdc_aaaa0003", "Old roadmap", "acct_b", Date.now() - 20 * DAY), trashed_at: Date.now() - 3 * DAY, trashed_by: "acct_b", restorable: true }];
  await open(page, { trash });
  await expect(line(deleted(page))).toContainText("Deleted · 1");
  await shoot(page, "docs-deleted-folded");
  await line(deleted(page)).click();
  await expect(line(deleted(page))).toHaveAttribute("aria-expanded", "true");
  await line(trashed(page, "Old roadmap")).click();
  const p = page.locator("[data-doc-deleted=pdc_aaaa0003]");
  await expect(p).toContainText("Deleted by Jo");
  await expect(p).toContainText("27 days left");
  await shoot(page, "docs-deleted-open");

  await p.locator("[data-doc-restore]").click();
  // The Deleted row goes first: until the trash is read again the restored
  // Doc can be drawn twice, back in the list and still under Deleted.
  await expect(deleted(page)).toHaveCount(0);
  await expect(row(page, "Old roadmap")).toBeVisible();
});

test("a viewer sees a deleted Doc without Restore, and who may restore it", async ({ page }) => {
  const trash = [{ doc: docOf("pdc_aaaa0003", "Old roadmap", "acct_b", Date.now() - 20 * DAY), trashed_at: Date.now() - DAY, trashed_by: "", restorable: false }];
  await open(page, { role: "viewer", trash });
  await line(deleted(page)).click();
  await line(trashed(page, "Old roadmap")).click();
  const p = page.locator("[data-doc-deleted=pdc_aaaa0003]");
  await expect(p).toContainText("Deleted by someone who left");
  await expect(p.locator("[data-doc-restore]")).toHaveCount(0);
  await expect(p).toContainText("An owner, an editor or its maker can restore it.");
});
