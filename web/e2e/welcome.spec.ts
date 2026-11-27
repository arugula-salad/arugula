// #110: Getting started, from Go to…'s Help and the phone's sheet: one
// screen since Lift 7 (#1487), Add a machine, with the join command to
// copy, and Claude Code's one click where it's here and not set up. It
// doesn't open by itself, not even on a first visit
// (https://app.arugula.io/issues/iss_fa949b979b607ef8f0414cbe674ab33b), and
// Tailscale is Settings' (a dialog of its own). #96: what a phone that
// can't be notified is told.

import { devices, expect, test } from "@playwright/test";
import { reset, closeContexts, sheetTab, paletteCommand, settingsRow } from "./helpers";

test.afterAll(closeContexts);

// A person's browser, not automation's: what a first visit would do, it
// does here (every other spec starts as a later one).
const person = () => {
  Object.defineProperty(Navigator.prototype, "webdriver", { get: () => false });
  Object.assign(window, { __arugulaFirstRun: true });
};

test("a first visit opens on the work, with no Getting started; the palette opens it, without Tailscale", async ({ browser }) => {
  const ctx = await browser.newContext();
  await ctx.addInitScript(person);
  const page = await ctx.newPage();
  await reset(page);
  await expect(page.locator(".session-button")).toBeVisible();
  const panel = page.getByRole("dialog", { name: "Add a machine" });
  // Nothing opens by itself: no Getting started, no What's new, no telemetry card.
  await page.waitForTimeout(1500);
  await expect(page.getByRole("dialog")).toHaveCount(0);
  await expect(page.locator("[data-whats-new], [data-telemetry-card], [data-agents-nudge]")).toHaveCount(0);

  // In Go to…'s Help: one screen, Add a machine, with the command to copy and no Phone.
  await paletteCommand(page, "Help/Getting started");
  await expect(panel).toBeVisible();
  await expect(panel.locator("[data-start-connect]")).toHaveText("Connect to Arugula cloud");
  await expect(panel.locator("[data-join-command]")).toHaveText("arugulad join https://control.arugula.io");
  await expect(panel.locator('[data-start-seg], [data-start-progress], [data-start-next], [data-start-serve]')).toHaveCount(0);
  await page.keyboard.press("Escape");
  await expect(panel).toBeHidden();

  // Not by itself on the next visit either.
  await page.reload();
  await expect.poll(() => page.evaluate(() => window.__arugula?.client.state !== null)).toBe(true);
  await expect(page.locator(".session-button")).toBeVisible();
  await page.waitForTimeout(1000);
  await expect(panel).toBeHidden();

  await ctx.close();
});

test("Tailscale is in Settings, a dialog of its own", async ({ page }) => {
  await reset(page);
  await settingsRow(page, "Tailscale…");
  const dialog = page.getByRole("dialog", { name: "Tailscale" });
  await expect(dialog.getByRole("heading", { name: /from your phone$/ })).toBeVisible();
  // What the daemon says about Tailscale: the test machine may or may not
  // have it, but the button is there until it serves this daemon.
  await expect(dialog.locator("[data-start-serve]")).toBeVisible();
  await dialog.getByRole("button", { name: "Done" }).click();
  await expect(dialog).toBeHidden();
  // The desktop app's menu, opening Getting started at its old Phone step, gets it too.
  await page.evaluate(() => dispatchEvent(new CustomEvent("arugula:getting-started", { detail: "phone" })));
  await expect(dialog).toBeVisible();
  await expect(page.getByRole("dialog", { name: "Add a machine" })).toHaveCount(0);
});

test("the desktop app up to 0.26.3 opens it from its Daemon menu", async ({ page }) => {
  // #708: that app names the event by the page's `__arugulaPage`, exactly so.
  await reset(page);
  const panel = page.getByRole("dialog", { name: "Add a machine" });
  await expect(panel).toBeHidden();
  await page.evaluate(() =>
    dispatchEvent(
      new CustomEvent(((window as { __arugulaPage?: boolean }).__arugulaPage ? "arugula" : "illogical") + ":getting-started", {
        detail: "cloud",
      }),
    ),
  );
  await expect(panel).toBeVisible();
  await expect(panel.locator("[data-start-connect]")).toBeVisible();
});

