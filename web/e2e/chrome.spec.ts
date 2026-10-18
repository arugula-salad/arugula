// M108 (#1023): the new workspace, behind Appearance → New workspace. The
// rail, the session sidebar (switching, search), the routes and their way
// back, and the header's connection chip. Every other spec runs the classic
// bar, which stays the default until M113.

import { expect, test, type Page } from "@playwright/test";
import { active, reset, tab, tabsInSession } from "./helpers";

const NEW = JSON.stringify({ palette: "default", finish: "flat", corners: "square", workspace: "new" });

async function newWorkspace(page: Page) {
  await page.addInitScript((v) => {
    try {
      localStorage.setItem("arugula-appearance", v);
    } catch {
      // A page without storage draws the classic bar; the test then fails.
    }
  }, NEW);
  await reset(page);
  await expect(page.locator(".ws-rail")).toBeVisible();
}

/** A new session named `name`, shown once it's there. */
async function session(page: Page, name: string): Promise<number> {
  await page.evaluate((name) => window.__arugula.client.intent({ op: "new_session", name, from_pane: null }), name);
  let id: number | undefined;
  await expect
    .poll(async () => (id = await page.evaluate((n) => window.__arugula.client.state!.sessions.find((s) => s.name === n)?.id, name)))
    .not.toBeUndefined();
  return id!;
}

const shownSession = (page: Page) => page.evaluate(() => window.__arugula.client.session);
const row = (page: Page, name: string) => page.locator(".ws-session", { has: page.locator(".ws-session-name", { hasText: new RegExp(`^${name}$`) }) });

