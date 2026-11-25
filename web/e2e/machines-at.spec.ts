// A machine's sessions are the Machines page's (Arugula #1349): the
// command palette's Go to <machine> on Machines, the session crumb in the
// header and the rail's mark, which opened M111's "Sessions on <host>" over
// the panes, open the Machines page scrolled to that machine, the crumb at
// its session's card, marked and focused. An old `#overview` link lands
// there too; on a machine's own page, with no Machines page, it's the
// panes, at that session, and the mark moves to the sidebar's search. In
// the app's fixture page (app-frame-page.tsx) over the suite's daemon, and
// on the daemon's own page, given a stand-in platform page for the mark.
import { expect, test, type Page } from "@playwright/test";
import { fixturePage } from "./fixture-page";
import { inProject, reset } from "./helpers";

const P = "prj_aaaa0002";
const APP = "http://app.test";

// Short, so a card low on the page is out of view until it's scrolled to.
test.use({ permissions: ["local-network-access"], serviceWorkers: "block", viewport: { width: 1280, height: 600 } });

interface Placed {
  id: number;
  pane: number;
}

/** `n` sessions in Arugula on the suite's daemon, each with its pane. */
async function sessions(page: Page, n: number): Promise<Placed[]> {
  await reset(page);
  for (let i = 1; i < n; i++) await page.evaluate(() => window.__arugula.client.intent({ op: "new_session", name: null, from_pane: null }));
  await expect.poll(() => page.evaluate(() => window.__arugula.client.state!.panes.length)).toBe(n);
  await inProject(page, P, "Arugula");
  return page.evaluate(() => {
    const c = window.__arugula.client;
    return c.state!.sessions.map((s) => ({ id: s.id, pane: c.state!.panes.find((p) => c.sessionOfPane(p.id) === s.id)!.id }));
  });
}

async function openApp(page: Page, path: string) {
  const body = await fixturePage("./app-frame-page.tsx");
  await page.route(new RegExp(`/p/${P}`), (r) => r.fulfill({ contentType: "text/html", body }));
  await page.goto(path);
}

const at = (session?: number) => new RegExp(`/p/${P}[^/]*/machines\\?show=[^&#]+${session === undefined ? "" : `&session=${session}`}$`);
const list = (page: Page) => page.locator("[data-machines-list]");

/** The suite's machine on Machines, the one marked; at `session`'s card, marked instead. */
async function shownAt(page: Page, session?: number) {
  const picked = list(page).locator("[data-machines-picked]");
  await expect(picked).toHaveCount(1);
  if (session === undefined) {
    await expect(picked.locator("[data-machines-here]")).toBeVisible();
    await expect(picked.locator("[data-machines-machine-link]")).toBeFocused();
  } else {
    await expect(picked).toHaveAttribute("data-machines-session", String(session));
    await expect(picked).toHaveAttribute("aria-current", "true");
    await expect(picked.locator("[data-machines-session-link]")).toBeFocused();
    // Under the suite's machine, this computer.
    await expect(list(page).locator("[data-machines-group]").filter({ has: page.locator("[data-machines-here]") }).locator(`[data-machines-session="${session}"]`)).toHaveCount(1);
  }
  await expect(picked).toBeInViewport();
  // Nothing of the old overlay.
  await expect(page.locator("[data-overview-session], .ws-overview")).toHaveCount(0);
}

test("the palette and the session crumb open Machines at this machine, and at the session's card", async ({ page }, info) => {
  const all = await sessions(page, 8);
  const last = all[all.length - 1];
  await openApp(page, `/p/${P}/machines?pane=${last.pane}`);
  await expect(page.locator(`[data-pane="${last.pane}"]`)).toBeVisible({ timeout: 20_000 });

  // The palette: Go to <machine> on Machines.
  await page.locator(".ws-head [data-open-palette]").click();
  const palette = page.getByRole("dialog", { name: "Command palette" });
  await palette.getByRole("combobox", { name: "Command" }).fill("on Machines");
  const go = palette.locator('[data-command="Places/machines"]');
  await expect(go).toHaveCount(1);
  await expect(go).toContainText("on Machines");
  await go.click();
  await expect(page).toHaveURL(at());
  await shownAt(page);
  await page.screenshot({ path: info.outputPath("palette-machine.png") });

  // Back on the machine's page, the session crumb: its card.
  await page.goBack();
  await expect(page.locator(`[data-pane="${last.pane}"]`)).toBeVisible();
  const crumb = page.locator(".ws-head [data-crumb-session]");
  await expect(crumb).toHaveAttribute("title", /on Machines/);
  await crumb.click();
  await expect(page).toHaveURL(at(last.id));
  await shownAt(page, last.id);
  await page.screenshot({ path: info.outputPath("crumb-card.png") });
});

test("an old #overview link lands on Machines at that machine, at its session's card", async ({ page }) => {
  const all = await sessions(page, 8);
  const old = all[all.length - 2];
  await openApp(page, `/p/${P}/machines?pane=${all[0].pane}#overview=${old.id}`);
  await expect(page).toHaveURL(at(old.id), { timeout: 20_000 });
  await shownAt(page, old.id);
  // With no session: the machine.
  await openApp(page, `/p/${P}/machines?pane=${all[0].pane}#overview`);
  await expect(page).toHaveURL(at(), { timeout: 20_000 });
  await shownAt(page);
});

test("on the daemon's own page, the mark opens Machines at this machine where there's a platform page, else the sidebar's search", async ({ page }) => {
  const all = await sessions(page, 2);
  const mark = page.locator(".ws-rail [data-open-mark]");

  // A machine on its own: no Machines page; the mark is the sidebar's search, and an old link the panes at its session.
  await expect(mark).toHaveAttribute("aria-label", /^Arugula: search the sessions on /);
  await mark.click();
  await expect(page.locator("input[data-session-search]")).toBeFocused();
  await page.goto(`/#overview=${all[0].id}`);
  await expect.poll(() => page.evaluate(() => location.hash)).toBe("");
  await expect.poll(() => page.evaluate(() => window.__arugula.client.session)).toBe(all[0].id);

  // Given a platform page (control's would say), Machines there, at this machine.
  await page.route(`${APP}/**`, (r) => r.fulfill({ contentType: "text/html", body: "<!doctype html><title>the app</title><p data-app-stand-in>the app</p>" }));
  await page.evaluate((o) => (window.__arugula as unknown as { platform(origin: string): void }).platform(o), APP);
  await expect(mark).toHaveAttribute("aria-label", /^Arugula: .+ on Machines$/);
  const machine = await page.evaluate(() => {
    const { client, hosts } = window.__arugula;
    return client.e2e?.daemon.id ?? hosts.find(hosts.current)?.id ?? hosts.current;
  });
  const opened = page.waitForEvent("popup");
  await mark.click();
  const app = await opened;
  await expect(app).toHaveURL(`${APP}/machines?show=${encodeURIComponent(machine)}`);
  await expect(app.locator("[data-app-stand-in]")).toBeVisible();
  await app.close();
  // The session crumb there: its card.
  const card = page.waitForEvent("popup");
  await page.locator(".ws-head [data-crumb-session]").click();
  const cardPage = await card;
  await expect(cardPage).toHaveURL(`${APP}/machines?show=${encodeURIComponent(machine)}&session=${all[0].id}`);
  await cardPage.close();
});