// #1003: joining Arugula cloud is one click. Connect shows the link that
// opens control with the code, where a signed-in person clicks Add, and the
// step says joined as soon as the daemon does: no fingerprint to compare.
// The daemon's answers are stood in for here (control isn't); the daemon's
// own join is crates/arugulad/tests/integration/join_tokens.rs.
test("the cloud step is one click in Arugula cloud, then joined", async ({ page }) => {
  await reset(page);
  const url = "https://control.arugula.io";
  const code = "ABCDE-FGHJK.MNPQ-RSTV-WXYZ-0123";
  const pending = { code, approve: `${url}/#join=${code}`, expires_ms: Date.now() + 15 * 60_000 };
  let state: "none" | "pending" | "joined" = "none";
  let asked: Record<string, unknown> | null = null;
  const control = () => ({ url, ...(state === "pending" ? { pending } : state === "joined" ? { joined: url } : {}) });
  await page.route(/\/api\/setup(\/control)?(\?.*)?$/, async (r) => {
    const u = new URL(r.request().url());
    if (u.pathname === "/api/setup/control") {
      asked = r.request().postDataJSON() as Record<string, unknown>;
      state = "pending";
      return r.fulfill({ json: { pending } });
    }
    if (u.searchParams.get("part") === "control") return r.fulfill({ json: { control: control() } });
    // The rest of the status is the daemon's own.
    const real = (await (await r.fetch()).json()) as Record<string, unknown>;
    return r.fulfill({ json: { ...real, control: control() } });
  });
  await page.evaluate(() => dispatchEvent(new CustomEvent("arugula:getting-started", { detail: "cloud" })));
  const panel = page.getByRole("dialog", { name: "Add a machine" });
  await expect(panel.locator("[data-start-join-how]")).toContainText("Click Add there");
  // Nothing asked about typing before sign-in: the join settles it, on for
  // your account (from your phone too) and off for a team (Arugula issue
  // iss_d8ce0c357cf0902c51dec39378ef7a1b).
  await expect(panel.locator("[data-start-remote-input]")).toContainText("type into");
  await expect(panel.locator("[data-start-remote-input]")).toContainText("from your phone");
  await expect(panel.locator("input[type=radio]")).toHaveCount(0);
  await panel.locator("[data-start-connect]").click();
  await expect.poll(() => asked).toEqual({ url });

  const add = panel.locator("[data-start-approve]");
  await expect(add).toHaveText("Add it in Arugula cloud");
  await expect(add).toHaveAttribute("href", pending.approve);
  await expect(panel.locator("[data-start-pending]")).toContainText("Waiting for you to click Add there");
  // The code, for a device signed in elsewhere; nothing to compare.
  await expect(panel.locator("[data-start-code]")).toHaveText("ABCDE-FGHJK.MNPQ-RSTV-WXYZ-0123");
  await expect(panel.locator("[data-start-confirm], [data-start-one-approval], .start-fp")).toHaveCount(0);
  await expect(panel.locator("[data-start-pending]")).not.toContainText(/approv|fingerprint/i);

  // Added there: the poll finds it joined.
  state = "joined";
  await expect(panel.locator("[data-start-joined]")).toHaveText("Joined to your account", { timeout: 10_000 });
  await expect(panel.locator("[data-start-next-device]")).not.toContainText(/approv/i);
});

test("the daemon's setup status, and the cheap poll for a join", async ({ page }) => {
  await reset(page);
  const status = await page.evaluate(async () => (await fetch("/api/setup")).json());
  expect(["missing", "stopped", "needs-login", "running"]).toContain(status.tailscale.state);
  expect(status.control.url).toBe("https://control.arugula.io");
  expect(typeof status.claude.installed).toBe("boolean");
  // Polling while a join waits asks for the cheap part only.
  const control = await page.evaluate(async () => (await fetch("/api/setup?part=control")).json());
  expect(Object.keys(control)).toEqual(["control"]);
});

