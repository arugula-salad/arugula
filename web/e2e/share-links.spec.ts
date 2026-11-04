// Native #245: the Share dialog lists the session's live read-only links,
// and Revoke ends one: its viewer is cut off and the link refuses. The
// access log opens beside them.

import { expect, test, type Page } from "@playwright/test";
import { active, ready, reset, run } from "./helpers";

test.use({ viewport: { width: 1440, height: 900 } });

test("a read-only link is listed, and Revoke ends it", async ({ page, context }) => {
  await reset(page);
  const pane = await active(page);
  await ready(page, pane);
  await run(page, pane, "echo linked-$((5*5))", "linked-25");
  const link = await page.evaluate((p) => window.__arugula.client.share(p), pane);
  expect(link).toMatch(/\/share\//);
  // The link is on the machine's tailnet name; this daemon is the one here.
  const url = new URL(link!).pathname;

  const viewer = await context.newPage();
  await viewer.goto(url);
  const shown = () => viewer.evaluate(() => (window as unknown as { __share: { text(): string } }).__share.text());
  const ended = () => viewer.evaluate(() => (window as unknown as { __share: { ended: string | null } }).__share.ended);
  await expect.poll(shown).toContain("linked-25");

  await page.locator(".ws-session-more").first().click();
  await page.getByRole("menuitem", { name: "Share session…" }).click();
  const dialog = page.locator(".share-dialog");
  const row = dialog.locator(`[data-share-live^="share:"][data-share-live-pane="${pane}"]`);
  await expect(row).toHaveCount(1);
  await expect(row).toContainText("Read-only link");

  await row.locator("[data-revoke]").click();
  await expect(row).toHaveCount(0);
  await expect.poll(ended).toMatch(/expired or was revoked/);
  expect((await page.request.get(url)).status()).toBe(404);

  // The access log: this session's grants and revokes (none here yet).
  await dialog.locator("[data-access-log-open]").click();
  await expect(dialog.locator("[data-access-log], [data-access-log-empty]")).toBeVisible();
  await viewer.close();
});

// A read-only link to the whole session (`links.list`, `link.revoke`). Its
// viewers come through control: teams.spec.ts has one being cut off.
const openShare = async (page: Page) => {
  await page.locator(".ws-session-more").first().click();
  await page.getByRole("menuitem", { name: "Share session…" }).click();
  return page.locator(".share-dialog");
};
const makeLink = async (page: Page) =>
  page.evaluate(async () => {
    const c = window.__arugula.client;
    const key = [...crypto.getRandomValues(new Uint8Array(32))].map((b) => b.toString(16).padStart(2, "0")).join("");
    const r = await c.request("POST", "/api/links", { session: c.state!.sessions[0].id, key, ttl_secs: 600, history: false });
    return r.ok;
  });

test("a session's link is listed apart from the people, and Revoke ends it", async ({ page }) => {
  await reset(page);
  await ready(page, await active(page));
  expect(await makeLink(page)).toBe(true);
  const dialog = await openShare(page);
  const row = dialog.locator('[data-share-live^="link:"]');
  await expect(row).toHaveCount(1);
  await expect(row).toContainText("Read-only link to this session");
  await expect(dialog.locator('[data-grant^="link:"]')).toHaveCount(0);
  await expect(dialog.getByText("Only you can reach it.")).toHaveCount(0);

  await row.locator("[data-revoke]").click();
  await expect(row).toHaveCount(0);
  await expect(dialog.getByText("Only you can reach it.")).toBeVisible();
  const left = await page.evaluate(async () => (await window.__arugula.client.request("GET", "/api/links")).json());
  expect(left).toEqual([]);
  // The access log says who lost what, in the same words as the other grants.
  await dialog.locator("[data-access-log-open]").click();
  await expect(dialog.locator('[data-access-log] [data-access-action="revoke"]').first()).toContainText("someone with a link");
});

test("a machine older than this page says it can't end a link here", async ({ page }) => {
  await reset(page);
  await ready(page, await active(page));
  for (const status of [404, 405]) {
    await page.route("**/api/links*", (r) => (r.request().method() === "GET" ? r.fulfill({ status, body: "" }) : r.continue()));
    const dialog = await openShare(page);
    await expect(dialog.locator("[data-links-need-update]")).toContainText("until it's updated");
    await expect(dialog.locator('[data-share-live^="link:"]')).toHaveCount(0);
    await dialog.getByRole("button", { name: "Done" }).click();
    await page.unrouteAll();
  }
  // A row with less than this page expects still draws: only its id is needed.
  await page.route("**/api/links*", (r) => r.fulfill({ json: [{ id: "abc123" }, { what: "nothing to revoke" }] }));
  const dialog = await openShare(page);
  await expect(dialog.locator('[data-share-live^="link:"]')).toHaveCount(1);
  await expect(dialog.locator('[data-share-live="link:abc123"]')).toContainText("Read-only link to this session");
  await expect(dialog.locator("[data-links-need-update]")).toHaveCount(0);
});
