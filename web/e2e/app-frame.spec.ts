// The app's frame (Arugula #989): the project-rooted rail (the project
// tile and its switcher, the project's places, a line, the team's), the
// path in the bar, the machines inside the frame (a real pane of the
// suite's daemon, the machines' list with Panes and Swarm, Swarm beside the
// rail), the shell's keys only on a machine's page, and on a phone the bar
// along the bottom. On a fixture page (app-frame-page.tsx) served on the
// daemon's origin; the hub's leg (tokens through the relay on the app's
// host) is the cloud's spec.
import { devices, expect, test, type Page } from "@playwright/test";
import { fixturePage } from "./fixture-page";
import { panes, reset } from "./helpers";

const P = "prj_aaaa0002";

// A page a route fulfils has no address of its own, so Chrome counts it as
// public and won't let it reach the daemon on loopback without this.
test.use({ permissions: ["local-network-access"] });

/** A fresh pane on the daemon's own page, then the app at `path(pane)`. */
async function openApp(page: Page, path: (pane: number) => string): Promise<number> {
  await reset(page);
  const [pane] = await panes(page);
  const body = await fixturePage("./app-frame-page.tsx");
  await page.route(new RegExp(`/p/${P}`), (r) => r.fulfill({ contentType: "text/html", body }));
  await page.goto(path(pane));
  return pane;
}

/** Put what's new in an issue's and a run's thread in chat's rooms, as the stream would. */
async function seedThreads(page: Page) {
  // The page awaits its key before it renders, and chat's first ask (the fixture has no chat to
  // answer it) clears the rooms when it fails: seed after it.
  await page.waitForFunction(() => (window as unknown as { __chat?: { on: boolean | null } }).__chat?.on === false);
  await page.evaluate((project) => {
    const c = (window as unknown as { __chat: { on: boolean | null; rooms: unknown[]; emit(): void } }).__chat;
    const room = (target: string, unread: number, mentions: number) => ({ id: `room_${target}`, kind: "object", team: null, name: target, machine: null, machine_name: null, target, project, with: null, others: [], private: false, last: 9, read: 0, unread, mentions, wants: unread, muted: false, level: "all", level_own: false, archived: false, writable: true, topic: "", description: "", pins: 0, topic_writable: true });
    c.on = true;
    c.rooms = [room("iss_1", 2, 0), room("iss_2", 1, 0), room("run_1", 4, 1)];
    c.emit();
  }, P);
}

const rail = (page: Page, place: string) => page.locator(`.ws-rail [data-open-${place}]`);
const lit = (page: Page, place: string) => expect(rail(page, place)).toHaveAttribute("aria-current", "page");

