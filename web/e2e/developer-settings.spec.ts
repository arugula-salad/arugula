// Developer settings (#665): each Labs feature has a flag of its own, which
// the owner turns on for a machine in a dialog. A machine that hasn't asked
// isn't offered the dialog (`#developer` opens it all the same, as the
// desktop app's Daemon menu does); one flag turns on its feature and no
// other, with no restart and no reload; and off takes it away again.
//
// The daemon is set up for every Labs feature, like labs-off.spec.ts's, so
// only the flags decide what shows. That spec is the proof of the default
// and of `labs` from before, which is every flag.

import { spawn, type ChildProcess } from "node:child_process";
import { existsSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { expect, test, type Page } from "@playwright/test";
import { closeContexts, open, paneEl, ready } from "./helpers";
import { ANY, daemonPort } from "./ports";

const OWNER = "me@example.com";
let base = "";
let state = "";
let daemon: ChildProcess | undefined;

test.use({ baseURL: async ({}, use) => use(base) });
test.describe.configure({ mode: "serial" });
test.afterAll(closeContexts);

test.beforeAll(async () => {
  state = mkdtempSync(join(tmpdir(), "arugula-e2e-developer-"));
  writeFileSync(join(state, "fountain-credentials"), '[default]\napi_key = "ftn_test_e2e"\n');
  writeFileSync(join(state, "studio.json"), JSON.stringify({ url: "http://127.0.0.1:9", token: "e2e-studio-token" }));
  writeFileSync(join(state, "wisp-token"), "e2e-wisp-token");
  daemon = spawn(
    "../target/debug/arugulad",
    [
      ...["--listen", ANY, "--state-dir", state, "--shell", "bash --norc --noprofile", "--no-manager-env"],
      ...["--owner", OWNER, "--tailscale-socket", "/nonexistent/tailscaled.sock"],
      ...["--wisp-url", "http://127.0.0.1:9", "--wisp-token-file", join(state, "wisp-token")],
      ...["--guest-ssh", "127.0.0.1:0", "--guest-ssh-host", "127.0.0.1"],
    ],
    {
      stdio: "ignore",
      env: { ...process.env, ARUGULA_FOUNTAIN_CREDENTIALS: join(state, "fountain-credentials") },
    },
  );
  base = `http://127.0.0.1:${await daemonPort(state, daemon)}`;
});

test.afterAll(() => {
  daemon?.kill("SIGKILL");
  if (state) rmSync(state, { recursive: true, force: true });
});

const FLAGS = ["chat", "huddles", "vms", "fountain", "studio", "workspaces", "guest-ssh", "swarm-themes", "forges", "agents"] as const;
type Flag = (typeof FLAGS)[number];

const setFlag = (flag: string, on: boolean, as?: string) =>
  fetch(`${base}/api/flags/${flag}`, {
    method: "PUT",
    headers: { "content-type": "application/json", ...(as ? { "tailscale-user-login": as } : {}) },
    body: JSON.stringify({ on }),
  });

const onNow = () =>
  Object.entries((JSON.parse(readFileSync(join(state, "flags.json"), "utf8")) as { flags: Record<string, boolean> }).flags)
    .filter(([, on]) => on)
    .map(([name]) => name);

/** Which flags' features this page shows, each by its own sign. */
async function shown(page: Page, pane: number): Promise<Flag[]> {
  await page.evaluate(() => window.__arugula.client.loadFeatures());
  const out: Flag[] = [];
  if (await page.locator("[data-open-chat]").count()) out.push("chat");
  if (await page.locator("[data-huddle]").count()) out.push("huddles");
  await paneEl(page, pane).click({ button: "right", position: { x: 60, y: 60 } });
  await expect(page.getByRole("menuitem", { name: "Share read-only link…" })).toBeVisible();
  const items = (await page.getByRole("menuitem").allInnerTexts()).map((s) => s.trim());
  await page.keyboard.press("Escape");
  if (items.includes("New VM pane on the right")) out.push("vms");
  if (items.includes("Fountain agents…")) out.push("fountain");
  if (items.includes("Open a studio app…")) out.push("studio");
  if (items.includes("Invite over ssh…")) out.push("guest-ssh");
  const page_says = await page.evaluate(() => {
    const c = window.__arugula.client as unknown as { flag(n: string): boolean; forgeLink(u: string): string | null };
    return {
      workspaces: c.flag("workspaces"),
      themes: c.flag("swarm-themes"),
      forges: c.forgeLink("https://gitlab.com/g/p/-/merge_requests/2"),
      // M76 is the daemon's and the CLI's; the page's catalog is M77's.
      agents: c.flag("agents"),
    };
  });
  if (page_says.workspaces) out.push("workspaces");
  if (page_says.themes) out.push("swarm-themes");
  if (page_says.forges) out.push("forges");
  if (page_says.agents) out.push("agents");
  return FLAGS.filter((f) => out.includes(f));
}

async function start(page: Page) {
  await open(page);
  const pane = await page.evaluate(() => window.__arugula.client.state!.panes[0].id);
  await ready(page, pane);
  await expect.poll(() => page.evaluate(() => window.__arugula.client.features !== null)).toBe(true);
  return pane;
}

const dialog = (page: Page) => page.getByRole("dialog", { name: "Developer settings" });

test("a machine that hasn't asked isn't offered Developer settings, and #developer opens them", async ({ page }) => {
  const pane = await start(page);
  expect(existsSync(join(state, "flags.json"))).toBe(false);
  expect(await shown(page, pane)).toEqual([]);
  await page.locator(".session-button").click();
  await expect(page.getByRole("menuitem", { name: "Permission rules…" })).toBeVisible();
  await expect(page.getByRole("menuitem", { name: "Developer settings…" })).toHaveCount(0);
  await page.keyboard.press("Escape");

  // As the desktop app's Daemon menu opens it: every flag, all off.
  await page.goto("/#developer");
  await expect(dialog(page)).toBeVisible();
  await expect(dialog(page).locator("[data-flag]")).toHaveCount(FLAGS.length);
  await expect(dialog(page).locator("[data-flag] input:checked")).toHaveCount(0);
  // Looking makes no file: the machine still hasn't asked.
  expect(existsSync(join(state, "flags.json"))).toBe(false);
});

test("turning chat on in the dialog shows chat, and nothing else, without a reload", async ({ page }) => {
  const pane = await start(page);
  await page.goto("/#developer");
  await dialog(page).locator('[data-flag="chat"] input').check();
  await expect(page.locator("[data-open-chat]")).toBeVisible();
  await expect(dialog(page).locator("[data-flag] input:checked")).toHaveCount(1);
  expect(onNow()).toEqual(["chat"]);
  await dialog(page).getByRole("button", { name: "Close" }).click();
  await expect(dialog(page)).toBeHidden();
  expect(await shown(page, pane)).toEqual(["chat"]);
  // The swarm keeps its one view.
  await page.goto("/#swarm");
  await expect(page.locator(".swarm")).toBeVisible();
  await expect(page.locator("[data-theme-pick]")).toHaveCount(0);
});

test("once asked for, the session menu offers them; off takes chat away again", async ({ page }) => {
  const pane = await start(page);
  await expect(page.locator("[data-open-chat]")).toBeVisible();
  await page.locator(".session-button").click();
  await page.getByRole("menuitem", { name: "Developer settings…" }).click();
  await expect(dialog(page).locator('[data-flag="chat"] input')).toBeChecked();
  await dialog(page).locator('[data-flag="chat"] input').uncheck();
  await expect(page.locator("[data-open-chat]")).toHaveCount(0);
  expect(onNow()).toEqual([]);
  await page.keyboard.press("Escape");
  expect(await shown(page, pane)).toEqual([]);
  // Still offered: the machine asked, and all off is an answer.
  await page.locator(".session-button").click();
  await expect(page.getByRole("menuitem", { name: "Developer settings…" })).toBeVisible();
});

test("each flag turns on its own feature and no other", async ({ page }) => {
  const pane = await start(page);
  for (const flag of FLAGS) {
    expect((await setFlag(flag, true)).ok, flag).toBe(true);
    expect(await shown(page, pane), `${flag} on`).toEqual([flag]);
    expect((await setFlag(flag, false)).ok, flag).toBe(true);
    expect(await shown(page, pane), `${flag} off`).toEqual([]);
  }
});

test("the extra swarm views follow their flag", async ({ page }) => {
  expect((await setFlag("swarm-themes", true)).ok).toBe(true);
  await open(page);
  await page.goto("/#swarm");
  await expect(page.locator(".swarm")).toBeVisible();
  await expect(page.locator("[data-theme-pick]")).toHaveCount(4);
  await expect(page.locator("[data-open-chat]")).toHaveCount(0);
  expect((await setFlag("swarm-themes", false)).ok).toBe(true);
});

test("the flags are the owner's: someone else can't read or change them", async () => {
  const other = "friend@example.com";
  expect((await setFlag("chat", true, other)).status).toBe(403);
  expect((await fetch(`${base}/api/flags`, { headers: { "tailscale-user-login": other } })).status).toBe(403);
  expect(onNow()).toEqual([]);
});
