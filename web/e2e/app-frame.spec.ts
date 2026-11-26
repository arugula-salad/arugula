// The app's frame (Arugula #989): the project-rooted rail (the project
// tile (Go to…'s projects), then a button per place: Overview, Boards, Docs,
// Machines, Chat, #1345), the path and the place's views in the bar, the
// machines inside the frame (a real pane of the suite's daemon, the
// Machines page (#1346), Swarm under the bar beside the rail, #1348), the shell's keys only on a
// machine's page, and on a phone the places in a bar along the bottom and
// the Inbox a bell in the top bar, with nothing wider than the phone; the
// Sessions page folded away (#1082), its old address on to Boards, Runs
// under Machines. On a fixture
// page (app-frame-page.tsx) served on the daemon's origin; the hub's leg
// (tokens through the relay on the app's host) is the cloud's spec.
import { devices, expect, test, type Page } from "@playwright/test";
import { fixturePage } from "./fixture-page";
import { inProject, panes, reset, type as typeIn } from "./helpers";

const P = "prj_aaaa0002";

// A page a route fulfils has no address of its own, so Chrome counts it as
// public and won't let it reach the daemon on loopback without this.
test.use({ permissions: ["local-network-access"] });

/** A fresh pane on the daemon's own page, `typed` into it, then the app at `path(pane)`. */
async function openApp(page: Page, path: (pane: number) => string, typed?: string): Promise<number> {
  await reset(page);
  const [pane] = await panes(page);
  if (typed) await typeIn(page, pane, typed);
  await inProject(page, P, "Arugula");
  const body = await fixturePage("./app-frame-page.tsx");
  await page.route(new RegExp(`/p/${P}`), (r) => r.fulfill({ contentType: "text/html", body }));
  await page.goto(path(pane));
  return pane;
}

/** Put what's new in an issue's and a run's thread, and in a channel of the team's, in chat's rooms, as the stream would. */
async function seedThreads(page: Page) {
  // The page awaits its key before it renders, and chat's first ask (the fixture has no chat to
  // answer it) clears the rooms when it fails: seed after it.
  await page.waitForFunction(() => (window as unknown as { __chat?: { on: boolean | null } }).__chat?.on === false);
  await page.evaluate((project) => {
    const c = (window as unknown as { __chat: { on: boolean | null; rooms: unknown[]; emit(): void } }).__chat;
    const room = (target: string, unread: number, mentions: number) => ({ id: `room_${target}`, kind: "object", team: null, name: target, machine: null, machine_name: null, target, project, with: null, others: [], private: false, last: 9, read: 0, unread, mentions, wants: unread, muted: false, level: "all", level_own: false, archived: false, writable: true, topic: "", description: "", pins: 0, topic_writable: true });
    const channel = { ...room("general", 2, 0), id: "room_general", kind: "channel", team: "tm_1", target: null, project: null };
    c.on = true;
    c.rooms = [room("iss_1", 2, 0), room("iss_2", 1, 0), room("run_1", 4, 1), channel];
    c.emit();
  }, P);
}

const rail = (page: Page, place: string) => page.locator(`[data-rail] [data-open-${place}]`);
const lit = (page: Page, place: string) => expect(rail(page, place)).toHaveAttribute("aria-current", "page");
/** A view in the bar's place (Arugula #1345). */
const view = (page: Page, key: string) => page.locator(`[data-platform-bar] [data-view-${key}]`);
const here = (page: Page, key: string) => expect(view(page, key)).toHaveAttribute("aria-current", "page");
/** The keys of the `data-<prefix>-…` hooks of `sel`'s children, in order. */
const hooks = (page: Page, sel: string, prefix: string) =>
  page.locator(sel).evaluateAll((els, pre) => els.map((e) => [...e.attributes].find((a) => a.name.startsWith(pre))?.name.slice(pre.length) ?? "").filter(Boolean), `data-${prefix}-`);
/** Which of the rail's places is lit: one at a time. */
async function onlyLit(page: Page, key: string) {
  await lit(page, key);
  await expect(page.locator("[data-rail] [aria-current=page]")).toHaveCount(1);
}

