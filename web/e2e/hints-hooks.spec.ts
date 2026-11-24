// The hooks offer (https://app.arugula.io/issues/iss_fa949b979b607ef8f0414cbe674ab33b,
// the onboarding prototype's 4a): Claude Code waits at a permission prompt
// in a terminal none of Arugula's hooks has spoken from, and Claude Code's
// settings lack them: a chip in that pane's header, "Answer these from
// anywhere". Turn on runs `POST /api/setup/hooks` (the hooks alone) and
// the status pill names the file it changed. Answered, Turn on or ×, it
// doesn't come back. Never on Codex: the daemon's `unhooked` is Claude
// Code's alone. Then the long pills, a phone's 360px wide: they wrap.
//
// Claude Code here is `sleep` under the name `claude` (the daemon knows a
// pane's program by its name), its prompt the pane's attention set through
// the API, and the daemon's setup answers are stood in for: nothing writes
// to this machine's ~/.claude. Hints keep quiet under a test runner unless
// a spec asks (hints/index.ts): this one does, `__arugulaHints`.

import { mkdtempSync, rmSync, symlinkSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { expect, test, type Page } from "@playwright/test";
import { closeContexts, panes, reset, type } from "./helpers";
import type { PaneId } from "../src/shared/proto";

test.afterAll(closeContexts);

/** `claude` and `codex`, each `sleep` by another name. */
let bin = "";
test.beforeAll(() => {
  bin = mkdtempSync(join(tmpdir(), "arugula-e2e-hints-hooks-"));
  for (const name of ["claude", "codex"]) symlinkSync("/usr/bin/sleep", join(bin, name));
});
test.afterAll(() => rmSync(bin, { recursive: true, force: true }));

const SAID = "Added Arugula's hooks to ~/.claude/settings.json: a terminal Claude's questions and permission prompts come to cards here and on your phone, from its next start.";

/** Hints on (the port's and the mark's seen already), and the daemon's
 * setup as a machine whose Claude Code lacks the hooks. Counts the clicks'
 * requests. */
async function setUp(page: Page) {
  await page.addInitScript(() => {
    Object.assign(window, { __arugulaHints: true });
    if (!localStorage.getItem("arugula.seen")) localStorage.setItem("arugula.seen", JSON.stringify({ "seen:command-mark": 1, "seen:port-listening": 1 }));
  });
  await page.route(/\/api\/setup\?part=agents$/, (r) => r.fulfill({ json: { claude: { installed: true, tools: true, hooks: false }, adapters: [] } }));
  const posted: string[] = [];
  await page.route(/\/api\/setup\/(hooks|agents\/claude|claude)$/, (r) => {
    posted.push(new URL(r.request().url()).pathname);
    return r.fulfill({ json: { ok: true, done: [SAID] } });
  });
  return posted;
}

/** `name` started in `pane`, known to the daemon by that name. */
async function start(page: Page, pane: PaneId, name: string) {
  await type(page, pane, `${join(bin, name)} 600\n`);
  await expect.poll(() => page.evaluate((p) => window.__arugula.client.info(p)?.command ?? "", pane)).toContain(name);
}

/** It waits for an answer (a permission prompt), as the daemon says. */
async function waits(page: Page, pane: PaneId) {
  await page.evaluate(
    (p) => fetch(`/api/panes/${p}/attention`, { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({ state: "needs_input" }) }),
    pane,
  );
  await expect.poll(() => page.evaluate((p) => window.__arugula.client.info(p)?.attention, pane)).toBe("needs_input");
}

/** No hint comes while someone types in a pane: the daemon's `typing` holds
 * 5 s after the keys that started it. */
const AFTER_TYPING = { timeout: 10_000 };

const chipOf = (page: Page, pane: PaneId) => page.locator(`[data-pane-header="${pane}"] [data-hint="claude-hooks"]`);
const seen = (page: Page) => page.evaluate(() => "seen:claude-hooks" in JSON.parse(localStorage.getItem("arugula.seen") ?? "{}"));

test("Claude Code waiting without hooks: Turn on adds them and says where, once", async ({ page }) => {
  const posted = await setUp(page);
  await reset(page);
  const [pane] = await panes(page);
  await start(page, pane, "claude");
  await expect.poll(() => page.evaluate((p) => window.__arugula.client.info(p)?.unhooked, pane)).toBe(true);
  // Running, not waiting: nothing yet.
  await page.waitForTimeout(500);
  await expect(chipOf(page, pane)).toHaveCount(0);

  await waits(page, pane);
  const chip = chipOf(page, pane);
  await expect(chip).toBeVisible(AFTER_TYPING);
  await expect(chip.locator(".hint-copy")).toHaveText("Answer these from anywhere");
  await chip.getByRole("button", { name: "Turn on" }).click();
  await expect(chip).toHaveCount(0);
  // The hooks alone: not the adapter, not the MCP server.
  await expect.poll(() => posted).toEqual(["/api/setup/hooks"]);
  await expect(page.locator("#status")).toContainText("~/.claude/settings.json");
  expect(await seen(page)).toBe(true);

  // Answered: not again, at the next prompt or after a reload.
  await reset(page);
  const [again] = await panes(page);
  await start(page, again, "claude");
  await waits(page, again);
  await page.waitForTimeout(1000);
  await expect(chipOf(page, again)).toHaveCount(0);
});

test("never on Codex; × puts it away for good", async ({ page }) => {
  await setUp(page);
  await reset(page);
  const [pane] = await panes(page);
  await start(page, pane, "codex");
  await waits(page, pane);
  await page.waitForTimeout(1000);
  await expect(chipOf(page, pane)).toHaveCount(0);
  expect(await page.evaluate((p) => window.__arugula.client.info(p)?.unhooked ?? false, pane)).toBe(false);

  await reset(page);
  const [claude] = await panes(page);
  await start(page, claude, "claude");
  await waits(page, claude);
  await expect(chipOf(page, claude)).toBeVisible(AFTER_TYPING);
  await chipOf(page, claude).getByRole("button", { name: "Dismiss this tip" }).click();
  await expect(chipOf(page, claude)).toHaveCount(0);
  expect(await seen(page)).toBe(true);

  await reset(page);
  const [next] = await panes(page);
  await start(page, next, "claude");
  await waits(page, next);
  await page.waitForTimeout(1000);
  await expect(chipOf(page, next)).toHaveCount(0);
});

test.describe("a phone's 360px", () => {
  test.use({ viewport: { width: 360, height: 740 }, isMobile: true, hasTouch: true });

  test("long pills wrap inside the window, their buttons whole", async ({ page }) => {
    const copy = "Get told on this device when a pane, an agent or a teammate needs you, wherever you are";
    await page.addInitScript((copy) => {
      Object.assign(window, {
        __arugulaHints: true,
        __arugulaTestHints: [{ id: "e2e-long-toast", place: "toast", fact: () => ({ args: {} }), copy: () => copy, action: { label: "Notify this device", route: "#" } }],
      });
    }, copy);
    await reset(page);

    // A hint's toast: as wide as the window lets it, not half of it.
    const toast = page.locator('[data-hint="e2e-long-toast"]');
    await expect(toast).toBeVisible();
    const box = (await toast.boundingBox())!;
    expect(box.x).toBeGreaterThanOrEqual(0);
    expect(box.x + box.width).toBeLessThanOrEqual(360);
    expect(box.width).toBeGreaterThan(300);
    // Its words take more than one line; its button keeps its own.
    const words = (await toast.locator(".hint-copy").boundingBox())!;
    expect(words.height).toBeGreaterThan(24);
    const act = toast.locator("[data-hint-act]");
    expect(await act.evaluate((b) => b.scrollWidth <= b.clientWidth)).toBe(true);
    expect((await act.boundingBox())!.height).toBeLessThan(24);
    await toast.getByRole("button", { name: "Dismiss this tip" }).click();

    // The status pill, saying what a click changed.
    await page.evaluate((said) => window.__arugula.client.toast(said), SAID);
    const pill = page.locator("#status");
    await expect(pill).toContainText("~/.claude/settings.json");
    const p = (await pill.boundingBox())!;
    expect(p.x).toBeGreaterThanOrEqual(0);
    expect(p.x + p.width).toBeLessThanOrEqual(360);
    expect(p.height).toBeGreaterThan(30);
  });
});
