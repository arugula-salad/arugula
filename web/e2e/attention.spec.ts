// M24: attention reasons and actions in the client. A command that fails
// in a tab nobody is looking at says why on its tab and pane; dismissing
// it on one client clears it on the other; and its push notification
// offers Dismiss (delivered through the DevTools protocol, as S18 did).

import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { expect, test, type Page } from "@playwright/test";
import { open, reset, closeContexts, text } from "./helpers";
import type { PaneId } from "../src/proto";

const cli = resolve("../target/debug/arugula");
const fixtures = resolve("../crates/daemon/tests/fixtures");

test.afterAll(closeContexts);

/** A shell in a new tab that isn't shown (so nobody is looking at it). */
async function hiddenShell(page: Page): Promise<PaneId> {
  return page.evaluate(async () => {
    const res = await fetch("/api/run", { method: "POST", headers: { "Content-Type": "application/json" }, body: "{}" });
    return (await res.json()).pane as number;
  });
}

async function send(page: Page, pane: PaneId, text: string) {
  await page.evaluate(
    async ({ pane, text }) => {
      await fetch(`/api/panes/${pane}/send`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ text, enter: true }),
      });
    },
    { pane, text },
  );
}

const reasonOf = (page: Page, pane: PaneId) =>
  page.evaluate((p) => window.__arugula.client.info(p)?.reason ?? null, pane);

test("a failure says why, and dismissing it on one client clears it on the other", async ({ browser }) => {
  const a = await browser.newPage();
  const b = await browser.newPage();
  await reset(a);
  await open(b);
  const shown = await a.evaluate(() => window.__arugula.client.tab!);
  const pane = await hiddenShell(a);
  await expect.poll(() => a.evaluate((p) => !!window.__arugula.client.info(p)?.running, pane)).toBe(true);
  // a made it, so a shows it: back to the first tab.
  await a.evaluate((t) => window.__arugula.client.selectTab(t), shown);
  await a.waitForTimeout(500);
  await send(a, pane, "cargo() { sleep 3.2; echo 'test result: FAILED'; return 101; }");
  await send(a, pane, "cargo test");
  await expect.poll(() => reasonOf(a, pane), { timeout: 15_000 }).toMatchObject({ kind: "failed", exit: 101, command: "cargo test" });
  const r = (await reasonOf(b, pane))!;
  expect(r.headline).toMatch(/^cargo test failed \(exit 101\) after 3s/);
  expect(r.bundle).toBe("failed:here");

  // Its tab says so, with the headline as the title.
  const badge = a.locator(".tab .att.failed");
  await expect(badge).toHaveCount(1);
  await expect(badge).toHaveAttribute("title", r.headline);

  // Dismissed on b: gone on a.
  expect(await b.evaluate((p) => window.__arugula.client.act({ action: "dismiss", pane: p }), pane)).toBe(true);
  await expect.poll(() => reasonOf(a, pane)).toBeNull();
  await expect(a.locator(".tab .att")).toHaveCount(0);
  await a.close();
  await b.close();
});

test("a push for a failure offers Dismiss", async ({ browser }) => {
  const context = await browser.newContext();
  await context.grantPermissions(["notifications"]);
  const page = await context.newPage();
  await open(page);
  await expect.poll(() => page.evaluate(async () => !!(await navigator.serviceWorker.getRegistration()))).toBe(true);
  await page.evaluate(() => navigator.serviceWorker.ready);
  const cdp = await context.newCDPSession(page);
  await cdp.send("ServiceWorker.enable");
  const registrationId = await new Promise<string>((resolve) => {
    cdp.on("ServiceWorker.workerRegistrationUpdated", (e) => {
      const r = e.registrations.find((x) => !x.isDeleted);
      if (r) resolve(r.registrationId);
    });
  });
  // What the daemon sends for a failure (see crates/daemon/tests/integration/attention.rs).
  const payload = {
    title: "Failed",
    body: "cargo test failed (exit 101) after 3s",
    pane: 7,
    tag: "pane-7",
    reason: { kind: "failed", actions: ["rerun", "dismiss"], bundle: "failed:here" },
  };
  await cdp.send("ServiceWorker.deliverPushMessage", {
    origin: new URL(page.url()).origin,
    registrationId,
    data: JSON.stringify(payload),
  });
  const shown = () =>
    page.evaluate(async () => {
      const reg = await navigator.serviceWorker.getRegistration();
      const ns = (await reg?.getNotifications()) ?? [];
      return ns.map((n) => ({ title: n.title, body: n.body, actions: (n as unknown as { actions: { action: string }[] }).actions.map((x) => x.action) }));
    });
  // M11: Rerun from the notification, as from the phone's Needs you.
  await expect.poll(shown).toEqual([{ title: "Failed", body: payload.body, actions: ["rerun", "dismiss"] }]);
  await context.close();
});

