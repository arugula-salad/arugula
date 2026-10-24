// M1 on a phone: one pane at a time, a sheet to switch, and a key bar.

import { createServer } from "node:http";
import type { AddressInfo } from "node:net";
import { devices, expect, test } from "@playwright/test";
import { active, paneEl, ready, reset, text, type, sheetTab } from "./helpers";

const { viewport, userAgent, deviceScaleFactor, isMobile, hasTouch } = devices["Pixel 7"];
test.use({ viewport, userAgent, deviceScaleFactor, isMobile, hasTouch });

test("one pane at a time, switch from the sheet, extra keys work", async ({ page }) => {
  // M113: whatever this device last picked, a phone keeps its own header:
  // no rail, sidebar, toolbar or pane headers.
  await page.addInitScript(() => {
    try {
      localStorage.setItem("arugula-appearance", JSON.stringify({ palette: "default", finish: "flat", corners: "square", workspace: "new" }));
    } catch {
      // No storage: the same, without the old key.
    }
  });
  await reset(page);
  // A split made on the phone.
  await page.locator(".sheet-button").click();
  await sheetTab(page, "More");
  await page.getByRole("button", { name: "Split pane" }).click();
  await expect.poll(() => page.evaluate(() => window.__arugula.client.state!.panes.length)).toBe(2);
  // Only the active pane is drawn, filling the screen.
  await expect.poll(() => page.locator(".pane").count()).toBe(1);
  const shown = await active(page);
  await ready(page, shown);
  const fill = await page.locator(".pane").boundingBox();
  expect(fill!.width).toBeGreaterThan(380);
  await expect(page.locator(".phone-bar")).toBeVisible();
  await expect(page.locator(".ws-rail, .ws-head, [data-session-sidebar], [data-pane-toolbar], [data-pane-header]")).toHaveCount(0);

  // Ctrl from the key bar, then "c", interrupts a running command.
  await type(page, shown, "sleep 100\n");
  await page.getByRole("button", { name: "Ctrl" }).click();
  await expect(page.getByRole("button", { name: "Ctrl" })).toHaveAttribute("aria-pressed", "true");
  await page.keyboard.type("c");
  await expect(page.getByRole("button", { name: "Ctrl" })).toHaveAttribute("aria-pressed", "false");
  await page.keyboard.type("echo back-$((8+1))\n");
  await expect.poll(() => text(page, shown)).toContain("back-9");
  // Drawn on screen, not just in the terminal's buffer.
  await expect(page.locator(".pane .xterm-rows")).toContainText("back-9");

  // Up arrow from the key bar recalls the last command.
  await page.getByRole("button", { name: "↑" }).click();
  await page.getByRole("button", { name: "Tab" }).click();
  await page.keyboard.press("Enter");
  await expect.poll(async () => (await text(page, shown)).match(/back-9/g)?.length).toBe(2);

  // The sheet lists both panes; pick the other one.
  await page.locator(".sheet-button").click();
  const other = page.locator(".sheet-item.sheet-pane:not(.current)");
  await expect(other).toHaveCount(1);
  await other.click();
  await expect.poll(() => active(page)).not.toBe(shown);
  await expect.poll(() => page.locator(".pane").count()).toBe(1);
});

// #95: the README's phone step, on a phone: Notify this device is in the
// sheet, turns on, and the daemon sends this device a test notification.
// Headless Chrome has no push service, so the browser's subscription is a
// stand-in whose endpoint is ours: what the daemon sends lands here.
test("notify this device from the sheet", async ({ page, context }) => {
  const got: { encoding?: string; bytes: number }[] = [];
  const service = createServer((req, res) => {
    let bytes = 0;
    req.on("data", (c: Buffer) => (bytes += c.length));
    req.on("end", () => {
      got.push({ encoding: req.headers["content-encoding"] as string | undefined, bytes });
      res.writeHead(201).end();
    });
  });
  await new Promise<void>((r) => service.listen(0, "127.0.0.1", r));
  const endpoint = `http://127.0.0.1:${(service.address() as AddressInfo).port}/push/e2e`;
  await page.addInitScript((endpoint) => {
    let sub: PushSubscription | null = null;
    const b64 = (b: ArrayBuffer) => btoa(String.fromCharCode(...new Uint8Array(b))).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
    PushManager.prototype.subscribe = async function () {
      const pair = (await crypto.subtle.generateKey({ name: "ECDH", namedCurve: "P-256" }, true, ["deriveBits"])) as CryptoKeyPair;
      const keys = { p256dh: b64(await crypto.subtle.exportKey("raw", pair.publicKey)), auth: b64(crypto.getRandomValues(new Uint8Array(16)).buffer) };
      sub = { endpoint, toJSON: () => ({ endpoint, keys }), unsubscribe: async () => ((sub = null), true) } as unknown as PushSubscription;
      return sub;
    };
    PushManager.prototype.getSubscription = async () => sub;
  }, endpoint);
  await context.grantPermissions(["notifications"]);
  await reset(page);
  await page.locator(".sheet-button").click();
  await sheetTab(page, "More");
  const notify = page.getByRole("button", { name: "Notify this device" });
  await expect(notify).toHaveAttribute("aria-pressed", "false");
  await notify.click();
  await expect(notify).toHaveAttribute("aria-pressed", "true");
  await expect.poll(() => got.length).toBeGreaterThan(0);
  expect(got[0].encoding).toBe("aes128gcm");
  expect(got[0].bytes).toBeGreaterThan(0);
  // And off again.
  await notify.click();
  await expect(notify).toHaveAttribute("aria-pressed", "false");
  service.close();
});

