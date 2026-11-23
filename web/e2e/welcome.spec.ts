// #110: Getting started opens once per browser (on a daemon's own page; on
// control's, once per account: getting-started-control.spec.ts), then from
// the account menu and the phone's sheet. Its last step, Things to try, is a
// button for each thing, marked done once used. #96: what a phone that
// can't be notified is told.

import { devices, expect, test } from "@playwright/test";
import { reset, closeContexts, sheetTab, paletteCommand } from "./helpers";

test.afterAll(closeContexts);

// The app doesn't greet automation (every other spec wants a clean first
// screen); these pretend to be a person.
const person = () => Object.defineProperty(Navigator.prototype, "webdriver", { get: () => false });

test("opens once on first run, a step at a time, then from the account menu", async ({ browser }) => {
  const ctx = await browser.newContext();
  await ctx.addInitScript(person);
  const page = await ctx.newPage();
  await reset(page);
  const panel = page.getByRole("dialog", { name: "Getting started" });
  await expect(panel).toBeVisible();
  const progress = panel.locator("[data-start-progress]");
  await expect(progress).toHaveText("Step 1 / 5 · Welcome");
  await expect(panel.getByRole("heading", { name: "Right-click anything" })).toBeVisible();

  // Next walks the steps; the rail jumps to one.
  await panel.locator("[data-start-next]").click();
  await expect(progress).toHaveText("Step 2 / 5 · Phone");
  // What the daemon says about Tailscale: the test machine may or may not
  // have it, but the button is there until it serves this daemon.
  await expect(panel.locator("[data-start-serve]")).toBeVisible();
  await panel.locator("[data-start-next]").click();
  await expect(progress).toHaveText("Step 3 / 5 · Cloud");
  await expect(panel.locator("[data-start-connect]")).toHaveText("Connect to Arugula cloud");
  await panel.locator("[data-start-next]").click();
  await expect(progress).toHaveText("Step 4 / 5 · Agents");
  await expect(panel.locator("[data-start-agent]")).toBeVisible();
  await panel.locator('[data-start-seg="try"]').click();
  await expect(progress).toHaveText("Step 5 / 5 · Things to try");
  // The test daemon isn't joined or served: those are still to do.
  await expect(panel.locator('[data-start-todo] [data-check="todo"]').first()).toBeVisible();
  await panel.getByRole("button", { name: "Done" }).click();
  await expect(panel).toBeHidden();
  // Closing says where it went.
  await expect(page.locator("[data-start-toast]")).toHaveText("Getting started is in the command palette, under Help, any time.");

  // Remembered: not again on reload.
  await page.reload();
  await expect.poll(() => page.evaluate(() => window.__arugula?.client.state !== null)).toBe(true);
  await expect(page.locator(".session-button")).toBeVisible();
  await expect(panel).toBeHidden();

  // Always in the command palette's Help, from the start.
  await paletteCommand(page, "Help/Getting started");
  await expect(panel).toBeVisible();
  await expect(progress).toHaveText("Step 1 / 5 · Welcome");
  // Its agents step opens the agent dialog.
  await panel.locator('[data-start-seg="agents"]').click();
  await panel.locator("[data-start-agent]").click();
  await expect(panel).toBeHidden();
  await ctx.close();
});

test("the desktop app up to 0.26.3 opens it from its Daemon menu", async ({ page }) => {
  // #708: that app names the event by the page's `__arugulaPage`, exactly so.
  await reset(page);
  const panel = page.getByRole("dialog", { name: "Getting started" });
  await expect(panel).toBeHidden();
  await page.evaluate(() =>
    dispatchEvent(
      new CustomEvent(((window as { __arugulaPage?: boolean }).__arugulaPage ? "arugula" : "illogical") + ":getting-started", {
        detail: "cloud",
      }),
    ),
  );
  await expect(panel).toBeVisible();
  await expect(panel.locator("[data-start-progress]")).toHaveText("Step 3 / 5 · Cloud");
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
  const panel = page.getByRole("dialog", { name: "Getting started" });
  await expect(panel.locator("[data-start-progress]")).toHaveText("Step 3 / 5 · Cloud");
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
    await expect(page.getByRole("dialog", { name: "Getting started" })).toBeHidden();
    await page.locator(".sheet-button").click();
    await sheetTab(page, "More");
    await page.getByRole("button", { name: "Getting started" }).click();
    await expect(page.getByRole("dialog", { name: "Getting started" })).toBeVisible();
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

test("Things to try: each button does its thing, and is marked done once used, for good", async ({ browser }) => {
  const ctx = await browser.newContext();
  const page = await ctx.newPage();
  await reset(page);
  const panel = page.getByRole("dialog", { name: "Getting started" });
  const tryStep = async () => {
    await paletteCommand(page, "Help/Getting started");
    await panel.locator('[data-start-seg="try"]').click();
    await expect(panel.locator("[data-start-tries]")).toBeVisible();
  };
  const item = (id: string) => panel.locator(`[data-start-try="${id}"]`);
  await tryStep();
  for (const id of ["swarm", "agent", "port", "pr", "push", "search"]) await expect(item(id)).toBeVisible();
  await expect(panel.locator("[data-start-try][data-done]")).toHaveCount(0);
  // The test daemon has no block sites: Open a port says why it can't.
  await expect(item("port").getByRole("button")).toBeDisabled();
  await expect(item("port")).toContainText("block sites");

  // Command search.
  await item("search").getByRole("button").click();
  await expect(panel).toBeHidden();
  await expect(page.getByRole("dialog", { name: "Command palette" })).toBeVisible();
  await page.keyboard.press("Escape");

  // Open a pull request asks which.
  await tryStep();
  await expect(item("search")).toHaveAttribute("data-done", "");
  await item("pr").getByRole("button").click();
  await expect(page.getByRole("dialog", { name: "Open a pull request" }).first()).toBeVisible();
  await page.keyboard.press("Escape");

  // Start an agent opens the agent dialog.
  await tryStep();
  await item("agent").getByRole("button").click();
  await expect(panel).toBeHidden();
  await expect(page.getByRole("dialog", { name: "Start an agent" }).first()).toBeVisible();
  await page.keyboard.press("Escape");

  // The swarm.
  await tryStep();
  await item("swarm").getByRole("button").click();
  await expect(page.locator(".swarm")).toBeVisible();
  expect(new URL(page.url()).hash).toMatch(/^#swarm/);

  // Remembered (the seen store, this browser's here), after a reload too.
  await page.goto("/");
  await expect.poll(() => page.evaluate(() => window.__arugula?.client.state !== null)).toBe(true);
  await expect(page.locator(".session-button")).toBeVisible();
  await tryStep();
  for (const id of ["search", "pr", "agent", "swarm"]) await expect(item(id)).toHaveAttribute("data-done", "");
  await expect(item("port")).not.toHaveAttribute("data-done", "");
  const seen = await page.evaluate(() => JSON.parse(localStorage.getItem("arugula.seen") ?? "{}"));
  expect(Object.keys(seen)).toEqual(expect.arrayContaining(["seen:getting-started.try.search", "seen:getting-started.try.swarm"]));
  await ctx.close();
});