test.describe("on a computer", () => {
  test.use({ viewport: { width: 1280, height: 800 } });

  test("the rail is the project's places, then the team's; the bar is the path", async ({ page }, info) => {
    await openApp(page, () => `/p/${P}/boards`);
    // The project's tile, its places, a line, the team's, then the Inbox, Settings and you.
    await expect(page.locator(".ws-rail [data-project-switcher]")).toHaveText("A");
    const order = await page.locator(".ws-rail > *").evaluateAll((els) =>
      els.map((e) => (e.classList.contains("ws-rail-line") ? "line" : ([...e.attributes].find((a) => a.name.startsWith("data-open-"))?.name.slice(10) ?? ""))).filter(Boolean),
    );
    expect(order).toEqual(["overview", "boards", "sessions", "docs", "deploys", "line", "chat", "machines", "inbox", "settings"]);
    await lit(page, "boards");
    await expect(page.locator(".ws-rail [data-signed-in-as]")).toBeVisible();
    // The path: the team, the project (a link up), the place.
    await expect(page.locator("[data-app-path]")).toHaveText(/Arugula team\s*Arugula\s*Boards/);
    await expect(page.locator("[data-app-path] [data-path=project]")).toHaveAttribute("href", new RegExp(`^/p/${P}`));
    // The bar's places are gone: they're the rail's.
    await expect(page.locator("[data-platform-bar] [data-platform-boards]")).toHaveCount(0);
    await page.screenshot({ path: info.outputPath("frame-boards.png") });
    // Each place is a step of the rail.
    await rail(page, "docs").click();
    await expect(page).toHaveURL(new RegExp(`/p/${P}[^/]*/docs$`));
    await lit(page, "docs");
    await rail(page, "overview").click();
    await expect(page).toHaveURL(new RegExp(`/p/${P}[^/]*$`));
    await lit(page, "overview");
    // Runs and Workspaces left the bar: Sessions has them.
    await rail(page, "sessions").click();
    await lit(page, "sessions");
    await page.locator("[data-sessions-runs]").click();
    await expect(page).toHaveURL(new RegExp(`/p/${P}[^/]*/runs$`));
    await lit(page, "sessions");
  });

  test("what's new in the issues' and runs' threads is a count on Boards and on Sessions", async ({ page }) => {
    await openApp(page, () => `/p/${P}`);
    await seedThreads(page);
    await expect(rail(page, "boards").locator(".ws-rail-count")).toHaveText("3");
    await expect(rail(page, "sessions").locator(".ws-rail-count")).toHaveText("@4");
    await expect(rail(page, "docs").locator(".ws-rail-count")).toHaveCount(0);
  });

  test("a team's settings and members are two clicks: Settings, then the link in sight", async ({ page }) => {
    await openApp(page, () => `/p/${P}/boards`);
    await rail(page, "settings").click();
    await expect(page).toHaveURL(new RegExp(`/p/${P}[^/]*/settings$`));
    const settings = page.locator("[data-overrides-team-settings]");
    await expect(settings).toHaveAttribute("href", /^\/t\/tm_1[^/]*\/settings$/);
    await settings.click();
    await expect(page.locator('[data-team-page="tm_1"][data-team-tab="settings"]')).toBeVisible();
    // Back to the project's settings, for the other.
    await page.goto(`/p/${P}/settings`);
    const members = page.locator("[data-overrides-members]");
    await expect(members).toHaveAttribute("href", /^\/t\/tm_1[^/]*\/members$/);
    await members.click();
    await expect(page).toHaveURL(/\/t\/tm_1[^/]*\/members$/);
    await expect(page.locator('[data-team-page="tm_1"][data-team-tab="members"]')).toBeVisible();
  });

  test("the project tile opens the switcher: Everything, each team's projects, then yours, with what wants you", async ({ page }, info) => {
    await openApp(page, () => `/p/${P}/boards`);
    await expect(page.locator("[data-app-path]")).toHaveText(/Arugula team\s*Arugula\s*Boards/);
    await page.screenshot({ path: info.outputPath("match-desktop-closed.png") });
    await page.locator(".ws-rail [data-project-switcher]").click();
    const sw = page.locator("[data-switcher]");
    await expect(sw).toBeVisible();
    await expect(sw.locator(".switcher-head")).toHaveText([/^Everything/, /^Arugula team/, /^Just you/]);
    // All projects counts everything that wants you; Arugula its own.
    await expect(sw.locator("[data-switcher-all] .switcher-count")).toHaveText("4");
    await expect(sw.locator(`[data-switcher-project="${P}"] .switcher-count`)).toHaveText("3");
    await expect(sw.locator('[data-switcher-project="prj_aaaa0003"] .switcher-count')).toHaveCount(0);
    // Each with its tile; Personal's is "Me"; the current one is lit.
    await expect(sw.locator(`[data-switcher-project="${P}"] .switcher-tile`)).toHaveText("A");
    await expect(sw.locator('[data-switcher-project="prj_aaaa0001"] .switcher-tile')).toHaveText("Me");
    await expect(sw.locator(`[data-switcher-project="${P}"]`)).toHaveAttribute("aria-current", "page");
    await expect(sw.locator("[data-switcher-new]")).toHaveText(/New project/);
    await page.screenshot({ path: info.outputPath("match-desktop-open.png") });
    // A team's own pages aren't the menu's: the rail's Chat and Settings reach them.
    await expect(sw.locator("[data-switcher-link]")).toHaveCount(0);
    // A row opens its project.
    await sw.locator('[data-switcher-project="prj_aaaa0003"]').click();
    await expect(page).toHaveURL(/\/p\/prj_aaaa0003[^/]*$/);
  });

  test("a pane opens on its machine's page inside the app, under Machines", async ({ page }, info) => {
    const pane = await openApp(page, (p) => `/p/${P}/machines?pane=${p}`);
    const frame = page.locator("[data-platform-machines]");
    // One rail, the app's, with Machines lit.
    await expect(frame.locator(".ws-rail")).toHaveCount(1);
    await lit(page, "machines");
    // The machine's page, as it ships, with Swarm among its places.
    await expect(frame.locator(".app.ws")).toBeVisible();
    await expect(frame.locator("[data-machine-swarm]")).toBeVisible();
    await expect(page.locator(`[data-pane="${pane}"]`)).toBeVisible();
    // Typed in the app, run on the machine, drawn back in the app.
    await page.locator(`[data-pane="${pane}"]`).click();
    await page.keyboard.type("echo app-frame-$((6*7))\n");
    await expect.poll(() => page.evaluate((p) => (window as unknown as { __app: { text(p: number): string } }).__app.text(p), pane)).toContain("app-frame-42");
    await page.screenshot({ path: info.outputPath("frame-machine.png") });
    // Swarm, one click from the machine's page.
    await frame.locator("[data-machine-swarm]").click();
    await expect(page).toHaveURL(new RegExp(`/p/${P}[^/]*/machines/swarm$`));
  });

  test("Machines lists the machines, with Panes and Swarm in the bar; Swarm sits beside the rail", async ({ page }, info) => {
    await openApp(page, () => `/p/${P}/boards`);
    await rail(page, "machines").click();
    await expect(page).toHaveURL(new RegExp(`/p/${P}[^/]*/machines$`));
    const list = page.locator("[data-machines-list]");
    await expect(list.locator("[data-machine-link]")).toHaveCount(1);
    await expect(page.locator("[data-app-path]")).toHaveText(/Arugula team\s*Arugula\s*Machines/);
    await expect(page.locator("[data-machines-panes]")).toHaveAttribute("aria-current", "page");
    // The suite's daemon: online, yours, and its sessions counted.
    const row = list.locator("[data-machine-link]");
    await expect(row.locator("[data-machine-state]")).toHaveText(/online/i);
    await expect(row).toContainText("You own it");
    await expect(row.locator("[data-machine-sessions]")).toHaveText(/^\d+ sessions?/);
    await page.screenshot({ path: info.outputPath("frame-machines.png") });
    await page.locator("[data-machines-swarm]").click();
    await expect(page).toHaveURL(new RegExp(`/p/${P}[^/]*/machines/swarm$`));
    await expect(page.locator("[data-app-swarm] .swarm")).toBeVisible();
    await lit(page, "machines");
    expect((await page.locator("[data-app-swarm] .swarm").boundingBox())!.x).toBeGreaterThan(0);
    await page.screenshot({ path: info.outputPath("frame-swarm.png") });
    // Back to the list by the rail, and the machine from it.
    await rail(page, "machines").click();
    await list.locator("[data-machine-link]").click();
    await expect(page.locator("[data-platform-machines] .app.ws")).toBeVisible();
  });
});

