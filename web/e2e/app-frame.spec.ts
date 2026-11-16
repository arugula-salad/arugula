// The app's frame (Arugula #989): the project-rooted rail (the project
// tile and its switcher, then three spaces: Plan, Build, Talk, #1080), the
// path and the space's places in the bar, the machines inside the frame (a
// real pane of the suite's daemon, the machines' list with Build's places,
// Swarm beside the rail), the shell's keys only on a machine's page, and on
// a phone the spaces and the Inbox in a bar along the bottom. On a fixture page (app-frame-page.tsx) served on the
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
/** A place in the bar's space (Arugula #1080). */
const place = (page: Page, key: string) => page.locator(`[data-platform-bar] [data-space-${key}]`);
const here = (page: Page, key: string) => expect(place(page, key)).toHaveAttribute("aria-current", "page");

test.describe("on a computer", () => {
  test.use({ viewport: { width: 1280, height: 800 } });

  test("the rail is three spaces, Plan, Build and Talk; the bar is the path and the space's places", async ({ page }, info) => {
    await openApp(page, () => `/p/${P}/boards`);
    // The project's tile, the three spaces, then the Inbox, Settings and you.
    await expect(page.locator(".ws-rail [data-project-switcher]")).toHaveText("A");
    const order = await page.locator(".ws-rail > *").evaluateAll((els) => els.map((e) => [...e.attributes].find((a) => a.name.startsWith("data-open-"))?.name.slice(10) ?? "").filter(Boolean));
    expect(order).toEqual(["plan", "build", "talk", "inbox", "settings"]);
    await lit(page, "plan");
    await expect(page.locator(".ws-rail [data-signed-in-as]")).toBeVisible();
    // The path: the team, the project (a link up), the place.
    await expect(page.locator("[data-app-path]")).toHaveText(/Arugula team\s*Arugula\s*Boards/);
    await expect(page.locator("[data-app-path] [data-path=project]")).toHaveAttribute("href", new RegExp(`^/p/${P}`));
    // Plan's places, in the bar; Sessions is Plan's until it folds away (#1082).
    const order2 = await page.locator("[data-space-places=plan] > *").evaluateAll((els) => els.map((e) => [...e.attributes].find((a) => a.name.startsWith("data-space-"))?.name.slice(11) ?? ""));
    expect(order2).toEqual(["overview", "boards", "docs", "sessions"]);
    await here(page, "boards");
    await page.screenshot({ path: info.outputPath("frame-boards.png") });
    // Each place is a step of the bar, in Plan.
    await place(page, "docs").click();
    await expect(page).toHaveURL(new RegExp(`/p/${P}[^/]*/docs$`));
    await here(page, "docs");
    await lit(page, "plan");
    await place(page, "overview").click();
    await expect(page).toHaveURL(new RegExp(`/p/${P}[^/]*$`));
    await here(page, "overview");
    // Plan opens on the Overview.
    await place(page, "boards").click();
    await rail(page, "plan").click();
    await expect(page).toHaveURL(new RegExp(`/p/${P}[^/]*$`));
    // Runs left the bar: Sessions has them.
    await place(page, "sessions").click();
    await here(page, "sessions");
    await page.locator("[data-sessions-runs]").click();
    await expect(page).toHaveURL(new RegExp(`/p/${P}[^/]*/runs$`));
    await here(page, "sessions");
    await lit(page, "plan");
    // Talk is Chat, with no places of its own.
    await rail(page, "talk").click();
    await expect(page).toHaveURL(/\/chat/);
    await lit(page, "talk");
    await expect(page.locator("[data-space-places]")).toHaveCount(0);
  });

  test("what's new in the issues' and runs' threads is a count on Boards and on Sessions, and on Plan", async ({ page }) => {
    await openApp(page, () => `/p/${P}`);
    await seedThreads(page);
    await expect(place(page, "boards").locator(".count-after")).toHaveText("3");
    await expect(place(page, "sessions").locator(".count-after")).toHaveText("@4");
    await expect(place(page, "docs").locator(".count-after")).toHaveCount(0);
    await expect(rail(page, "plan").locator(".ws-rail-count")).toHaveText("@7");
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

  test("your picture's menu is your name, Your account… and Sign out; presence is Chat's (Arugula #1079)", async ({ page }) => {
    await openApp(page, () => `/p/${P}/boards`);
    await page.locator(".ws-rail [data-signed-in-as]").click();
    const items = page.locator(".menu-item");
    await expect(items).toHaveText(["Your account…", "Sign out of this page"]);
    await expect(page.locator(".menu", { hasText: "Your presence" })).toHaveCount(0);
    await items.first().click();
    await expect(page).toHaveURL(/\/settings$/);
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

  test("a pane opens on its machine's page inside the app, under Build", async ({ page }, info) => {
    const pane = await openApp(page, (p) => `/p/${P}/machines?pane=${p}`);
    const frame = page.locator("[data-platform-machines]");
    // One rail, the app's, with Build lit.
    await expect(frame.locator(".ws-rail")).toHaveCount(1);
    await lit(page, "build");
    // The machine's page, as it ships, with Build's places: Panes, Swarm,
    // Machines. Its own Docs are Plan's Docs in the app (Arugula #1080).
    await expect(frame.locator(".app.ws")).toBeVisible();
    await expect(frame.locator("[data-machine-panes]")).toHaveAttribute("aria-current", "page");
    await expect(frame.locator("[data-machine-swarm]")).toBeVisible();
    await expect(frame.locator("[data-machine-machines]")).toBeVisible();
    await expect(frame.locator("[data-machine-docs]")).toHaveCount(0);
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

  test("on macOS a machine's page has one window-button strip: its bar is 28px down, not 56px (Arugula #1089)", async ({ page }) => {
    await openApp(page, (p) => `/p/${P}/machines?pane=${p}`);
    const frame = page.locator("[data-platform-machines]");
    await expect(frame.locator(".app.ws .ws-head")).toBeVisible();
    await page.evaluate(() => { document.documentElement.dataset.desktop = "macos"; });
    expect((await frame.locator(".app.ws .ws-head").boundingBox())!.y).toBe(28);
    expect(await frame.locator(".app.ws").evaluate((el) => getComputedStyle(el, "::before").content)).toBe("none");
  });

  test("Build's Machines lists the machines, with Build's places in the bar; Swarm sits beside the rail", async ({ page }, info) => {
    await openApp(page, () => `/p/${P}/boards`);
    // No pane yet in this project: Build opens on the list.
    await rail(page, "build").click();
    await expect(page).toHaveURL(new RegExp(`/p/${P}[^/]*/machines$`));
    const list = page.locator("[data-machines-list]");
    await expect(list.locator("[data-machine-link]")).toHaveCount(1);
    await expect(page.locator("[data-app-path]")).toHaveText(/Arugula team\s*Arugula\s*Machines/);
    const order = await page.locator("[data-space-places=build] > *").evaluateAll((els) => els.map((e) => [...e.attributes].find((a) => a.name.startsWith("data-space-"))?.name.slice(11) ?? ""));
    expect(order).toEqual(["panes", "swarm", "machines"]);
    await here(page, "machines");
    // The suite's daemon: online, yours, and its sessions counted.
    const row = list.locator("[data-machine-link]");
    await expect(row.locator("[data-machine-state]")).toHaveText(/online/i);
    await expect(row).toContainText("You own it");
    await expect(row.locator("[data-machine-sessions]")).toHaveText(/^\d+ sessions?/);
    await page.screenshot({ path: info.outputPath("frame-machines.png") });
    await place(page, "swarm").click();
    await expect(page).toHaveURL(new RegExp(`/p/${P}[^/]*/machines/swarm$`));
    await expect(page.locator("[data-app-swarm] .swarm")).toBeVisible();
    await lit(page, "build");
    expect((await page.locator("[data-app-swarm] .swarm").boundingBox())!.x).toBeGreaterThan(0);
    await page.screenshot({ path: info.outputPath("frame-swarm.png") });
    // Back to the list by the rail, and the machine from it.
    await rail(page, "build").click();
    await list.locator("[data-machine-link]").click();
    await expect(page.locator("[data-platform-machines] .app.ws")).toBeVisible();
    // The machine's bar has Machines, back to the list.
    await page.locator("[data-platform-machines] [data-machine-machines]").click();
    await expect(page).toHaveURL(new RegExp(`/p/${P}[^/]*/machines$`));
    await here(page, "machines");
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
    // On to Plan, by the rail: the same page, no reload.
    await rail(page, "plan").click();
    await expect(page).toHaveURL(new RegExp(`/p/${P}[^/]*$`));
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
    await place(page, "docs").click();
    await expect(page).toHaveURL(new RegExp(`/p/${P}[^/]*/docs$`));
    // Closing the page saves it; opening the app comes back to it.
    await page.goto("/");
    await expect(page).toHaveURL(new RegExp(`/p/${P}[^/]*/docs$`));
    await here(page, "docs");
    await page.locator(".ws-rail [data-project-switcher]").click();
    await expect(page.locator(`[data-switcher-project="${P}"]`)).toHaveAttribute("href", new RegExp(`/p/${P}[^/]*/docs$`));
  });

  test("Build on the rail comes back to your last pane, and a reload stays on it", async ({ page }) => {
    const pane = await openApp(page, (p) => `/p/${P}/machines?pane=${p}`);
    await expect(page.locator(`[data-pane="${pane}"]`)).toBeVisible();
    const there = new RegExp(`/p/${P}[^/]*/machines(/[0-9a-f]+[^/?]*)?\\?pane=${pane}$`);
    await expect(page).toHaveURL(there);
    await rail(page, "plan").click();
    await expect(page).toHaveURL(new RegExp(`/p/${P}[^/]*$`));
    await expect(rail(page, "build")).toHaveAttribute("href", there);
    await rail(page, "build").click();
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

  test("the spaces and the Inbox are a bar along the bottom; the top bar has the project and the space's places", async ({ page }, info) => {
    await openApp(page, () => `/p/${P}/boards`);
    await expect(page.locator(".ws-rail")).toHaveCount(0);
    const tabs = page.locator("[data-phone-tabs]");
    await expect(tabs).toBeVisible();
    const order = await tabs.locator("> *").evaluateAll((els) => els.map((e) => [...e.attributes].find((a) => a.name.startsWith("data-tab-"))?.name.slice(9) ?? ""));
    expect(order).toEqual(["plan", "build", "talk", "inbox"]);
    await expect(tabs.locator("[data-tab-plan]")).toHaveAttribute("aria-current", "page");
    await expect(page.locator("[data-platform-bar] [data-project-switcher]")).toContainText("Arugula");
    // The Inbox is a tab, not a bell in the bar; Settings is under your picture.
    await expect(page.locator("[data-platform-bar] [data-signed-in-as]")).toBeVisible();
    await here(page, "boards");
    await page.screenshot({ path: info.outputPath("frame-phone-boards.png") });
    // The project's name opens the same switcher.
    await page.locator("[data-platform-bar] [data-project-switcher]").click();
    await expect(page.locator("[data-switcher] .switcher-head")).toHaveText([/^Everything/, /^Arugula team/, /^Just you/]);
    await page.screenshot({ path: info.outputPath("match-phone-open.png") });
    await page.keyboard.press("Escape");
    await expect(page.locator("[data-switcher]")).toHaveCount(0);
    // Plan's places, in the top bar. No Deploys for v1 (Arugula #1078).
    await place(page, "docs").click();
    await expect(page).toHaveURL(new RegExp(`/p/${P}[^/]*/docs$`));
    await expect(tabs.locator("[data-tab-plan]")).toHaveAttribute("aria-current", "page");
    await expect(page.locator("[data-space-deploys]")).toHaveCount(0);
    // The Inbox, by its tab.
    await tabs.locator("[data-tab-inbox]").click();
    await expect(page).toHaveURL(/\/inbox/);
    await expect(tabs.locator("[data-tab-inbox]")).toHaveAttribute("aria-current", "page");
  });

  test("what's new in the issues' and runs' threads is a count on the Plan tab", async ({ page }) => {
    await openApp(page, () => `/p/${P}`);
    await seedThreads(page);
    await expect(page.locator("[data-tab-plan] .ws-rail-count")).toHaveText("@7");
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
