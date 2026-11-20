// Pinned sessions (Arugula iss_c0fd27ac196647fa7c857dc3659a0098): pinned
// from a session's menu or its row's pin button, a Pinned group below
// Needs you in the Sessions sidebar, a pinned session that needs you in
// Needs you until it's answered, the Panes page's Pinned section first,
// and the phone's sheet in the same order. On the app's fixture page
// (app-frame-page.tsx) over the suite's daemon; the platform's pins route
// is stubbed here, as the platform keeps them per account.
import { devices, expect, test, type Page } from "@playwright/test";
import type { SessionPin } from "../src/shared/proto";
import { fixturePage } from "./fixture-page";
import { inProject, panes, reset, type } from "./helpers";

const P = "prj_aaaa0002";
/** The suite's daemon, as the fixture page names it. */
const SUITE = "5e771e0000000001";

test.use({ permissions: ["local-network-access"] });

/** `GET` and `PUT /api/sessions/pins`, kept here: what the page wrote. */
async function stubPins(page: Page, start: SessionPin[] = []) {
  const kept = { pins: start, puts: [] as SessionPin[][] };
  await page.route("**/api/sessions/pins", async (r) => {
    if (r.request().method() === "PUT") {
      kept.pins = (r.request().postDataJSON() as { pins: SessionPin[] }).pins;
      kept.puts.push(kept.pins);
    }
    await r.fulfill({ json: { pins: kept.pins } });
  });
  return kept;
}

/** Sessions on the daemon's own page, `n` in all, in the project; each
 * one's id and its pane, oldest first. */
async function sessions(page: Page, n: number): Promise<{ id: number; pane: number }[]> {
  await reset(page);
  for (let i = 1; i < n; i++) await page.evaluate(() => window.__arugula.client.intent({ op: "new_session", name: null, from_pane: null }));
  await expect.poll(() => page.evaluate(() => window.__arugula.client.state!.sessions.length)).toBe(n);
  await expect.poll(() => page.evaluate(() => window.__arugula.client.state!.panes.length)).toBe(n);
  await inProject(page, P, "Arugula");
  return page.evaluate(() => {
    const c = window.__arugula.client;
    return c.state!.sessions.map((s) => ({ id: s.id, pane: c.state!.panes.find((p) => c.sessionOfPane(p.id) === s.id)!.id }));
  });
}

/** The app at `path`. */
async function openApp(page: Page, path: string) {
  const body = await fixturePage("./app-frame-page.tsx");
  await page.route(new RegExp(`/p/${P}`), (r) => r.fulfill({ contentType: "text/html", body }));
  await page.goto(path);
}

/** A program in the pane saying it's blocked until a line comes (OSC 7501). */
const asks = `printf '\\033]7501;state=blocked:app=agent\\033\\\\'; read x; printf '\\033]7501;state=done:app=agent\\033\\\\'\n`;

const groups = (page: Page) => page.locator("[data-session-group]").evaluateAll((els) => els.map((e) => e.getAttribute("data-session-group")));
const inGroup = (page: Page, group: string) =>
  page.locator(`[aria-labelledby="ws-group-${group}"] [data-session-id]`).evaluateAll((els) => els.map((e) => Number(e.getAttribute("data-session-id"))));
const row = (page: Page, id: number) => page.locator(`.ws-session[data-session-id="${id}"]`);