test.describe("on a computer", () => {
  test.use({ viewport: { width: 1440, height: 900 } });

  test("the sidebar switches sessions, and a session comes back at its last tab", async ({ page }) => {
    await newWorkspace(page);
    const first = await shownSession(page);
    // A second tab in the first session, shown.
    await page.locator(".ws-tabs .new-tab").click();
    await expect.poll(() => tabsInSession(page)).toHaveLength(2);
    const [, second] = await tabsInSession(page);
    await page.locator(`[data-tab-id="${second}"]`).click();
    await expect.poll(() => tab(page).then((t) => t.id)).toBe(second);

    const beta = await session(page, "beta");
    await row(page, "beta").locator(".ws-session-main").click();
    await expect.poll(() => shownSession(page)).toBe(beta);
    await expect(page.locator(".ws-crumb")).toContainText("beta");
    await expect(row(page, "beta")).toHaveAttribute("aria-selected", "true");

    // Back: the tab that was shown, not the session's first.
    const firstName = await page.evaluate((s) => window.__arugula.client.state!.sessions.find((x) => x.id === s)!.name, first);
    await row(page, firstName).locator(".ws-session-main").click();
    await expect.poll(() => shownSession(page)).toBe(first);
    await expect.poll(() => tab(page).then((t) => t.id)).toBe(second);

    // Rows say what they hold, from state.
    await expect(row(page, firstName).locator(".ws-session-counts")).toHaveAttribute("aria-label", "2 tabs, 2 panes");
    await expect(page.locator('[data-session-group="idle"], [data-session-group="working"]').first()).toBeVisible();
  });

  test("rail routes keep the way back: Panes returns to the same tab and pane", async ({ page }) => {
    await newWorkspace(page);
    const t = (await tab(page)).id;
    const pane = await active(page);
    const rail = page.locator(".ws-rail");

    await rail.locator("[data-open-swarm]").click();
    await expect.poll(() => page.evaluate(() => location.hash)).toBe("#swarm");
    await expect(rail.locator("[data-open-swarm]")).toHaveAttribute("aria-pressed", "true");
    await expect(rail.locator("[data-open-swarm]")).toHaveAttribute("aria-label", /all machines/);
    // The swarm's bar leaves the places to the rail.
    await expect(page.locator(".app-bar .places")).toHaveCount(0);

    await rail.locator("[data-open-panes]").click();
    await expect.poll(() => page.evaluate(() => location.hash)).toBe("");
    await expect(rail.locator("[data-open-panes]")).toHaveAttribute("aria-pressed", "true");
    expect((await tab(page)).id).toBe(t);
    expect(await active(page)).toBe(pane);

    // Sessions hides and shows the sidebar, and the device remembers.
    await rail.locator("[data-open-sessions]").click();
    await expect(page.locator("[data-session-sidebar]")).toHaveCount(0);
    await page.reload();
    await expect(page.locator(".ws-rail")).toBeVisible();
    await expect(page.locator("[data-session-sidebar]")).toHaveCount(0);
    await rail.locator("[data-open-sessions]").click();
    await expect(page.locator("[data-session-sidebar]")).toBeVisible();

    // Settings opens this device's settings.
    await rail.locator("[data-open-settings]").click();
    await expect(page.getByRole("menuitem", { name: "Appearance…" })).toBeVisible();
    await page.keyboard.press("Escape");
  });

  test("sidebar search: filters, arrows and Enter pick, an empty result says so", async ({ page }) => {
    await newWorkspace(page);
    await session(page, "alpha-search");
    await session(page, "alpine-search");
    await session(page, "zulu");
    const search = page.locator("[data-session-search]");

    await search.fill("search");
    await expect(page.locator(".ws-session")).toHaveCount(2);
    await expect(row(page, "zulu")).toHaveCount(0);
    // The first match is picked; Down moves to the second and wraps back.
    // Their order is by group, which follows what their fresh shells are
    // doing: wait until both have settled into one.
    await expect(page.locator("[data-session-group]")).toHaveCount(1);
    const picked = () => page.locator(".ws-session.hi").getAttribute("data-session-id").then(Number);
    const first = await picked();
    await search.press("ArrowDown");
    await expect.poll(picked).not.toBe(first);
    const second = await picked();
    await search.press("ArrowDown");
    await expect.poll(picked).toBe(first);
    await search.press("ArrowDown");
    await expect.poll(picked).toBe(second);
    await search.press("Enter");
    await expect.poll(() => shownSession(page)).toBe(second);
    // Enter hands the keys to the pane and clears the search.
    await expect(search).toHaveValue("");

    await search.fill("nothing-like-it");
    await expect(page.locator("[data-session-empty]")).toContainText("nothing-like-it");
    await expect(page.locator(".ws-session")).toHaveCount(0);
    await search.press("Escape");
    await expect(search).toHaveValue("");
    await expect(page.locator(".ws-session")).toHaveCount(4);
  });

  test("the connection chip says direct, then reconnecting while the link is down", async ({ page }) => {
    await newWorkspace(page);
    const chip = page.locator("[data-connection]");
    await expect(chip).toHaveAttribute("data-connection", "direct");
    // Drop the link and hold off the retry, so the state stays put.
    await page.evaluate(() => {
      const c = window.__arugula.client as unknown as { schedule: (f: () => void, d: number) => void; link?: { close(): void } };
      (window as unknown as { __retry: unknown }).__retry = c.schedule;
      c.schedule = () => {};
      c.link?.close();
    });
    await expect(chip).toHaveAttribute("data-connection", "reconnecting");
    await expect(chip).toHaveText("Reconnecting");
    await page.evaluate(() => {
      const c = window.__arugula.client as unknown as { schedule: unknown; connect(): void };
      c.schedule = (window as unknown as { __retry: unknown }).__retry;
      c.connect();
    });
    await expect(chip).toHaveAttribute("data-connection", "direct");
  });

  test("Appearance → New workspace switches between the two shells", async ({ page }) => {
    await newWorkspace(page);
    await page.locator("[data-account-menu]").click();
    await page.getByRole("menuitem", { name: "Appearance…" }).click();
    await page.locator("[data-workspace-choice] input").uncheck();
    await expect(page.locator(".ws-rail")).toHaveCount(0);
    await expect(page.locator(".app-bar [data-open-panes]")).toBeVisible();
    await page.locator("[data-workspace-choice] input").check();
    await expect(page.locator(".ws-rail")).toBeVisible();
    await expect(page.locator(".app-bar")).toHaveCount(0);
  });
});

test.describe("on a narrow window", () => {
  test.use({ viewport: { width: 1000, height: 700 } });

  test("the sidebar folds into the rail and opens over the panes", async ({ page }) => {
    await newWorkspace(page);
    await expect(page.locator("[data-session-sidebar]")).toHaveCount(0);
    const before = await page.locator(".main").boundingBox();
    const beta = await session(page, "narrow-beta");
    await page.locator(".ws-rail [data-open-sessions]").click();
    await expect(page.locator("[data-session-sidebar].overlay")).toBeVisible();
    // Over the panes: they keep their room.
    expect((await page.locator(".main").boundingBox())?.width).toBe(before?.width);
    const firstName = await page.evaluate(() => window.__arugula.client.state!.sessions[0].name);
    await row(page, firstName).locator(".ws-session-main").click();
    await expect(page.locator("[data-session-sidebar]")).toHaveCount(0);
    expect(await shownSession(page)).not.toBe(beta);
  });
});