// M111 (#1026): the attention drawer, in the new workspace. A real
// permission request (Claude Code's PermissionRequest hook, fed S18's
// recorded input, as perm-reach.spec.ts does) in a tab nobody is looking
// at: the bell counts it, the drawer shows it, closing the drawer and Go to
// pane leave it pending, and Allow in the drawer answers it.
test.describe("the attention drawer", () => {
  test.use({ viewport: { width: 1440, height: 900 } });
  const COMMAND = "cargo test -p arugula-control --test relay";
  let dir = "";
  test.beforeAll(() => {
    dir = mkdtempSync(join(tmpdir(), "arugula-e2e-m111-"));
    const read = (n: string) => JSON.parse(readFileSync(join(fixtures, n), "utf8"));
    const input = { command: COMMAND, description: "Run the relay tests" };
    writeFileSync(join(dir, "pre.json"), JSON.stringify({ ...read("s18-hook-pretooluse-bash.json"), tool_name: "Bash", tool_input: input, tool_use_id: "toolu_m111" }));
    writeFileSync(join(dir, "perm.json"), JSON.stringify({ ...read("s18-hook-permission.json"), tool_name: "Bash", tool_input: input }));
  });
  test.afterAll(() => rmSync(dir, { recursive: true, force: true }));

  test("a pending ask is in the drawer; Go to pane lands on it still pending; Allow answers it", async ({ page }) => {
    await reset(page);
    await expect(page.locator(".ws-rail")).toBeVisible();
    const shown = await page.evaluate(() => window.__arugula.client.tab!);
    const pane = await hiddenShell(page);
    await expect.poll(() => page.evaluate((p) => !!window.__arugula.client.info(p)?.running, pane)).toBe(true);
    await page.evaluate((t) => window.__arugula.client.selectTab(t), shown);
    await send(page, pane, `${cli} hook < ${dir}/pre.json; ${cli} hook < ${dir}/perm.json; echo hook-done-$((6*7))`);
    const ask = () => page.evaluate((p) => window.__arugula.client.info(p)?.ask?.kind ?? null, pane);
    await expect.poll(ask, { timeout: 15_000 }).toBe("permission");

    // The bell counts it.
    const bell = page.locator("[data-open-attention]");
    await expect(bell).toHaveAttribute("data-bell-count", "1");
    await bell.click();
    const drawer = page.getByRole("dialog", { name: "Needs you" });
    await expect(drawer).toBeVisible();
    const item = drawer.locator(`[data-att-pane="${pane}"]`);
    await expect(item).toContainText("Claude Code asks");
    await item.click();
    await expect(item).toHaveAttribute("aria-selected", "true");
    // What it would run, where, and who asks.
    const fact = (k: string) => drawer.locator(`[data-fact="${k}"] dd`);
    await expect(fact("command")).toHaveText(COMMAND);
    await expect(fact("agent")).toHaveText("Claude Code");
    const cwd = await page.evaluate((p) => window.__arugula.client.cwd(p), pane);
    if (cwd) await expect(fact("directory")).toHaveText(cwd);
    await expect(fact("machine")).not.toBeEmpty();

    // Escape closes it, gives the focus back to the bell, and answers nothing.
    await page.keyboard.press("Escape");
    await expect(drawer).toBeHidden();
    await expect(bell).toBeFocused();
    expect(await ask()).toBe("permission");

    // Go to pane: its tab and pane, with the ask still pending.
    await bell.click();
    await drawer.locator(`[data-att-pane="${pane}"]`).click();
    await drawer.locator("[data-attention-goto]").click();
    await expect(drawer).toBeHidden();
    await expect.poll(() => page.evaluate(() => window.__arugula.client.active())).toBe(pane);
    expect(await page.evaluate((p) => window.__arugula.client.tabOfPane(p)?.id === window.__arugula.client.tab, pane)).toBe(true);
    await page.waitForTimeout(500);
    expect(await ask()).toBe("permission");
    expect(await text(page, pane)).not.toContain("hook-done-42");

    // Allow, from the drawer.
    await page.evaluate((t) => window.__arugula.client.selectTab(t), shown);
    await bell.click();
    await drawer.locator(`[data-att-pane="${pane}"]`).click();
    await drawer.getByRole("button", { name: "Allow", exact: true }).click();
    await expect.poll(ask).toBeNull();
    await expect(drawer.locator(`[data-att-pane="${pane}"]`)).toHaveCount(0);
    await expect(bell).toHaveAttribute("data-bell-count", "0");
    await expect.poll(() => text(page, pane)).toContain("hook-done-42");
  });
});