test.describe("the shell's keys", () => {
  test.use({ viewport: { width: 1280, height: 800 } });

  test("are the machine page's only: on Boards, Commands no longer opens", async ({ page }) => {
    await openApp(page, (p) => `/p/${P}/machines?pane=${p}`);
    await expect(page.locator("[data-platform-machines] .app.ws")).toBeVisible();
    const palette = page.locator(".palette-search");
    // On the machine's page, Commands opens as it does on the shell's page.
    await page.keyboard.press("Control+Shift+KeyP");
    await expect(palette).toBeVisible();
    await page.keyboard.press("Escape");
    await expect(palette).toHaveCount(0);
    // On to Boards, by the rail: the same page, no reload.
    await rail(page, "boards").click();
    await expect(page).toHaveURL(new RegExp(`/p/${P}[^/]*/boards$`));
    await expect(page.locator(".platform-main")).toBeVisible();
    // The chord reaches the page as any key would: nothing takes it.
    await page.evaluate(() => addEventListener("keydown", (e) => ((window as unknown as { prevented: boolean }).prevented = e.defaultPrevented)));
    await page.keyboard.press("Control+Shift+KeyP");
    await expect(palette).toHaveCount(0);
    expect(await page.evaluate(() => (window as unknown as { prevented: boolean }).prevented)).toBe(false);
  });
});

test.describe("where you were", () => {
  // The daemon's own page (reset's) registers its worker, which would
  // answer `/` before the route that serves the app there.
  test.use({ viewport: { width: 1280, height: 800 }, serviceWorkers: "block" });

  test("opening the app goes back to your place in your last project; the switcher opens a project there", async ({ page }) => {
    await openApp(page, () => `/p/${P}/boards`);
    // The app's own `/`, too, from here on.
    const body = await fixturePage("./app-frame-page.tsx");
    await page.route(/^https?:\/\/[^/]+\/$/, (r) => r.fulfill({ contentType: "text/html", body }));
    await rail(page, "docs").click();
    await expect(page).toHaveURL(new RegExp(`/p/${P}[^/]*/docs$`));
    // Closing the page saves it; opening the app comes back to it.
    await page.goto("/");
    await expect(page).toHaveURL(new RegExp(`/p/${P}[^/]*/docs$`));
    await lit(page, "docs");
    await page.locator(".ws-rail [data-project-switcher]").click();
    await expect(page.locator(`[data-switcher-project="${P}"]`)).toHaveAttribute("href", new RegExp(`/p/${P}[^/]*/docs$`));
  });

  test("Machines on the rail comes back to your last pane, and a reload stays on it", async ({ page }) => {
    const pane = await openApp(page, (p) => `/p/${P}/machines?pane=${p}`);
    await expect(page.locator(`[data-pane="${pane}"]`)).toBeVisible();
    const there = new RegExp(`/p/${P}[^/]*/machines(/[0-9a-f]+[^/?]*)?\\?pane=${pane}$`);
    await expect(page).toHaveURL(there);
    await rail(page, "boards").click();
    await expect(page).toHaveURL(new RegExp(`/p/${P}[^/]*/boards$`));
    await expect(rail(page, "machines")).toHaveAttribute("href", there);
    await rail(page, "machines").click();
    await expect(page).toHaveURL(there);
    await expect(page.locator(`[data-pane="${pane}"]`)).toBeVisible();
    await page.reload();
    await expect(page.locator(`[data-pane="${pane}"]`)).toBeVisible();
    await expect(page).toHaveURL(there);
  });
});

