// Labs from the page (#464): the owner turns the `labs` flag on from the
// session menu's Labs…, the page reloads and chat appears; then turns it off
// and chat goes. The daemon starts with no flags file, as a new install, and
// the switch writes `flags.json` in its state directory.

import { spawn, type ChildProcess } from "node:child_process";
import { mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { expect, test, type Page } from "@playwright/test";
import { closeContexts, open } from "./helpers";
import { ANY, daemonPort } from "./ports";

let base = "";
let state = "";
let daemon: ChildProcess | undefined;

test.use({ baseURL: async ({}, use) => use(base) });
test.describe.configure({ mode: "serial" });
test.afterAll(closeContexts);

test.beforeAll(async () => {
  state = mkdtempSync(join(tmpdir(), "arugula-e2e-labs-settings-"));
  daemon = spawn(
    "../target/debug/arugulad",
    [
      ...["--listen", ANY, "--state-dir", state, "--shell", "bash --norc --noprofile", "--no-manager-env"],
      ...["--owner", "me@example.com", "--tailscale-socket", "/nonexistent/tailscaled.sock"],
    ],
    { stdio: "ignore" },
  );
  base = `http://127.0.0.1:${await daemonPort(state, daemon)}`;
});

test.afterAll(() => {
  daemon?.kill("SIGKILL");
  if (state) rmSync(state, { recursive: true, force: true });
});

const flags = () => JSON.parse(readFileSync(join(state, "flags.json"), "utf8")) as { flags: Record<string, boolean> };

async function flip(page: Page, button: string) {
  await page.locator(".session-button").click();
  await page.getByRole("menuitem", { name: "Labs…" }).click();
  const dialog = page.getByRole("dialog", { name: "Labs" });
  await expect(dialog.locator("[data-flag=labs]")).toContainText("Turns on what a new install doesn't show");
  // The page loads again, with the daemon's new answer to /api/host.
  const reloaded = page.waitForEvent("load");
  await dialog.getByRole("button", { name: button }).click();
  await reloaded;
}

test("Labs…, turned on, brings chat; turned off, takes it away", async ({ page }) => {
  await open(page);
  await expect.poll(() => page.evaluate(() => window.__arugula.client.features !== null)).toBe(true);
  await expect(page.locator("[data-open-chat]")).toHaveCount(0);

  await flip(page, "Off: turn on");
  await expect.poll(() => page.evaluate(() => window.__arugula?.client.hasLabs())).toBe(true);
  await expect(page.locator("[data-open-chat]")).toBeVisible();
  expect(flags()).toEqual({ flags: { labs: true } });

  await flip(page, "On: turn off");
  await expect.poll(() => page.evaluate(() => window.__arugula?.client.features != null)).toBe(true);
  expect(await page.evaluate(() => window.__arugula.client.hasLabs())).toBe(false);
  await expect(page.locator("[data-open-chat]")).toHaveCount(0);
  expect(flags()).toEqual({ flags: { labs: false } });
});