test.describe("on a computer", () => {
  test.use({ viewport: { width: 1280, height: 800 } });

  test("the rail is a button per place, Inbox and Settings at its foot; the bar is the path and the place's views", async ({ page }, info) => {
    await openApp(page, () => `/p/${P}/boards`);
    // Your projects' marks (this one ringed, with what's for you), the five
    // places, then the Inbox, Settings and you.
    const mark = page.locator("[data-rail] [data-project-switcher]");
    await expect(mark).toHaveAttribute("aria-label", /^Arugula, 3 for you: your projects$/);
    await expect(mark).toHaveAttribute("aria-current", "true");
    await expect(page.locator(`[data-rail] [data-project-mark="prj_aaaa0003"]`)).toBeVisible();
    expect(await hooks(page, "[data-rail] > *", "open")).toEqual(["overview", "boards", "docs", "machines", "chat", "inbox", "settings"]);
    await expect(page.locator("[data-rail] [data-open-plan], [data-rail] [data-open-build], [data-rail] [data-open-talk]")).toHaveCount(0);
    await onlyLit(page, "boards");
    await expect(page.locator("[data-rail] [data-signed-in-as]")).toBeVisible();
    // The path: the project, then the place.
    await expect(page.locator("[data-app-path]")).toHaveText(/Arugula ▾\s*Boards/);
    // The project, first, opens Go to…'s projects (v6).
    await page.locator("[data-app-path] [data-path=project]").click();
    await expect(page.locator("[data-goto=projects]")).toBeVisible();
    await page.keyboard.press("Escape");
    // Boards has no views; Sessions folded into the boards' rows (#1082).
    await expect(page.locator("[data-place-views]")).toHaveCount(0);
    await page.screenshot({ path: info.outputPath("frame-boards.png") });
    // Each place is a button of the rail, lit where it is.
    await rail(page, "docs").click();
    await expect(page).toHaveURL(new RegExp(`/p/${P}[^/]*/docs$`));
    await onlyLit(page, "docs");
    await expect(page.locator("[data-place-views]")).toHaveCount(0);
    // The Overview's views: Overview, Swarm.
    await rail(page, "overview").click();
    await expect(page).toHaveURL(new RegExp(`/p/${P}[^/]*$`));
    await onlyLit(page, "overview");
    expect(await hooks(page, "[data-place-views=overview] > *", "view")).toEqual(["overview", "swarm"]);
    await here(page, "overview");
    await page.screenshot({ path: info.outputPath("frame-overview.png") });
    // An old link to Sessions goes on to Boards, in its place in the history.
    await page.goto(`/p/${P}/sessions?label=ui`);
    await expect(page).toHaveURL(new RegExp(`/p/${P}[^/]*/boards$`));
    await onlyLit(page, "boards");
    await page.goBack();
    await expect(page).toHaveURL(new RegExp(`/p/${P}[^/]*$`));
    // Chat, with no views of its own.
    await rail(page, "chat").click();
    await expect(page).toHaveURL(/\/chat/);
    await onlyLit(page, "chat");
    await expect(page.locator("[data-place-views]")).toHaveCount(0);
    // The Inbox and Settings, at the foot.
    await rail(page, "inbox").click();
    await expect(page).toHaveURL(/\/inbox/);
    await onlyLit(page, "inbox");
    await rail(page, "settings").click();
    await expect(page).toHaveURL(/\/settings/);
    await onlyLit(page, "settings");
  });

  test("Machines lights on its page, a machine's page, Runs and Build's old address; the Overview on its Swarm", async ({ page }, info) => {
    await openApp(page, () => `/p/${P}/boards`);
    // Machines opens on the Machines page, its views Machines and Runs
    // (no Workspaces for a reader off the hosted list).
    await rail(page, "machines").click();
    await expect(page).toHaveURL(new RegExp(`/p/${P}[^/]*/machines$`));
    await onlyLit(page, "machines");
    expect(await hooks(page, "[data-place-views=machines] > *", "view")).toEqual(["machines", "runs"]);
    await here(page, "machines");
    await page.screenshot({ path: info.outputPath("frame-machines-views.png") });
    await view(page, "runs").click();
    await expect(page).toHaveURL(new RegExp(`/p/${P}[^/]*/runs$`));
    await onlyLit(page, "machines");
    await here(page, "runs");
    await view(page, "machines").click();
    await expect(page).toHaveURL(new RegExp(`/p/${P}[^/]*/machines$`));
    // Build's old address goes on to the Machines page (#1346), lit as Machines.
    await page.goto(`/p/${P}/machines/panes`);
    await expect(page).toHaveURL(new RegExp(`/p/${P}[^/]*/machines$`));
    await onlyLit(page, "machines");
    // The Swarm is the Overview's view, at its route for now (Arugula #1348).
    await rail(page, "overview").click();
    await view(page, "swarm").click();
    await expect(page).toHaveURL(new RegExp(`/p/${P}[^/]*/machines/swarm$`));
    await onlyLit(page, "overview");
  });

  test("Swarm is the Overview's view under the bar, beside the rail; a card opens its pane, Back returns to the Swarm, Escape to the Overview (Arugula #1348)", async ({ page }, info) => {
    // The pane asks for attention (OSC 9), so it's a card on the Swarm's rail.
    const pane = await openApp(page, () => `/p/${P}`, "printf '\\033]9;%s\\007' look\n");
    await expect.poll(() => hooks(page, "[data-place-views] > *", "view")).toEqual(["overview", "swarm"]);
    await here(page, "overview");
    await view(page, "swarm").click();
    await expect(page).toHaveURL(new RegExp(`/p/${P}[^/]*/machines/swarm$`));
    const swarm = page.locator("[data-app-swarm] .swarm");
    await expect(swarm).toBeVisible();
    // The rail and the bar stay, the Overview lit and its Swarm view.
    await expect(page.locator("[data-rail]")).toBeVisible();
    await onlyLit(page, "overview");
    await here(page, "swarm");
    await expect(page.locator("[data-app-path]")).toHaveText(/Arugula ▾\s*Overview\s*Swarm/);
    // Under the bar, beside the rail; the bar's Overview is its way back, so it has none of its own.
    const bar = (await page.locator("[data-platform-bar]").boundingBox())!;
    const box = (await swarm.boundingBox())!;
    expect(box.y).toBeGreaterThanOrEqual(bar.y + bar.height - 1);
    expect(box.x).toBeGreaterThan(0);
    await expect(page.locator("[data-app-swarm] [data-swarm-back]")).toHaveCount(0);
    await page.screenshot({ path: info.outputPath("frame-swarm.png") });
    // A card opens its pane on its machine's page; Back returns to the Swarm.
    const card = page.locator(`[data-app-swarm] .swarm-card[data-panes$=":${pane}"]`);
    await expect(card).toBeVisible({ timeout: 20_000 });
    await card.locator("[data-open-card]").click();
    await expect(page).toHaveURL(new RegExp(`/p/${P}[^/]*/machines(/[0-9a-f]+[^/?]*)?\\?pane=${pane}(&machine=[\\w.-]+)?$`));
    await expect(page.locator(`[data-pane="${pane}"]`)).toBeVisible();
    await page.goBack();
    await expect(page).toHaveURL(new RegExp(`/p/${P}[^/]*/machines/swarm$`));
    await expect(swarm).toBeVisible();
    await expect(page.locator("[data-rail]")).toBeVisible();
    // Escape steps back to the Overview.
    await page.keyboard.press("Escape");
    await expect(page).toHaveURL(new RegExp(`/p/${P}[^/]*$`));
    await expect(page.locator("[data-app-swarm]")).toHaveCount(0);
    await onlyLit(page, "overview");
    await here(page, "overview");
  });

  test("what's new in the issues' threads is a count on Boards; the runs', on Runs (not Machines, Lift 3); chat's, on Chat (Arugula #1345)", async ({ page }) => {
    await openApp(page, () => `/p/${P}/machines`);
    await seedThreads(page);
    await expect(rail(page, "boards").locator("[data-count]")).toHaveText("3");
    await expect(rail(page, "machines").locator("[data-count]")).toHaveCount(0);
    await expect(rail(page, "overview").locator("[data-count]")).toHaveCount(0);
    await expect(rail(page, "docs").locator("[data-count]")).toHaveCount(0);
    await expect(view(page, "runs").locator(".count-after")).toHaveText("@4");
    await expect(rail(page, "chat").locator("[data-count]")).toHaveText("2");
  });

  test("the Issues page's path is Boards › Issues, Boards a link up; an issue's, Issues a link up too (Arugula #1387)", async ({ page }) => {
    await openApp(page, () => `/p/${P}/issues`);
    await onlyLit(page, "boards");
    await expect(page.locator("[data-app-path]")).toHaveText(/Arugula ▾\s*Boards\s*Issues$/);
    await expect(page.locator("[data-app-path] [data-path=boards]")).toHaveAttribute("href", new RegExp(`^/p/${P}[^/]*/boards$`));
    await expect(page.locator("[data-app-path] [data-path=issues]")).toHaveAttribute("aria-current", "page");
    await page.goto(`/p/${P}/issues/iss_0001`);
    await expect(page.locator("[data-app-path]")).toHaveText(/Arugula ▾\s*Boards\s*Issues$/);
    await page.locator("[data-app-path] [data-path=issues]").click();
    await expect(page).toHaveURL(new RegExp(`/p/${P}[^/]*/issues$`));
    await page.locator("[data-app-path] [data-path=boards]").click();
    await expect(page).toHaveURL(new RegExp(`/p/${P}[^/]*/boards$`));
    await expect(page.locator("[data-app-path]")).toHaveText(/Arugula ▾\s*Boards$/);
  });

  test("a team's defaults are a click from a project's setting that follows them (Arugula #1323)", async ({ page }) => {
    await openApp(page, () => `/p/${P}/boards`);
    // The rail's Settings is Settings' Project scope, at the project you're in (Arugula #1104).
    await rail(page, "settings").click();
    await expect(page).toHaveURL(new RegExp(`/settings/project/${P}[^/]*$`));
    await page.locator('[data-settings-row="agents"]').click();
    const line = page.locator('[data-team-default="agents_model"]');
    await expect(line).toHaveText("Team default · follows Arugula team");
    const defaults = line.locator("[data-team-default-owner]");
    await expect(defaults).toHaveAttribute("href", /^\/settings\/team\/tm_1[^/]*\/defaults$/);
    await defaults.click();
    await expect(page.locator('[data-owner-settings="tm_1"] [data-settings-scope="team"]')).toHaveAttribute("aria-selected", "true");
  });

  test("your picture's menu is your name, Send feedback… and Sign out; Settings is the rail's; presence is Chat's (Arugula #1079)", async ({ page }) => {
    await openApp(page, () => `/p/${P}/boards`);
    await page.locator("[data-rail] [data-signed-in-as]").click();
    const items = page.locator(".menu-item");
    await expect(items).toHaveText(["Send feedback…", "Sign out of this page"]);
    await expect(page.locator(".menu", { hasText: "Your presence" })).toHaveCount(0);
  });

  test("the project tile opens Go to…'s projects, each with what wants you there", async ({ page }, info) => {
    await openApp(page, () => `/p/${P}/boards`);
    await expect(page.locator("[data-app-path]")).toHaveText(/Arugula ▾\s*Boards/);
    await page.screenshot({ path: info.outputPath("match-desktop-closed.png") });
    await page.locator("[data-rail] [data-project-switcher]").click();
    const sw = page.getByRole("dialog", { name: "Switch project" });
    await expect(sw).toBeVisible();
    await expect(sw.locator("[data-goto=projects]")).toBeVisible();
    // Each project where you left it; Arugula with what's for you there.
    await expect(sw.locator(`[data-command="project/${P}"]`)).toContainText("3 for you");
    await expect(sw.locator('[data-command="project/prj_aaaa0003"]')).not.toContainText("for you");
    await expect(sw.locator('[data-command="project/new"]')).toHaveText(/New project/);
    // Only projects: no places or boards.
    await expect(sw.locator('[data-command^="place/"], [data-command^="board/"]')).toHaveCount(0);
    await page.screenshot({ path: info.outputPath("match-desktop-open.png") });
    // A row opens its project.
    await sw.locator('[data-command="project/prj_aaaa0003"]').click();
    await expect(page).toHaveURL(/\/p\/prj_aaaa0003[^/]*$/);
    // Your own project is named Default, and its address still says personal (Arugula #1165).
    await page.locator("[data-rail] [data-project-switcher]").click();
    await sw.locator('[data-command="project/prj_aaaa0001"]').click();
    await expect(page).toHaveURL(/\/p\/prj_aaaa0001-personal$/);
  });

  test("a pane opens on its machine's page inside the app, under Machines", async ({ page }, info) => {
    const pane = await openApp(page, (p) => `/p/${P}/machines?pane=${p}`);
    const frame = page.locator("[data-platform-machines]");
    // One rail, the app's, with Machines lit.
    await expect(frame.locator("[data-rail]")).toHaveCount(1);
    await onlyLit(page, "machines");
    // The machine's page, as it ships, with Machines' views: Machines, Runs
    // (Arugula #1345), then its own Recipes. Its own Docs are the rail's Docs
    // in the app, and the Swarm is the Overview's.
    await expect(frame.locator(".app.ws")).toBeVisible();
    expect(await hooks(page, "[data-platform-machines] [data-machine-places] > *", "machine")).toEqual(["machines", "runs", "recipes"]);
    await expect(frame.locator("[data-machine-machines]")).toHaveAttribute("aria-current", "page");
    await expect(frame.locator("[data-machine-docs]")).toHaveCount(0);
    await expect(page.locator(`[data-pane="${pane}"]`)).toBeVisible();
    // Typed in the app, run on the machine, drawn back in the app.
    await page.locator(`[data-pane="${pane}"]`).click();
    await page.keyboard.type("echo app-frame-$((6*7))\n");
    await expect.poll(() => page.evaluate((p) => (window as unknown as { __app: { text(p: number): string } }).__app.text(p), pane)).toContain("app-frame-42");
    await page.screenshot({ path: info.outputPath("frame-machine.png") });
    // Runs, one click from the machine's page.
    await frame.locator("[data-machine-runs]").click();
    await expect(page).toHaveURL(new RegExp(`/p/${P}[^/]*/runs$`));
    await onlyLit(page, "machines");
    // Machines, from a machine's page, is the Machines page (Arugula #1346).
    await page.goBack();
    await frame.locator("[data-machine-machines]").click();
    await expect(page).toHaveURL(new RegExp(`/p/${P}[^/]*/machines$`));
    await expect(page.locator(`[data-machines-list] [data-panes-pane="${pane}"]`)).toBeVisible();
  });

  test("Machines opens on the Machines page: machines, their sessions, their panes (Arugula #1346)", async ({ page }, info) => {
    const pane = await openApp(page, () => `/p/${P}/boards`);
    await rail(page, "machines").click();
    await expect(page).toHaveURL(new RegExp(`/p/${P}[^/]*/machines$`));
    await onlyLit(page, "machines");
    await here(page, "machines");
    await expect(page.locator("[data-app-path]")).toHaveText(/Arugula ▾\s*Machines/);
    const list = page.locator("[data-machines-list]");
    // No machine's page leads: a section per machine, a card per session.
    await expect(page.locator("[data-platform-machines] .app.ws")).toHaveCount(0);
    const group = list.locator("[data-machines-group]");
    await expect(group).toHaveCount(1);
    await expect(group.locator("[data-machines-state]")).toHaveText(/online/i);
    await expect(group.locator("[data-machines-here]")).toHaveText("this computer");
    const row = group.locator(`[data-machines-session] [data-panes-pane="${pane}"]`);
    await expect(row).toBeVisible();
    await expect(list.locator("[data-machines-stats]")).toHaveText(/^1 machine · \d+ sessions? · \d+ panes?/);
    // No machine filter; Runs and Workspaces are the bar's (#1345).
    await expect(list.locator("[data-panes-machine], [data-machines-runs]")).toHaveCount(0);
    await page.screenshot({ path: info.outputPath("frame-machines.png") });
    // A row opens the pane on its machine's page, under Machines.
    await row.click();
    await expect(page).toHaveURL(new RegExp(`/p/${P}[^/]*/machines(/[0-9a-f]+[^/?]*)?\\?pane=${pane}$`));
    await expect(page.locator(`[data-pane="${pane}"]`)).toBeVisible();
    await onlyLit(page, "machines");
    await expect(page.locator("[data-platform-machines] [data-machine-machines]")).toHaveAttribute("aria-current", "page");
  });


  test("on macOS a machine's page has one window-button strip: its bar is 28px down, not 56px (Arugula #1089)", async ({ page }) => {
    await openApp(page, (p) => `/p/${P}/machines?pane=${p}`);
    const frame = page.locator("[data-platform-machines]");
    await expect(frame.locator(".app.ws [data-top]")).toBeVisible();
    await page.evaluate(() => { document.documentElement.dataset.desktop = "macos"; });
    expect((await frame.locator(".app.ws [data-top]").boundingBox())!.y).toBe(28);
    expect(await frame.locator(".app.ws").evaluate((el) => getComputedStyle(el, "::before").content)).toBe("none");
  });

  test("the Machines page is Machines', and a machine's name opens its machine; Swarm sits beside the rail", async ({ page }, info) => {
    await openApp(page, () => `/p/${P}/machines`);
    const list = page.locator("[data-machines-list]");
    await expect(list.locator("[data-machines-machine-link]")).toHaveCount(1);
    await expect(page.locator("[data-app-path]")).toHaveText(/Arugula ▾\s*Machines/);
    await onlyLit(page, "machines");
    await here(page, "machines");
    // The Overview's Swarm, beside the rail.
    await rail(page, "overview").click();
    await view(page, "swarm").click();
    await expect(page).toHaveURL(new RegExp(`/p/${P}[^/]*/machines/swarm$`));
    await expect(page.locator("[data-app-swarm] .swarm")).toBeVisible();
    await onlyLit(page, "overview");
    expect((await page.locator("[data-app-swarm] .swarm").boundingBox())!.x).toBeGreaterThan(0);
    await page.screenshot({ path: info.outputPath("frame-swarm.png") });
    // Back by the rail, to Machines; the machine from it.
    await rail(page, "machines").click();
    await expect(page).toHaveURL(new RegExp(`/p/${P}[^/]*/machines$`));
    await list.locator("[data-machines-machine-link]").click();
    await expect(page.locator("[data-platform-machines] .app.ws")).toBeVisible();
    // The machine's bar has Machines, back to the Machines page.
    await page.locator("[data-platform-machines] [data-machine-machines]").click();
    await expect(page).toHaveURL(new RegExp(`/p/${P}[^/]*/machines$`));
    await here(page, "machines");
  });

});

