// The Overview's Needs you (Lift 4, #1480), this project's slice of the
// Inbox, as kit NeedRows in the right third: a row expands in place and
// answers there (a pane's permission on its machine, an approval and a
// to-do through the Inbox), then stays as a receipt; a teammate's asks fold
// into "N asks for Jo"; To do follows the asks, its how in a well and a note;
// alerts aren't here; the keys j k e y x D n ↵ move, expand, answer and
// open; moving to a pane's ask peeks its session in the panel.
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { devices, expect, test, type Page } from "@playwright/test";
import { cli, overview, P, paneItem, say } from "./overview-fixture";

test.use({ permissions: ["local-network-access"] });

const fixtures = fileURLToPath(new URL("../../crates/daemon/tests/fixtures/", import.meta.url));
const needs = (page: Page) => page.locator("[data-ov-needs]");
const row = (page: Page, id: string) => page.locator(`[data-need-item="${id}"]`);
const posted = (page: Page) => page.evaluate(() => (window as unknown as { __inboxPosted?: unknown[] }).__inboxPosted ?? []);

/** An Inbox item of the project's, the reader's unless `who` (an account) says whose. */
const item = (id: string, kind: string, title: string, more: Record<string, unknown> = {}) => ({
  id,
  kind,
  group: "decide",
  counts: !more.for || more.for === "acct_a",
  title,
  from: { name: "Chief of staff · Arugula" },
  subject: { type: "proposal", id, url: `/p/${P}/chat`, project: P },
  since: Date.now() - 7 * 60_000,
  ...more,
});
const APPROVAL = item("prp_start", "approval", "Start a worker on #12 on geek?", { lines: ["Repo: acme/app", "Why: the release waits on it."], answer: { via: "card", options: ["Start", "Not now"] } });
const TODO = item("prp_todo", "todo", "Swap the filament spool, then Done", { lines: ["Why: the printer stopped.", "1. Open the lid\n2. Swap the spool"], answer: { via: "todo", options: ["Done", "Can't"] }, since: Date.now() - 60_000 });
const ALERT = item("run_1:run_failed", "run_failed", "CI failed on main", { subject: { type: "run", id: "run_1", url: "/", project: P } });

/** A Claude Code permission in the suite daemon's terminal (its hook), and the Inbox's item for it. */
async function permission(page: Page, term: number): Promise<string> {
  const dir = mkdtempSync(join(tmpdir(), "arugula-e2e-1480-"));
  const read = (n: string) => JSON.parse(readFileSync(join(fixtures, n), "utf8"));
  writeFileSync(join(dir, "pre.json"), JSON.stringify({ ...read("s18-hook-pretooluse-bash.json"), tool_name: "Bash", tool_input: { command: "npm test" }, tool_use_id: "toolu_1480" }));
  writeFileSync(join(dir, "perm.json"), JSON.stringify({ ...read("s18-hook-permission.json"), tool_name: "Bash", tool_input: { command: "npm test" } }));
  await page.evaluate(
    ({ term, line }) => fetch(`/api/panes/${term}/send`, { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ text: line, enter: true }) }),
    { term, line: `${cli} hook < ${dir}/pre.json; ${cli} hook < ${dir}/perm.json; echo hook-done-$((6*7))` },
  );
  // Its ask reaches the fleet before the Inbox lists it.
  await expect
    .poll(() => page.evaluate((t) => !!(window as unknown as { __app: { fleet: { panes: { id: number; info: { ask?: unknown } }[] } } }).__app.fleet.panes.find((p) => p.id === t)?.info.ask, term), { timeout: 15_000 })
    .toBe(true);
  return dir;
}

