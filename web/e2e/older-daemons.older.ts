// The page at HEAD against released daemons (Arugula #808), run by `just
// older-daemons`: one project per daemon (playwright.older.config.ts), the
// page's files from web/dist (older-daemon.ts). Control serves its newest
// page to every machine, and a machine runs whatever it last updated to:
// 0.66.12's Docs page read a field 0.66.10's rows didn't have, threw while
// drawing, and sat on "Reading this machine's Docs…" for every machine not
// yet updated. So each page that lists a daemon's rows is opened here and
// must show them: its content, not its loading state, nor the note that it
// failed to draw (shared/ui/guard.tsx), and nothing thrown along the way.

import { readFileSync } from "node:fs";
import { expect, type Locator, type Page } from "@playwright/test";
import { api, test } from "./older-daemon";

const DIST_INDEX = new URL("../dist/index.html", import.meta.url);
const MARK = "OLDER-DAEMON-7731";

/** Whether this daemon has the API a page reads (0.42 has no Docs). */
async function has(url: string, path: string): Promise<boolean> {
  return (await api(url, path)).status !== 404;
}

/** The page has drawn `content`, and nothing on it is loading, failed or thrown. */
async function shows(page: Page, errors: string[], content: Locator) {
  const failed = page.locator("[data-page-failed]");
  // Whichever comes first: a page that failed says so at once.
  await expect(content.first().or(failed).first(), "the page's content").toBeVisible();
  await expect(failed, `the page failed to draw: ${errors.join("; ")}`).toHaveCount(0);
  await expect(content.first(), "the page's content").toBeVisible();
  await expect(page.getByRole("status").filter({ hasText: /^Loading .*…$/ }), "still loading").toHaveCount(0);
  expect(errors, "thrown in the page").toEqual([]);
}

/** Open the page on the panes, connected, with the machine's state. */
async function open(page: Page, hash = "") {
  await page.goto(`/${hash}`);
  await expect.poll(() => page.evaluate(() => window.__arugula?.client.connected)).toBe(true);
  await expect.poll(() => page.evaluate(() => window.__arugula.client.state !== null)).toBe(true);
}

test("the page is this checkout's, not the daemon's own", async ({ page, daemon }) => {
  // Vite's entry, by its hash: what web/dist has, not what the daemon embeds.
  const entry = /<script type="module"[^>]* src="([^"]+)"/.exec(readFileSync(DIST_INDEX, "utf8"))![1];
  await page.goto("/");
  await expect(page.locator(`script[type="module"][src="${entry}"]`)).toHaveCount(1);
  expect(((await (await api(daemon.url, "/api/host")).json()) as { version: string }).version).toBe(daemon.version);
});

test("the panes: a terminal that runs a command", async ({ page, errors }) => {
  await open(page);
  const pane = await page.evaluate(() => window.__arugula.client.active());
  expect(pane, "a pane to type in").not.toBeNull();
  await page.locator(`[data-pane="${pane}"]`).click({ position: { x: 40, y: 40 } });
  await page.keyboard.type(`echo ${MARK}-$((6*7))\n`, { delay: 2 });
  await expect.poll(() => page.evaluate((p) => window.__arugula.text(p), pane!)).toContain(`${MARK}-42`);
  await shows(page, errors, page.locator(`[data-pane="${pane}"]`));
});

/** The machine's Settings dialogs that list the daemon's rows: what each
 * shows once read. A daemon older than the dialog may say so instead
 * ("too old", "older than this page"), which is the page coping. */
const DIALOGS: [string, string][] = [
  ["Access…", "[data-access-kind]"],
  ["Permission rules…", "[data-rule], p.dim:has-text(\"No standing tool rules\")"],
  ["Usage and crash reports…", "[data-machine-telemetry-switch]"],
  ["Guest SSH access…", "[data-sharing-guest-ssh]"],
  ["Developer settings…", "[data-flag]"],
];
const OLDER = /too old|older than/;

test("Settings: every section, and the machine's dialogs read their rows", async ({ page, errors }) => {
  await open(page, "#settings");
  const settings = page.locator("[data-settings-page]");
  for (const id of ["device", "preferences", "machine", "help"]) await expect(settings.locator(`[data-settings-section="${id}"]`)).toBeVisible();
  await shows(page, errors, settings.locator('[data-settings-section="machine"] [data-setting]'));
  const machine = settings.locator('[data-settings-section="machine"]');
  const opened: string[] = [];
  for (const [row, content] of DIALOGS) {
    const button = machine.getByRole("button", { name: row, exact: true });
    if (!(await button.count())) continue;
    opened.push(row);
    await button.click();
    const dialog = page.getByRole("dialog", { name: row.replace(/…$/, "") });
    await expect(dialog.locator(content).or(dialog.getByText(OLDER)).first(), `${row}: its rows`).toBeVisible();
    await expect(dialog.getByText(/^Loading…$/), `${row}: still loading`).toHaveCount(0);
    await page.keyboard.press("Escape");
    await expect(dialog).toHaveCount(0);
  }
  // Every one is on a daemon's own page whatever its version: the page decides.
  expect(opened).toEqual(DIALOGS.map(([row]) => row));
  expect(errors, "thrown in the page").toEqual([]);
});