test.describe("on a phone", () => {
  const { viewport, userAgent, deviceScaleFactor, isMobile, hasTouch } = devices["Pixel 7"];
  test.use({ viewport, userAgent, deviceScaleFactor, isMobile, hasTouch });

  test("the places are a bar along the bottom, with More; the bar has the project and the bell", async ({ page }, info) => {
    await openApp(page, () => `/p/${P}/boards`);
    await expect(page.locator(".ws-rail")).toHaveCount(0);
    const tabs = page.locator("[data-phone-tabs]");
    await expect(tabs).toBeVisible();
    await expect(tabs.locator("[data-tab-boards]")).toHaveAttribute("aria-current", "page");
    for (const t of ["overview", "boards", "sessions", "chat", "more"]) await expect(tabs.locator(`[data-tab-${t}]`)).toBeVisible();
    await expect(page.locator("[data-platform-bar] [data-project-switcher]")).toContainText("Arugula");
    await expect(page.locator("[data-phone-bell]")).toBeVisible();
    await page.screenshot({ path: info.outputPath("frame-phone-boards.png") });
    // The project's name opens the same switcher.
    await page.locator("[data-platform-bar] [data-project-switcher]").click();
    await expect(page.locator("[data-switcher] .switcher-head")).toHaveText([/^Everything/, /^Arugula team/, /^Just you/]);
    await page.screenshot({ path: info.outputPath("match-phone-open.png") });
    await page.keyboard.press("Escape");
    await expect(page.locator("[data-switcher]")).toHaveCount(0);
    // More: the rest of the places.
    await tabs.locator("[data-tab-more]").click();
    for (const label of ["Docs", "Deploys", "Machines", "Settings"]) await expect(page.locator(".menu-item", { hasText: label }).first()).toBeVisible();
    await page.screenshot({ path: info.outputPath("frame-phone-more.png") });
    await page.locator(".menu-item", { hasText: "Docs" }).first().click();
    await expect(page).toHaveURL(new RegExp(`/p/${P}[^/]*/docs$`));
    await expect(tabs.locator("[data-tab-more]")).toHaveClass(/here/);
  });


  test("what's new in the issues' and runs' threads is a count on the Boards and Sessions tabs", async ({ page }) => {
    await openApp(page, () => `/p/${P}`);
    await seedThreads(page);
    await expect(page.locator("[data-tab-boards] .ws-rail-count")).toHaveText("3");
    await expect(page.locator("[data-tab-sessions] .ws-rail-count")).toHaveText("@4");
  });

  test("Machines is a simple list, and a row opens its machine", async ({ page }, info) => {
    await openApp(page, () => `/p/${P}/machines`);
    const row = page.locator("[data-machines-list] [data-machine-link]");
    await expect(row).toHaveCount(1);
    await expect(row.locator("[data-machine-state]")).toHaveText(/online/i);
    await page.screenshot({ path: info.outputPath("app-frame-machines-phone.png") });
    await row.click();
    await expect(page).toHaveURL(new RegExp(`/p/${P}[^/]*/machines(/[0-9a-f]{6,}|\\?machine=)`));
    await expect(page.locator("[data-machines-list]")).toHaveCount(0);
    await expect(page.locator("[data-platform-machines]")).toBeVisible();
  });

  test("a pane opens full screen with the machine's own header and key bar, and no places bar", async ({ page }, info) => {
    const pane = await openApp(page, (p) => `/p/${P}/machines?pane=${p}`);
    await expect(page.locator(".ws-rail")).toHaveCount(0);
    await expect(page.locator("[data-phone-tabs]")).toHaveCount(0);
    await expect(page.locator(`[data-pane="${pane}"]`)).toBeVisible();
    await page.screenshot({ path: info.outputPath("frame-phone-pane.png") });
  });
});
