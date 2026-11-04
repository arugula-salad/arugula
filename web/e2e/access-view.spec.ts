// Access (Arugula #681): the owner answers "who can reach this machine?"
// from one dialog, *Access…* in the machine's settings. Its rows are the
// daemon's own answer (`GET /api/access`): the owner, each session's shares,
// links to a pane with when they end, ssh guests where Guest SSH access is
// on, and trust live now, each row saying whose word it is. Its agents'
// powers are Arugula cloud's word, and this daemon isn't joined, so the
// dialog says so instead of asking. An older daemon has no `access.get`:
// behind this page it answers 404, and the dialog says what to do.

import { spawn, type ChildProcess } from "node:child_process";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { expect, test, type Page } from "@playwright/test";
import { closeContexts, open, ready } from "./helpers";
import { ANY, daemonPort } from "./ports";
import { labs } from "./labs";

test.afterAll(closeContexts);

const OWNER = "me@example.com";
const FRIEND = "friend@example.com";
let base = "";
let dir = "";
let daemon: ChildProcess | undefined;

test.use({ baseURL: async ({}, use) => use(base) });
test.describe.configure({ mode: "serial" });

test.beforeAll(async () => {
  dir = mkdtempSync(join(tmpdir(), "arugula-e2e-access-view-"));
  daemon = spawn(
    "../target/debug/arugulad",
    [
      ...["--listen", ANY, "--state-dir", labs(dir), "--owner", OWNER],
      ...["--shell", "bash --norc --noprofile", "--no-manager-env", "--tailscale-socket", "/nonexistent/sock"],
      ...["--guest-ssh", "127.0.0.1:0", "--guest-ssh-host", "127.0.0.1"],
    ],
    { stdio: "ignore" },
  );
  base = `http://127.0.0.1:${await daemonPort(dir, daemon)}`;
});

test.afterAll(() => {
  daemon?.kill("SIGKILL");
  if (dir) rmSync(dir, { recursive: true, force: true });
});

const api = (path: string, body?: unknown, method = body === undefined ? "GET" : "POST") =>
  fetch(base + path, {
    method,
    headers: body === undefined ? {} : { "content-type": "application/json" },
    body: body === undefined ? undefined : JSON.stringify(body),
  });

const dialog = (page: Page) => page.getByRole("dialog", { name: "Access" });

async function openDialog(page: Page) {
  await page.locator(".ws-head [data-account-menu]").click();
  await page.getByRole("menuitem", { name: "Access…", exact: true }).click();
  await expect(dialog(page)).toBeVisible();
}

test("the owner sees who reaches the machine, each row saying whose word it is", async ({ page }) => {
  await open(page);
  const pane = await page.evaluate(() => window.__arugula.client.state!.panes[0].id);
  await ready(page, pane);
  // The first pane is in the first session.
  const [session, name] = await page.evaluate(() => [window.__arugula.client.state!.sessions[0].id, window.__arugula.client.state!.sessions[0].name]);
  const where = `$${session} ${name}`;

  // Nothing shared yet: the owner alone, and no powers since it isn't joined.
  await openDialog(page);
  await expect(dialog(page).locator("[data-access-kind]")).toContainText("Not joined to Arugula cloud");
  await expect(dialog(page).locator("[data-access-row]")).toHaveCount(1);
  const owner = dialog(page).locator('[data-access-row="owner"]');
  await expect(owner).toContainText(OWNER);
  await expect(owner.locator("[data-access-role]")).toHaveText("owner");
  await expect(owner.locator("[data-access-where]")).toHaveText("every session");
  await expect(owner.locator('[data-access-word="this_machine"]')).toHaveText("this machine");
  await expect(dialog(page).locator("[data-access-powers-none]")).toContainText("Not joined to Arugula cloud");
  await dialog(page).getByRole("button", { name: "Done", exact: true }).click();
  await expect(dialog(page)).toBeHidden();

  // A session share, a link to a pane, trust on a pane, and an ssh guest
  // (Guest SSH access is on here, as every Labs flag from before is): one
  // row each, where it reaches and until when.
  expect((await api("/api/acl", { session, principal: `tailnet:${FRIEND}`, role: "editor", history: false })).ok).toBe(true);
  expect((await api("/api/shares", { pane, ttl_secs: 600 })).ok).toBe(true);
  await page.evaluate(
    ({ pane, to }) => window.__arugula.client.paneOp(pane, { op: "grant_trust", to, minutes: 5 }),
    { pane, to: `tailnet:${FRIEND}` },
  );
  expect((await api("/api/guests", { pane, rw: true, label: "sam" })).ok).toBe(true);

  await openDialog(page);
  await expect(dialog(page).locator("[data-access-row]")).toHaveCount(5);
  const share = dialog(page).locator('[data-access-row="share"]');
  await expect(share).toContainText(FRIEND);
  await expect(share.locator("[data-access-role]")).toHaveText("drives");
  await expect(share.locator("[data-access-where]")).toHaveText(where);
  await expect(share).toContainText("from");
  const link = dialog(page).locator('[data-access-row="link"]');
  await expect(link.locator("[data-access-where]")).toHaveText(`%${pane} in ${where}`);
  await expect(link).toContainText("until");
  const guest = dialog(page).locator('[data-access-row="guest"]');
  await expect(guest).toContainText("sam");
  await expect(guest.locator("[data-access-role]")).toHaveText("drives");
  await expect(guest).toContainText("over ssh, may type");
  const trust = dialog(page).locator('[data-access-row="trust"]');
  await expect(trust).toContainText(FRIEND);
  await expect(trust.locator("[data-access-where]")).toHaveText(`%${pane} in ${where}`);
  await expect(dialog(page).locator('[data-access-word="this_machine"]')).toHaveCount(5);
  await expect(dialog(page).locator('[data-access-word="roster"]')).toHaveCount(0);
  await page.keyboard.press("Escape");
  await expect(dialog(page)).toBeHidden();
});

test("a guest never gets the view: the operation is the owner's", async () => {
  const r = await fetch(`${base}/api/access`, { headers: { "tailscale-user-login": FRIEND } });
  expect(r.status).toBe(403);
});

test("an older daemon behind this page answers 404, and the dialog says what to do", async ({ page }) => {
  await page.route("**/api/access", (r) => r.fulfill({ status: 404, json: { error: "no such route" } }));
  await open(page);
  const pane = await page.evaluate(() => window.__arugula.client.state!.panes[0].id);
  await ready(page, pane);
  await openDialog(page);
  await expect(dialog(page).locator("[data-access-error]")).toContainText("older than this page");
  await expect(dialog(page).locator("[data-access-error]")).toContainText("arugula access");
  await expect(dialog(page).locator("[data-access-row]")).toHaveCount(0);
  await page.keyboard.press("Escape");
});
