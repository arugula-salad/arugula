// The machine view is the shown project's (Arugula #1164, #1347): two
// projects' sessions on one machine, and the sidebar, the palette's jumps
// and the phone's sheet hold the shown project's only, as the owner and as
// a teammate. The daemon's own page still shows every one. On the app's
// fixture page (app-frame-page.tsx) over the suite's daemon.
import { devices, expect, test, type Page } from "@playwright/test";
import { fixturePage } from "./fixture-page";
import { panes, reset, type } from "./helpers";

const P = "prj_aaaa0002";
const SITE = "prj_aaaa0003";

test.use({ permissions: ["local-network-access"] });

/** A program in the pane saying it's blocked until a line comes (OSC 7501). */
const asks = `printf '\\033]7501;state=blocked:app=agent\\033\\\\'; read x; printf '\\033]7501;state=done:app=agent\\033\\\\'\n`;

interface Placed {
  id: number;
  pane: number;
}

/** On the daemon's own page: alpha and beta in Arugula, site in Site; alpha
 * and site ask, so each project has one that needs you. */
async function twoProjects(page: Page): Promise<{ alpha: Placed; beta: Placed; site: Placed }> {
  await reset(page);
  for (let i = 1; i < 3; i++) await page.evaluate(() => window.__arugula.client.intent({ op: "new_session", name: null, from_pane: null }));
  await expect.poll(() => page.evaluate(() => window.__arugula.client.state!.panes.length)).toBe(3);
  const [alpha, beta, site] = await page.evaluate(() => {
    const c = window.__arugula.client;
    return c.state!.sessions.map((s) => ({ id: s.id, pane: c.state!.panes.find((p) => c.sessionOfPane(p.id) === s.id)!.id }));
  });
  await page.evaluate(
    ([sessions, P, SITE]) => {
      const c = window.__arugula.client;
      for (const [s, name, project] of sessions) {
        c.intent({ op: "rename_session", session: s, name });
        c.intent({ op: "set_session_project", session: s, project: project === P ? { id: P, name: "Arugula" } : { id: SITE, name: "Site" } });
      }
    },
    [[[alpha.id, "alpha", P], [beta.id, "beta", P], [site.id, "site", SITE]], P, SITE] as const,
  );
  await expect
    .poll(() => page.evaluate(() => window.__arugula.client.state!.sessions.map((s) => `${s.name}:${s.project?.id}`).sort()))
    .toEqual([`alpha:${P}`, `beta:${P}`, `site:${SITE}`]);
  for (const s of [alpha, site]) {
    await page.evaluate((id) => window.__arugula.client.selectSession(id), s.id);
    await expect.poll(() => panes(page)).toEqual([s.pane]);
    await type(page, s.pane, asks);
    await expect.poll(() => page.evaluate((p) => window.__arugula.client.info(p)?.attention, s.pane)).toBe("needs_input");
  }
  return { alpha, beta, site };
}

async function openApp(page: Page, path: string) {
  const body = await fixturePage("./app-frame-page.tsx");
  await page.route(new RegExp(`/p/${P}`), (r) => r.fulfill({ contentType: "text/html", body }));
  await page.goto(path);
}

/** As the daemon tells a guest: a role in each session (a teammate's machine). */
async function asTeammate(page: Page, sessions: number[]) {
  await page.evaluate((ids) => {
    const c = (window as unknown as { __app: { client: { state: { roles?: unknown }; emit(): void } } }).__app.client;
    c.state.roles = ids.map((id) => [id, "editor"]);
    c.emit();
  }, sessions);
}

const sidebar = (page: Page) => page.getByRole("complementary", { name: /^Sessions on / });
const listed = (page: Page) => sidebar(page).locator(".ws-session[data-session-id]").evaluateAll((els) => els.map((e) => Number(e.getAttribute("data-session-id"))).sort());

test.describe("on a computer", () => {
  test.use({ viewport: { width: 1280, height: 800 } });

  test("the sidebar and the palette's jumps hold the shown project's sessions only, as owner and teammate (Arugula #1347)", async ({ page }, info) => {
    const { alpha, beta, site } = await twoProjects(page);
    // The daemon's own page shows no project: every session.
    await expect.poll(() => listed(page)).toEqual([alpha.id, beta.id, site.id].sort());

    // The app's machine page, in Arugula: alpha and beta.
    await openApp(page, `/p/${P}/machines?pane=${alpha.pane}`);
    await expect(page.locator(`[data-pane="${alpha.pane}"]`)).toBeVisible();
    await expect.poll(() => listed(page)).toEqual([alpha.id, beta.id].sort());
    await page.screenshot({ path: info.outputPath("sidebar-owner.png") });

    // The palette's jumps: no site either.
    await page.locator(".ws-head [data-open-palette]").click();
    const palette = page.getByRole("dialog", { name: "Command palette" });
    await palette.getByRole("combobox", { name: "Command" }).fill("Go to");
    await expect(palette.locator(`[data-command="Go/pane ${beta.pane}"]`)).toHaveCount(1);
    await expect(palette.locator(`[data-command="Go/session ${site.id}"]`)).toHaveCount(0);
    await expect(palette.locator(`[data-command="Go/pane ${site.pane}"]`)).toHaveCount(0);
    await page.keyboard.press("Escape");
    await expect(palette).toBeHidden();

    // A teammate's view of the same machine: the same two.
    await asTeammate(page, [alpha.id, beta.id, site.id]);
    await expect.poll(() => listed(page)).toEqual([alpha.id, beta.id].sort());
    await page.screenshot({ path: info.outputPath("sidebar-teammate.png") });
  });
});

test.describe("on a phone", () => {
  const { viewport, userAgent, deviceScaleFactor, isMobile, hasTouch } = devices["Pixel 7"];
  test.use({ viewport, userAgent, deviceScaleFactor, isMobile, hasTouch });

  test("the sheet's sessions and Needs you are the shown project's (Arugula #1347)", async ({ page }, info) => {
    const { alpha, beta, site } = await twoProjects(page);
    await openApp(page, `/p/${P}/machines?pane=${beta.pane}`);
    await expect(page.locator(`[data-pane="${beta.pane}"]`)).toBeVisible();
    await page.locator(".sheet-button").click();
    const heads = page.locator(".sheet-sessions [data-session]");
    await expect.poll(() => heads.evaluateAll((els) => els.map((e) => Number(e.getAttribute("data-session"))).sort())).toEqual([alpha.id, beta.id].sort());
    // Needs you: alpha's ask, not site's.
    await expect(page.locator(`[data-wants="${alpha.pane}"]`)).toBeVisible();
    await expect(page.locator(`[data-wants="${site.pane}"]`)).toHaveCount(0);
    await page.screenshot({ path: info.outputPath("sheet.png") });
  });
});