test.describe("the shell's keys", () => {
  test.use({ viewport: { width: 1280, height: 800 } });

  test("Go to… opens on every page; the panes' keys are the machine page's only", async ({ page }) => {
    await openApp(page, (p) => `/p/${P}/machines?pane=${p}`);
    await expect(page.locator("[data-platform-machines] .app.ws")).toBeVisible();
    const goTo = page.locator("[data-goto]");
    // On the machine's page: its panes, as on the shell's page.
    await page.keyboard.press("Control+Shift+KeyK");
    await expect(goTo).toBeVisible();
    await expect(goTo.locator('[data-command^="Go/pane "]').first()).toBeVisible();
    await page.keyboard.press("Escape");
    await expect(goTo).toHaveCount(0);
    // On to the Overview, by the rail: the same page, no reload.
    await rail(page, "overview").click();
    await expect(page).toHaveURL(new RegExp(`/p/${P}[^/]*$`));
    await expect(page.locator(".platform-main")).toBeVisible();
    // Go to… still: the project's places, no panes.
    await page.keyboard.press("Control+Shift+KeyK");
    await expect(goTo).toBeVisible();
    await expect(goTo.locator(`[data-command="place/${P}/boards"]`)).toBeVisible();
    await expect(goTo.locator('[data-command^="Go/pane "]')).toHaveCount(0);
    await page.keyboard.press("Escape");
    // A pane's key reaches the page as any key would: nothing takes it.
    await page.evaluate(() => addEventListener("keydown", (e) => ((window as unknown as { prevented: boolean }).prevented = e.defaultPrevented)));
    await page.keyboard.press("Control+Shift+Digit1");
    expect(await page.evaluate(() => (window as unknown as { prevented: boolean }).prevented)).toBe(false);
  });
});