// #331: with no session, the sheet's New tab makes one, and New agent and
// Conversations open (the daemon puts what they start in a new session).
test("with no sessions, the sheet's New tab makes one; New agent and Conversations open", async ({ page }) => {
  await reset(page);
  await page.evaluate(() => {
    const c = window.__arugula.client;
    for (const s of c.state!.sessions) c.intent({ op: "close_session", session: s.id });
  });
  await expect.poll(() => page.evaluate(() => window.__arugula.client.state!.sessions.length)).toBe(0);
  const sheet = async () => {
    await page.locator(".sheet-button").tap();
    await expect(page.locator(".sheet")).toBeVisible();
  };

  await sheet();
  await page.locator(".sheet [data-new-agent]").tap();
  await expect(page.getByRole("dialog", { name: "Start an agent" })).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(page.getByRole("dialog", { name: "Start an agent" })).toHaveCount(0);

  await sheet();
  await page.locator(".sheet [data-conversations]").tap();
  const conversations = page.getByRole("dialog", { name: "Claude Code conversations" });
  await expect(conversations).toBeVisible();
  // Its filter isn't focused on a phone (no keyboard popping up), so no
  // Escape: Cancel closes it.
  await conversations.getByRole("button", { name: "Cancel" }).tap();
  await expect(conversations).toHaveCount(0);

  await sheet();
  await page.locator(".sheet [data-new-tab]").tap();
  await expect.poll(() => page.evaluate(() => window.__arugula.client.state!.sessions.length)).toBe(1);
  await expect(page.locator("[data-no-sessions]")).toHaveCount(0);
});

// #237: share a session from the phone: the sheet's Share session… opens
// the session menu's picker, and a push's Share (`#pane=N&share`) opens
// the pane with it.
test("share a session from the sheet, and from a push's Share", async ({ page }) => {
  await reset(page);
  const { session, pane } = await page.evaluate(() => {
    const c = window.__arugula.client;
    return { session: c.state!.sessions[0].id, pane: c.state!.panes[0].id };
  });
  await page.locator(".sheet-button").tap();
  await sheetTab(page, "More");
  await page.locator(".sheet [data-share-session]").tap();
  await expect(page.locator(".sheet")).toHaveCount(0);
  const dialog = page.locator(`[data-share="${session}"]`);
  await expect(dialog).toBeVisible();
  // The same picker: a person, watch or drive.
  await expect(dialog.getByLabel("Who")).toBeVisible();
  await expect(dialog.getByLabel("Role", { exact: true })).toBeVisible();
  await dialog.getByRole("button", { name: "Done" }).tap();
  await expect(dialog).toHaveCount(0);

  // A push's Share opens a fresh page there (a hash alone doesn't load it).
  await page.goto(`/#pane=${pane}&share`);
  await page.reload();
  await expect(page.locator(`[data-share="${session}"]`)).toBeVisible();
  await expect.poll(() => page.evaluate(() => window.__arugula.client.active())).toBe(pane);
});

test("a keyboard suggestion replaces the word, not the line", async ({ page }) => {
  await reset(page);
  const shown = await active(page);
  await ready(page, shown);
  await paneEl(page, shown).click({ position: { x: 40, y: 40 } });
  // Gboard, as on a Pixel: each key is a keyCode 229 keydown, then the
  // textarea changes, sometimes after xterm.js has looked. A tapped
  // suggestion replaces the word in the textarea.
  await page.evaluate(async () => {
    const ta = document.querySelector<HTMLTextAreaElement>(".pane .xterm-helper-textarea")!;
    const edit = async (value: string, inputType: string) => {
      const key = new KeyboardEvent("keydown", { bubbles: true, cancelable: true, key: "Unidentified" });
      Object.defineProperty(key, "keyCode", { value: 229 });
      ta.dispatchEvent(key);
      await new Promise((r) => setTimeout(r, 20));
      ta.value = value;
      ta.dispatchEvent(new InputEvent("input", { bubbles: true, inputType }));
    };
    let typed = "";
    for (const c of "echo brpwn") await edit((typed += c), "insertText");
    await edit("echo brown", "insertReplacementText");
  });
  await page.keyboard.press("Enter");
  await expect.poll(() => text(page, shown)).toMatch(/^brown$/m);
  expect(await text(page, shown)).not.toContain("brpwn");
});