test.describe("on a computer", () => {
  test.use({ viewport: { width: 1440, height: 900 } });

  test("a pane's permission expands in place and is allowed in the row, on its machine; it stays as a receipt", async ({ page }, info) => {
    const { term } = await overview(page);
    const dir = await permission(page, term);
    try {
      const host = await page.evaluate((t) => (window as unknown as { __app: { fleet: { panes: { id: number; host: string }[] } } }).__app.fleet.panes.find((p) => p.id === t)!.host, term);
      const it = { ...paneItem(host, term), kind: "permission", title: "Allow Bash: npm test" };
      await say(page, [it]);
      const r = row(page, it.id);
      await expect(r).toBeVisible();
      await expect(r).toHaveAttribute("data-need", "permission");
      await expect(r.locator("button[aria-expanded]").first()).toHaveAttribute("aria-expanded", "false");
      await expect(needs(page).locator("[data-ov-needs-count]")).toHaveText("1");
      // It says whom it asks, in the needs colour, and where it is: no amber fill.
      await expect(r).toContainText("asks you");
      // Expanded: the command in a well, Allow and Deny with their keys.
      await r.locator("button[aria-expanded]").first().click();
      await expect(r.locator("[data-kit=well] pre")).toContainText("npm test");
      const allow = r.getByRole("button", { name: /^Allow/ });
      await expect(allow).toHaveAttribute("aria-keyshortcuts", "y");
      await page.screenshot({ path: info.outputPath("overview-needs-permission.png") });
      await allow.click();
      // Answered on its machine: the hook returned and the shell went on.
      await expect.poll(() => page.evaluate(async (p) => (await fetch(`/api/panes/${p}/capture?format=text`)).text(), term), { timeout: 15_000 }).toContain("hook-done-42");
      // A receipt where it was, though the Inbox drops it.
      await say(page, []);
      await expect(r).toContainText(/Allow by you/);
      await expect(r.getByRole("button", { name: /^Allow/ })).toHaveCount(0);
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  });

  test("an approval answers through the Inbox with its option; a teammate's asks fold under one line and are neutral; alerts aren't here", async ({ page }) => {
    await overview(page);
    await say(page, [APPROVAL, item("prp_jo1", "approval", "Close the stale sessions?", { for: "acct_b" }), item("prp_jo2", "approval", "Start a lead on Later?", { for: "acct_b" }), ALERT]);
    const r = row(page, APPROVAL.id);
    await expect(r).toHaveAttribute("data-need", "approval");
    await expect(needs(page).locator("[data-ov-needs-count]")).toHaveText("1");
    await expect(r.locator("button[aria-expanded]").first()).toContainText("chief of staff");
    // The alert is the board's and Running's, not here.
    await expect(row(page, ALERT.id)).toHaveCount(0);
    // Jo's two, folded into one line until it's opened; theirs say "asks Jo" and have no answers.
    const fold = page.locator('[data-ov-asks-for="acct_b"]');
    await expect(fold.locator("button[aria-expanded]").first().first()).toHaveText(/2 asks for Jo/);
    await expect(row(page, "prp_jo1")).toHaveCount(0);
    await fold.locator("button[aria-expanded]").first().first().click();
    await expect(row(page, "prp_jo1")).toBeVisible();
    await row(page, "prp_jo1").locator("button[aria-expanded]").first().click();
    await expect(row(page, "prp_jo1")).toContainText("asks Jo");
    await expect(row(page, "prp_jo1").locator("[data-answer]")).toHaveCount(0);
    // Mine: its lines, then Start (y) and Not now (x); Start goes to the Inbox as its choice.
    await r.locator("button[aria-expanded]").first().click();
    await expect(r).toContainText("Why: the release waits on it.");
    await r.getByRole("button", { name: /^Start/ }).click();
    await expect.poll(() => posted(page)).toEqual([{ id: APPROVAL.id, choice: "Start" }]);
    await expect(r).toContainText(/Start by you/);
  });

  test("To do follows the asks: its why, its how in a well to copy, a note with no secrets, and Done (D) sends the note", async ({ page }) => {
    await overview(page);
    await say(page, [APPROVAL, TODO]);
    await expect(page.locator("[data-ov-todo]")).toContainText("To do");
    const r = row(page, TODO.id);
    await expect(r).toHaveAttribute("data-need", "todo");
    await expect(r.locator("button[aria-expanded]").first()).toContainText("waits on you");
    await r.locator("button[aria-expanded]").first().click();
    await expect(r).toContainText("Why: the printer stopped.");
    await expect(r.locator("[data-kit=well] pre")).toHaveText("1. Open the lid\n2. Swap the spool");
    await expect(r.getByRole("textbox", { name: /Note/ })).toBeVisible();
    await expect(r).toContainText("no secrets");
    await r.getByRole("textbox", { name: /Note/ }).fill("swapped to PETG");
    // D from the page (not the field) presses Done with the note.
    await r.locator("button[aria-expanded]").first().focus();
    await page.keyboard.press("Shift+D");
    await expect.poll(() => posted(page)).toEqual([{ id: TODO.id, choice: "Done", note: "swapped to PETG" }]);
    await expect(r).toContainText(/Done by you .* · swapped to PETG/);
  });

  test("the keys: j and k move and open, e folds, y answers, n goes to the next thing through To do, Enter opens it where it is", async ({ page }) => {
    await overview(page);
    const second = item("prp_close", "approval", "Close two sessions?", { answer: { via: "card", options: ["Approve", "Deny"] }, since: Date.now() - 3 * 60_000 });
    await say(page, [APPROVAL, second, TODO]);
    await expect(row(page, APPROVAL.id)).toBeVisible();
    await page.locator("body").click({ position: { x: 5, y: 5 } });
    // j: the first row, open and selected.
    await page.keyboard.press("j");
    await expect(row(page, APPROVAL.id)).toHaveAttribute("data-selected", "true");
    await expect(row(page, APPROVAL.id).locator("button[aria-expanded]").first()).toHaveAttribute("aria-expanded", "true");
    // e folds it; j moves on; k back.
    await page.keyboard.press("e");
    await expect(row(page, APPROVAL.id).locator("button[aria-expanded]").first()).toHaveAttribute("aria-expanded", "false");
    await page.keyboard.press("j");
    await expect(row(page, second.id)).toHaveAttribute("data-selected", "true");
    await page.keyboard.press("k");
    await expect(row(page, APPROVAL.id)).toHaveAttribute("data-selected", "true");
    // n: the next thing that needs you, then To do.
    await page.keyboard.press("n");
    await expect(row(page, second.id)).toHaveAttribute("data-selected", "true");
    await page.keyboard.press("n");
    await expect(row(page, TODO.id)).toHaveAttribute("data-selected", "true");
    // y answers the open row the first way.
    await page.keyboard.press("k");
    await page.keyboard.press("y");
    await expect.poll(() => posted(page)).toEqual([{ id: second.id, choice: "Approve" }]);
    // Enter opens the selected one where it lives: its chat.
    await page.locator("body").click({ position: { x: 5, y: 5 } });
    await page.keyboard.press("j");
    await page.keyboard.press("Enter");
    await expect(page).toHaveURL(new RegExp(`/p/${P}[^/]*/chat`));
  });

  test("the keys are in ? under Needs you, and their chips in the footer", async ({ page }) => {
    await overview(page);
    const ids = await page.evaluate(() => (window as unknown as { __arugulaKeys: () => string[] }).__arugulaKeys());
    for (const id of ["needs-down", "needs-up", "needs-expand", "needs-yes", "needs-no", "needs-done", "needs-next", "needs-open"]) expect(ids).toContain(id);
  });

  test("moving to a pane's ask peeks its session in the panel", async ({ page }) => {
    const { term, agent } = await overview(page);
    const host = await page.evaluate((t) => (window as unknown as { __app: { fleet: { panes: { id: number; host: string }[] } } }).__app.fleet.panes.find((p) => p.id === t)!.host, term);
    // The agent's block on #12 asks (as the Inbox says).
    await say(page, [{ ...paneItem(host, agent), title: "Which colour?" }]);
    await row(page, `${host}.${agent}:1`).locator("button[aria-expanded]").first().click();
    await expect(page.locator('[data-ov-panel] [data-ov-tab^="peek:"] [role="tab"]')).toHaveAttribute("aria-selected", "true");
    await expect(page.locator("[data-ov-panel] [data-ov-peek]")).toBeVisible();
  });
});

test.describe("on a phone", () => {
  const { viewport, userAgent, deviceScaleFactor, isMobile, hasTouch } = devices["Pixel 7"];
  test.use({ viewport, userAgent, deviceScaleFactor, isMobile, hasTouch });

  test("a row fits the column, expanded, its answers in reach", async ({ page }, info) => {
    await overview(page);
    await say(page, [{ ...APPROVAL, title: "Start a worker on #12 on geek, with a title that goes on and on and on and on and on?" }]);
    const r = row(page, APPROVAL.id);
    await r.locator("button[aria-expanded]").first().click();
    await expect(r.getByRole("button", { name: /^Start/ })).toBeVisible();
    expect(await needs(page).evaluate((e) => e.scrollWidth <= e.clientWidth)).toBe(true);
    await page.screenshot({ path: info.outputPath("overview-needs-phone.png") });
  });
});