test.describe("where you were", () => {
  // The daemon's own page (reset's) registers its worker, which would
  // answer `/` before the route that serves the app there.
  test.use({ viewport: { width: 1280, height: 800 }, serviceWorkers: "block" });

  test("opening the app goes back to your place in your last project; Go to… opens a project there", async ({ page }) => {
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
    await page.locator("[data-rail] [data-project-switcher]").click();
    await expect(page.locator(`[data-command="project/${P}"]`)).toContainText("Docs");
    await page.locator('[data-command="project/prj_aaaa0003"]').click();
    await expect(page).toHaveURL(/\/p\/prj_aaaa0003[^/]*$/);
    await page.locator("[data-rail] [data-project-switcher]").click();
    await page.locator(`[data-command="project/${P}"]`).click();
    await expect(page).toHaveURL(new RegExp(`/p/${P}[^/]*/docs$`));
  });

  test("Machines marks the pane you had open last; it opens again, and a reload stays on it", async ({ page }) => {
    const pane = await openApp(page, (p) => `/p/${P}/machines?pane=${p}`);
    await expect(page.locator(`[data-pane="${pane}"]`)).toBeVisible();
    const there = new RegExp(`/p/${P}[^/]*/machines(/[0-9a-f]+[^/?]*)?\\?pane=${pane}$`);
    await expect(page).toHaveURL(there);
    await rail(page, "overview").click();
    await expect(page).toHaveURL(new RegExp(`/p/${P}[^/]*$`));
    await expect(rail(page, "machines")).toHaveAttribute("href", new RegExp(`/p/${P}[^/]*/machines$`));
    await rail(page, "machines").click();
    const last = page.locator("[data-machines-list] [data-panes-last]");
    await expect(last).toHaveAttribute("data-panes-pane", String(pane));
    await expect(last).toContainText("where you were");
    await last.click();
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

  test("the rail's five places are a bar along the bottom; the top bar has the project, the place's views, the Inbox's bell and you", async ({ page }, info) => {
    await openApp(page, () => `/p/${P}/boards`);
    await expect(page.locator("[data-rail]")).toHaveCount(0);
    const tabs = page.locator("[data-phone-tabs]");
    await expect(tabs).toBeVisible();
    expect(await hooks(page, "[data-phone-tabs] > *", "tab")).toEqual(["overview", "boards", "docs", "machines", "chat"]);
    await expect(tabs.locator("[data-tab-boards]")).toHaveAttribute("aria-current", "page");
    await expect(tabs.locator("[aria-current=page]")).toHaveCount(1);
    await expect(page.locator("[data-platform-bar] [data-project-switcher]")).toContainText("Arugula");
    // The Inbox is a bell in the bar, with its count across your projects; Settings is under your picture.
    const bell = page.locator("[data-platform-bar] [data-phone-inbox]");
    await expect(bell).toBeVisible();
    await expect(bell.locator("[data-count]")).toHaveText("4");
    await expect(tabs.locator("[data-tab-inbox]")).toHaveCount(0);
    await expect(page.locator("[data-platform-bar] [data-signed-in-as]")).toBeVisible();
    // With no rail, the picture's menu keeps Your account… (Arugula #1319).
    await page.locator("[data-platform-bar] [data-signed-in-as]").click();
    await expect(page.locator(".menu-item").first()).toHaveText("Your account…");
    await page.keyboard.press("Escape");
    await expect(page.locator(".menu-item")).toHaveCount(0);
    await page.screenshot({ path: info.outputPath("frame-phone-boards.png") });
    // The project's name opens the same list.
    await page.locator("[data-platform-bar] [data-project-switcher]").click();
    await expect(page.locator(`[data-goto=projects] [data-command="project/${P}"]`)).toBeVisible();
    await page.screenshot({ path: info.outputPath("match-phone-open.png") });
    await page.keyboard.press("Escape");
    await expect(page.locator("[data-goto]")).toHaveCount(0);
    // Each tab is its place. No Swarm on a phone's Overview, and no Deploys for v1 (Arugula #1078).
    await tabs.locator("[data-tab-overview]").click();
    await expect(page).toHaveURL(new RegExp(`/p/${P}[^/]*$`));
    await expect(tabs.locator("[data-tab-overview]")).toHaveAttribute("aria-current", "page");
    await expect(page.locator("[data-place-views]")).toHaveCount(0);
    await expect(view(page, "swarm")).toHaveCount(0);
    await tabs.locator("[data-tab-docs]").click();
    await expect(page).toHaveURL(new RegExp(`/p/${P}[^/]*/docs$`));
    await expect(tabs.locator("[data-tab-docs]")).toHaveAttribute("aria-current", "page");
    await tabs.locator("[data-tab-chat]").click();
    await expect(page).toHaveURL(/\/chat/);
    await expect(tabs.locator("[data-tab-chat]")).toHaveAttribute("aria-current", "page");
    // Machines' views, in the top bar.
    await tabs.locator("[data-tab-machines]").click();
    await expect(page).toHaveURL(new RegExp(`/p/${P}[^/]*/machines$`));
    await expect(tabs.locator("[data-tab-machines]")).toHaveAttribute("aria-current", "page");
    expect(await hooks(page, "[data-place-views=machines] > *", "view")).toEqual(["machines", "runs"]);
    await page.screenshot({ path: info.outputPath("frame-phone-machines.png") });
    // The Inbox, by its bell: no tab lit.
    await bell.click();
    await expect(page).toHaveURL(/\/inbox/);
    await expect(bell).toHaveAttribute("aria-current", "page");
    await expect(tabs.locator("[aria-current=page]")).toHaveCount(0);
  });

  test("what's new in the issues' threads is a count on the Boards tab; the runs', on Machines; chat's, on Chat", async ({ page }) => {
    await openApp(page, () => `/p/${P}`);
    await seedThreads(page);
    await expect(page.locator("[data-tab-boards] [data-count]")).toHaveText("3");
    await expect(page.locator("[data-tab-machines] [data-count]")).toHaveCount(0);
    await expect(page.locator("[data-tab-chat] [data-count]")).toHaveText("2");
    await expect(page.locator("[data-tab-overview] [data-count]")).toHaveCount(0);
  });

  test("Machines' tab opens the Machines page; a row opens its pane", async ({ page }, info) => {
    const pane = await openApp(page, () => `/p/${P}/boards`);
    await page.locator("[data-phone-tabs] [data-tab-machines]").click();
    await expect(page).toHaveURL(new RegExp(`/p/${P}[^/]*/machines$`));
    const row = page.locator(`[data-machines-list] [data-panes-pane="${pane}"]`);
    await expect(row).toBeVisible();
    await page.screenshot({ path: info.outputPath("frame-phone-panes.png") });
    await row.click();
    await expect(page.locator(`[data-pane="${pane}"]`)).toBeVisible();
  });

  test("Machines is one column, and a machine's name opens its machine", async ({ page }, info) => {
    await openApp(page, () => `/p/${P}/machines`);
    const row = page.locator("[data-machines-list] [data-machines-machine-link]");
    await expect(row).toHaveCount(1);
    await expect(page.locator("[data-machines-list] [data-machines-state]")).toHaveText(/online/i);
    await page.screenshot({ path: info.outputPath("app-frame-machines-phone.png") });
    await row.click();
    await expect(page).toHaveURL(new RegExp(`/p/${P}[^/]*/machines(/[0-9a-f]{6,}|\\?machine=)`));
    await expect(page.locator("[data-machines-list]")).toHaveCount(0);
    await expect(page.locator("[data-platform-machines]")).toBeVisible();
  });

  test("a pane opens full screen with the machine's own header and key bar, and no places bar", async ({ page }, info) => {
    const pane = await openApp(page, (p) => `/p/${P}/machines?pane=${p}`);
    await expect(page.locator("[data-rail]")).toHaveCount(0);
    await expect(page.locator("[data-phone-tabs]")).toHaveCount(0);
    await expect(page.locator(`[data-pane="${pane}"]`)).toBeVisible();
    await page.screenshot({ path: info.outputPath("frame-phone-pane.png") });
  });
});

test.describe("at 390px", () => {
  test.use({ viewport: { width: 390, height: 844 }, isMobile: true, hasTouch: true });

  // Pick 15 (Arugula #1345): no page is wider than the phone.
  test("no page scrolls sideways", async ({ page }) => {
    const pane = await openApp(page, () => `/p/${P}`);
    const sideways = () => page.evaluate(() => document.documentElement.scrollWidth - document.documentElement.clientWidth);
    for (const path of ["", "/boards", "/docs", "/chat", "/machines", "/machines/swarm", "/runs", `/machines?pane=${pane}`]) {
      await page.goto(`/p/${P}${path}`);
      await expect(page.locator(path.includes("pane=") ? `[data-pane="${pane}"]` : "[data-platform-bar], [data-app-swarm] .swarm").first()).toBeVisible();
      expect(await sideways(), path || "/").toBeLessThanOrEqual(0);
    }
    // The Overview has no views on a phone, and Swarm is a computer's: its address is the Overview here (Arugula #1348).
    await page.goto(`/p/${P}`);
    await expect(page.locator("[data-platform-bar]")).toBeVisible();
    await expect(page.locator("[data-place-views]")).toHaveCount(0);
    await page.goto(`/p/${P}/machines/swarm`);
    await expect(page).toHaveURL(new RegExp(`/p/${P}[^/]*$`));
    await expect(page.locator("[data-app-swarm]")).toHaveCount(0);
    // The Inbox, by its bell; Settings, under your picture.
    await page.goto(`/p/${P}`);
    await page.locator("[data-phone-inbox]").click();
    await expect(page).toHaveURL(/\/inbox/);
    expect(await sideways(), "/inbox").toBeLessThanOrEqual(0);
    await page.locator("[data-platform-bar] [data-signed-in-as]").click();
    await page.locator(".menu-item", { hasText: "Your account…" }).click();
    await expect(page).toHaveURL(/\/settings/);
    expect(await sideways(), "/settings").toBeLessThanOrEqual(0);
  });
});
