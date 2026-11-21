// Shared by you (Arugula #1315): the Docs you shared with a link, at the
// foot of My Docs, once the hub serves `GET /api/doc-links` on the app's
// host. Open and Copy link ask the machine that shared one, and wait while
// it's offline; Unshare asks it, else takes the link down at the hub. Until
// the hub answers there (404), Docs shows none, Settings keeps its ↗ row to
// control's Shared Docs, and `/docs?shared=1` (control's Shared Docs…)
// goes back to control's. On #971's fixture page (move-to-project-page.tsx):
// the platform's answers are this spec's, the machines' the page's.
import { expect, test, type Page, type Route } from "@playwright/test";
import { fixturePage } from "./fixture-page";

const ME = "prj_aaaa0001";
const projects = [{ id: ME, name: "Default", owner: { kind: "personal" }, team: null }];
const json = (r: Route, body: unknown, status = 200) => r.fulfill({ status, contentType: "application/json", body: JSON.stringify(body) });

const link = (id: string, daemon: string, opens: number) => ({ id, daemon, size: 3 * 1024, made: Date.now() - 86_400_000, updated: Date.now() - 3_600_000, opens });

/** The fixture page at `path`, the hub's links `links` (null: not served here, a 404), and the ids it was asked to delete. */
async function open(page: Page, path: string, links: ReturnType<typeof link>[] | null) {
  const deleted: string[] = [];
  // What this spec doesn't answer, nobody does.
  await page.route(/\/api\//, (r) => json(r, { error: "not here" }, 404));
  await page.route(/\/api\/projects$/, (r) => json(r, { me: { account: "acct_a", login: "sam", name: "Sam" }, projects, hosted: false }));
  await page.route(/\/api\/projects\/[^/]+\/docs$/, (r) => json(r, { project: ME, docs: [], writer: true }));
  await page.route(/\/api\/doc-links(\/[^/]+)?$/, (r) => {
    if (!links) return json(r, { error: "not here" }, 404);
    if (r.request().method() === "DELETE") {
      const id = r.request().url().split("/").pop()!;
      deleted.push(id);
      links.splice(
        links.findIndex((l) => l.id === id),
        1,
      );
      return json(r, { deleted: id });
    }
    return json(r, links);
  });
  const body = await fixturePage("./move-to-project-page.tsx");
  await page.route(/\/(p\/prj_|docs|settings)/, (r) => (r.request().resourceType() === "document" ? r.fulfill({ contentType: "text/html", body }) : r.fallback()));
  await page.goto(path);
  return deleted;
}

const group = (page: Page) => page.locator("[data-docs-shared]");
const item = (page: Page, id: string) => page.locator(`[data-shared-doc="${id}"]`);

test.use({ viewport: { width: 1280, height: 800 } });

test("My Docs ends with Shared by you: a link's title from its machine, Open and Copy link while it answers, Unshare at the hub", async ({ page, context }) => {
  await context.grantPermissions(["clipboard-read", "clipboard-write"]);
  const deleted = await open(page, `/p/${ME}/docs`, [link("lnk_mac", "m_mac", 3), link("lnk_off", "m_off", 0)]);
  await expect(group(page).locator(".list-group-title")).toHaveText("Shared by you");
  await expect(group(page).locator(".list-group-n")).toHaveText("2");

  const mac = item(page, "lnk_mac");
  await expect(mac.locator("[data-shared-doc-title]")).toHaveText("Launch plan");
  await expect(mac).toContainText("on mac · 3 KB");
  await expect(mac).toContainText("opened about 3 times");
  // The offline machine's link: named by its machine, and nothing to ask it.
  const off = item(page, "lnk_off");
  await expect(off.locator("[data-shared-doc-title]")).toHaveText("laptop");
  await expect(off).toContainText("not opened yet");
  await expect(off.locator("[data-open-shared-doc]")).toBeDisabled();
  await expect(off.locator("[data-copy-doc-link]")).toBeDisabled();
  await expect(off.locator("[data-copy-doc-link]")).toHaveAttribute("title", "its machine is offline");

  await mac.locator("[data-copy-doc-link]").click();
  await expect(page.locator("[data-platform-toast]")).toContainText("Link copied");
  expect(await page.evaluate(() => navigator.clipboard.readText())).toBe("https://share.example/d/lnk_mac#key");

  await mac.locator("[data-open-shared-doc]").click();
  await expect.poll(() => page.evaluate(() => (window as unknown as { __links: string[] }).__links)).toEqual(["open m_mac lnk_mac"]);

  // Unshare asks first; this machine doesn't take it down, so the hub does.
  await mac.locator("[data-unshare-doc]").click();
  await page.locator("[data-confirm-remove]").click();
  await expect(item(page, "lnk_mac")).toHaveCount(0);
  expect(deleted).toEqual(["lnk_mac"]);
  expect(await page.evaluate(() => (window as unknown as { __links: string[] }).__links)).toEqual(["open m_mac lnk_mac", "unshare m_mac lnk_mac"]);
  await expect(group(page).locator(".list-group-n")).toHaveText("1");
});

test("control's Shared Docs… lands on My Docs' Shared by you", async ({ page }) => {
  await open(page, "/docs?shared=1", [link("lnk_mac", "m_mac", 0)]);
  await expect(page.locator("[data-docs-page]")).toHaveAttribute("data-docs-mode", "personal");
  await expect(item(page, "lnk_mac")).toBeInViewport();
});

test("Settings drops its Shared Docs row once Docs has them", async ({ page }) => {
  await open(page, "/settings/you/account", []);
  const account = page.locator('[data-settings-section="account"]');
  await expect(account.locator('[data-settings-control="teams"]')).toBeVisible();
  await expect(account.locator('[data-settings-control="shared-docs"]')).toHaveCount(0);
});

test("where the hub doesn't serve them here: no Shared by you, Settings keeps its row, and Shared Docs… goes back to control's", async ({ page }) => {
  await open(page, `/p/${ME}/docs`, null);
  await expect(page.locator("[data-docs-none]")).toBeVisible();
  await expect(group(page)).toHaveCount(0);

  await page.goto("/settings/you/account");
  const row = page.locator('[data-settings-section="account"] [data-settings-control="shared-docs"]');
  await expect(row).toContainText("Docs you shared with a link");
  const origin = new URL(page.url()).origin;
  await expect(row).toHaveAttribute("href", `${origin}/#shared-docs`);

  await page.route(`${origin}/`, (r) => r.fulfill({ contentType: "text/html", body: "<title>control</title>" }));
  await page.goto("/docs?shared=1");
  await expect(page).toHaveURL(`${origin}/#shared-docs`);
});