test.describe("phone", () => {
  const { viewport, deviceScaleFactor, isMobile, hasTouch } = devices["Pixel 7"];
  test.use({ viewport, deviceScaleFactor, isMobile, hasTouch });

  test("in the sheet", async ({ page }) => {
    await reset(page);
    await expect(page.getByRole("dialog", { name: "Add a machine" })).toBeHidden();
    await page.locator(".sheet-button").click();
    await sheetTab(page, "More");
    await page.getByRole("button", { name: "Getting started" }).click();
    await expect(page.getByRole("dialog", { name: "Add a machine" })).toBeVisible();
  });

  test("iOS in a Safari tab: add it to the Home Screen, once", async ({ browser }) => {
    const ctx = await browser.newContext({ ...devices["iPhone 13"], hasTouch: true });
    // Safari in a tab has no push at all.
    await ctx.addInitScript(() => {
      delete (window as { PushManager?: unknown }).PushManager;
    });
    const page = await ctx.newPage();
    await reset(page);
    const hint = page.locator("[data-install-hint]");
    await expect(hint).toContainText("Add Arugula to your Home Screen");
    await page.locator(".sheet-button").click();
    await sheetTab(page, "More");
    await expect(page.locator("[data-notify-blocked]")).toHaveText(/^Add Arugula to your Home Screen to get notifications \(Share › Add to Home Screen\), then open it from there\.$/);
    await expect(page.getByRole("button", { name: "Notify this device" })).toHaveCount(0);
    await page.locator(".sheet-backdrop").click({ position: { x: 5, y: 600 } });
    await hint.getByRole("button", { name: "Dismiss" }).click();
    await expect(hint).toBeHidden();
    await page.reload();
    await expect(page.locator(".sheet-button")).toBeVisible();
    await expect(hint).toBeHidden();
    await ctx.close();
  });

  test("blocked: says where to unblock it", async ({ page }) => {
    await page.addInitScript(() => Object.defineProperty(Notification, "permission", { get: () => "denied" }));
    await reset(page);
    await page.locator(".sheet-button").click();
    await sheetTab(page, "More");
    await expect(page.locator("[data-notify-blocked]")).toHaveText("Notifications are blocked for this site in your browser's settings.");
  });
});

// Claude Code's one click (#335): offered on the screen where Claude Code is
// here and not set up; opened at `agents` (the hooks' nudge, the hint's
// Set it up), whatever its state. The daemon's answers are stood in for.
test("Claude Code's one click: on the screen when it's here and not set up, and at agents whatever its state", async ({ page }) => {
  await reset(page);
  let ready = false;
  let posted = 0;
  const claude = () => ({ installed: true, tools: ready, hooks: ready });
  await page.route(/\/api\/setup(\/agents\/claude)?(\?.*)?$/, async (r) => {
    const u = new URL(r.request().url());
    if (u.pathname === "/api/setup/agents/claude") {
      posted++;
      ready = true;
      return r.fulfill({ json: { ok: true, done: ["Claude Code has Arugula's MCP server and hooks."] } });
    }
    if (u.searchParams.get("part") === "agents") return r.fulfill({ json: { claude: claude(), adapters: [] } });
    // The rest is the daemon's own; a read still out when the test ends is dropped.
    const real = await r
      .fetch()
      .then((x) => x.json() as Promise<Record<string, unknown>>)
      .catch(() => null);
    if (real) await r.fulfill({ json: { ...real, claude: claude(), adapters: [] } }).catch(() => {});
  });
  const panel = page.getByRole("dialog", { name: "Add a machine" });
  await page.evaluate(() => dispatchEvent(new CustomEvent("arugula:getting-started", { detail: "cloud" })));
  const offer = panel.locator("[data-start-offer]");
  await expect(offer).toContainText("One click runs it here as agent panes");
  await offer.locator("[data-start-claude]").click();
  await expect.poll(() => posted).toBe(1);
  await expect(offer.locator("[data-start-done]")).toContainText("MCP server and hooks");
  await page.keyboard.press("Escape");
  // Set up: not offered by itself, but at `agents` it says so.
  await page.evaluate(() => dispatchEvent(new CustomEvent("arugula:getting-started", { detail: "cloud" })));
  await expect(panel.locator("[data-start-connect]")).toBeVisible();
  await expect(panel.locator("[data-start-offer]")).toHaveCount(0);
  await page.keyboard.press("Escape");
  await page.evaluate(() => dispatchEvent(new CustomEvent("arugula:getting-started", { detail: "agents" })));
  await expect(panel.locator("[data-start-claude-ready]")).toHaveText("Claude Code is set up");
});