test.describe("on a computer", () => {
  test.use({ viewport: { width: 1280, height: 800 } });

  test("pinned from the menu and the row's button, below Needs you, in Needs you while it asks, then unpinned", async ({ page }, info) => {
    const [a, b] = await sessions(page, 2);
    const kept = await stubPins(page);
    await openApp(page, `/p/${P}/machines?pane=${a.pane}`);
    await expect(row(page, a.id)).toBeVisible();
    await expect(row(page, b.id)).toBeVisible();
    await expect(page.locator('[data-session-group="pinned"]')).toHaveCount(0);
    await page.screenshot({ path: info.outputPath("pins-sidebar-before.png") });

    // From the ⋯ menu: first in it.
    await row(page, a.id).locator(".ws-session-more[aria-label^='Actions for']").click();
    await expect(page.getByRole("menuitem").first()).toHaveText(/Pin session/);
    await page.getByRole("menuitem", { name: "Pin session" }).click();
    await expect.poll(() => inGroup(page, "pinned")).toEqual([a.id]);
    expect(kept.pins).toEqual([{ machine: SUITE, session: a.id }]);

    // From the button that shows on the row's hover.
    await row(page, b.id).hover();
    await row(page, b.id).locator("[data-session-pin-button]").click();
    await expect.poll(() => inGroup(page, "pinned")).toEqual([a.id, b.id]);
    expect(kept.pins).toEqual([
      { machine: SUITE, session: a.id },
      { machine: SUITE, session: b.id },
    ]);
    await expect(page.locator('[data-session-group="pinned"]')).toContainText("Pinned 2");
    await expect(row(page, a.id).locator("[data-session-pin-button]")).toHaveAttribute("aria-pressed", "true");

    // It asks: in Needs you, above Pinned, and only there.
    await type(page, a.pane, asks);
    await expect.poll(() => inGroup(page, "needs")).toEqual([a.id]);
    expect(await inGroup(page, "pinned")).toEqual([b.id]);
    expect(await groups(page)).toEqual(["needs", "pinned"]);
    await page.screenshot({ path: info.outputPath("pins-sidebar-after.png") });
    // Answered: back in Pinned, where it was pinned.
    await type(page, a.pane, "y\n");
    await expect.poll(() => inGroup(page, "pinned")).toEqual([a.id, b.id]);
    await expect(page.locator('[data-session-group="needs"]')).toHaveCount(0);

    // The sidebar's search filters Pinned too.
    const search = page.locator("[data-session-search]");
    await search.fill(await row(page, b.id).locator(".ws-session-name").innerText());
    await expect.poll(() => inGroup(page, "pinned")).toEqual([b.id]);
    await search.fill("");

    // Unpinned from the menu: it leaves, and with none left there's no Pinned.
    for (const s of [a, b]) {
      await row(page, s.id).locator(".ws-session-more[aria-label^='Actions for']").click();
      await page.getByRole("menuitem", { name: "Unpin session" }).click();
    }
    await expect(page.locator('[data-session-group="pinned"]')).toHaveCount(0);
    expect(kept.pins).toEqual([]);
  });

  test("the Panes page lists pinned sessions first, not again under their machine", async ({ page }, info) => {
    const [a, b] = await sessions(page, 2);
    await page.screenshot({ path: info.outputPath("pins-panes-before.png") });
    const kept = await stubPins(page, [{ machine: SUITE, session: b.id }]);
    await openApp(page, `/p/${P}/machines/panes`);
    const list = page.locator("[data-panes-list]");
    const pinned = list.locator("[data-panes-pinned]");
    await expect(pinned.locator(`[data-panes-pane="${b.pane}"]`)).toBeVisible();
    // First, above the machines.
    const order = await list.locator("[data-panes-pinned], [data-panes-group]").evaluateAll((els) => els.map((e) => (e.hasAttribute("data-panes-pinned") ? "pinned" : "machine")));
    expect(order).toEqual(["pinned", "machine"]);
    const session = pinned.locator(`[data-panes-pinned-session="${SUITE}/${b.id}"]`);
    await expect(session).toContainText(/on /);
    const machine = list.locator("[data-panes-group]");
    await expect(machine.locator(`[data-panes-pane="${a.pane}"]`)).toBeVisible();
    await expect(machine.locator(`[data-panes-pane="${b.pane}"]`)).toHaveCount(0);
    await page.screenshot({ path: info.outputPath("pins-panes-after.png") });

    // Unpinned: back under its machine, and no Pinned section.
    await session.locator("[data-panes-unpin]").click();
    await expect(pinned).toHaveCount(0);
    await expect(machine.locator(`[data-panes-pane="${b.pane}"]`)).toBeVisible();
    expect(kept.pins).toEqual([]);
  });
});

test.describe("on a phone", () => {
  const { viewport, userAgent, deviceScaleFactor, isMobile, hasTouch } = devices["Pixel 7"];
  test.use({ viewport, userAgent, deviceScaleFactor, isMobile, hasTouch });

  test("the sheet's sessions: Needs you, then Pinned, then the rest", async ({ page }, info) => {
    const [a, b, c] = await sessions(page, 3);
    // B asks, from the machine's own page.
    await page.evaluate((s) => window.__arugula.client.selectSession(s), b.id);
    await expect.poll(() => panes(page)).toEqual([b.pane]);
    await type(page, b.pane, asks);
    await expect.poll(() => page.evaluate((p) => window.__arugula.client.info(p)?.attention, b.pane)).toBe("needs_input");
    await stubPins(page, [
      { machine: SUITE, session: c.id },
      { machine: SUITE, session: b.id },
    ]);
    await openApp(page, `/p/${P}/machines?pane=${a.pane}`);
    await expect(page.locator(`[data-pane="${a.pane}"]`)).toBeVisible();
    await page.locator(".sheet-button").click();
    const heads = page.locator(".sheet-sessions [data-session]");
    await expect.poll(() => heads.evaluateAll((els) => els.map((e) => Number(e.getAttribute("data-session"))))).toEqual([b.id, c.id, a.id]);
    await expect(page.locator(`.sheet-sessions [data-session="${c.id}"][data-session-pinned]`)).toBeVisible();
    await page.screenshot({ path: info.outputPath("pins-phone-sheet.png") });
  });
});
