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
  writeFileSync(join(state, "wisp-token"), "e2e-wisp-token");
  daemon = spawn(
    "../target/debug/arugulad",
    [
      ...["--listen", ANY, "--state-dir", state, "--shell", "bash --norc --noprofile", "--no-manager-env"],
      ...["--owner", OWNER, "--tailscale-socket", "/nonexistent/tailscaled.sock"],
      ...["--wisp-url", "http://127.0.0.1:9", "--wisp-token-file", join(state, "wisp-token")],
      ...["--guest-ssh", "127.0.0.1:0", "--guest-ssh-host", "127.0.0.1"],
    ],
    { stdio: "ignore" },
  );
  base = `http://127.0.0.1:${await daemonPort(state, daemon)}`;
});

test.afterAll(() => {
  daemon?.kill("SIGKILL");
  if (state) rmSync(state, { recursive: true, force: true });
});

const FLAGS = ["sandbox-providers", "chant-workspaces", "additional-forges", "agent-delegation"] as const;
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

/** Which flags' features this page shows, each by its own sign; and
 * `guest-ssh` for *Invite over ssh…*, which is a sharing setting now
 * (native #205), so no flag shows it. */
async function shown(page: Page, pane: number): Promise<(Flag | "guest-ssh")[]> {
  await page.evaluate(() => window.__arugula.client.loadFeatures());
  const out: (Flag | "guest-ssh")[] = [];
  await paneEl(page, pane).click({ button: "right", position: { x: 60, y: 60 } });
  await expect(page.getByRole("menuitem", { name: "Share read-only link…" })).toBeVisible();
  const items = (await page.getByRole("menuitem").allInnerTexts()).map((s) => s.trim());
  await page.keyboard.press("Escape");
  if (items.includes("New VM pane on the right")) out.push("sandbox-providers");
  if (items.includes("Invite over ssh…")) out.push("guest-ssh");
  const page_says = await page.evaluate(() => {
    const c = window.__arugula.client as unknown as { flag(n: string): boolean; forgeLink(u: string): string | null };
    return {
      workspaces: c.flag("chant-workspaces"),
      forges: c.forgeLink("https://gitlab.com/g/p/-/merge_requests/2"),
      // M76 is the daemon's and the CLI's; the page's catalog is M77's.
      agents: c.flag("agent-delegation"),
    };
  });
  if (page_says.workspaces) out.push("chant-workspaces");
  if (page_says.forges) out.push("additional-forges");
  if (page_says.agents) out.push("agent-delegation");
  return [...FLAGS, "guest-ssh" as const].filter((f) => out.includes(f));
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
  await page.locator(".ws-head [data-account-menu]").click();
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

const workspacesFlag = (page: Page) => page.evaluate(() => window.__arugula.client.flag("workspaces"));

test("turning chant workspaces on in the dialog turns on them, and nothing else, without a reload", async ({ page }) => {
  const pane = await start(page);
  await page.goto("/#developer");
  // Chat isn't a flag (native #203): it's control's, by team.
  await expect(dialog(page).locator('[data-flag="chat"]')).toHaveCount(0);
  await dialog(page).locator('[data-flag="workspaces"] input').check();
  await expect.poll(() => workspacesFlag(page)).toBe(true);
  await expect(dialog(page).locator("[data-flag] input:checked")).toHaveCount(1);
  expect(onNow()).toEqual(["workspaces"]);
  await dialog(page).getByRole("button", { name: "Done", exact: true }).click();
  await expect(dialog(page)).toBeHidden();
  expect(await shown(page, pane)).toEqual(["workspaces"]);
  // The swarm's views are no flag's: all four, whatever is on.
  await page.goto("/#swarm");
  await expect(page.locator(".swarm")).toBeVisible();
  await expect(page.locator("[data-theme-pick]")).toHaveCount(4);
});

test("once asked for, the account menu offers them; off takes chant workspaces away again", async ({ page }) => {
  const pane = await start(page);
  expect(await workspacesFlag(page)).toBe(true);
  await page.locator(".ws-head [data-account-menu]").click();
  await page.getByRole("menuitem", { name: "Developer settings…" }).click();
  await expect(dialog(page).locator('[data-flag="workspaces"] input')).toBeChecked();
  await dialog(page).locator('[data-flag="workspaces"] input').uncheck();
  await expect.poll(() => workspacesFlag(page)).toBe(false);
  expect(onNow()).toEqual([]);
  await page.keyboard.press("Escape");
  expect(await shown(page, pane)).toEqual([]);
  // Still offered: the machine asked, and all off is an answer.
  await page.locator(".ws-head [data-account-menu]").click();
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

test("a renamed flag's old name still turns its feature on, under its new name", async ({ page }) => {
  // aru-117: `PUT /api/flags/vms` is `sandbox-providers`; only new names show.
  const pane = await start(page);
  const renamed = [
    ["vms", "sandbox-providers"],
    ["workspaces", "chant-workspaces"],
    ["forges", "additional-forges"],
    ["agents", "agent-delegation"],
  ] as const;
  for (const [old, now] of renamed) {
    expect((await setFlag(old, true)).ok, old).toBe(true);
    expect(onNow(), old).toEqual([now]);
    expect(await shown(page, pane), `${old} on`).toEqual([now]);
    expect((await setFlag(old, false)).ok, old).toBe(true);
    expect(await shown(page, pane), `${old} off`).toEqual([]);
  }
  await page.goto("/#developer");
  await expect(dialog(page).locator('[data-flag="vms"]')).toHaveCount(0);
  await expect(dialog(page).locator('[data-flag="sandbox-providers"]')).toHaveCount(1);
});

/** Requests that are each flag's feature (#347): its routes, and making
 * its blocks. Configs that don't parse, so with the flag on they fail past
 * the gate, in their own words, and nothing starts. (The swarm's
 * views have no route.) */
const GATED: [Flag, string, string, unknown][] = [
  ["sandbox-providers", "GET", "/api/machines", undefined],
  ["sandbox-providers", "GET", "/api/sandboxes", undefined],
  ["chant-workspaces", "POST", "/api/blocks", { type: "workspace", config: {}, local: true }],
  ["additional-forges", "POST", "/api/blocks", { type: "forge", config: { repo: "o/r", number: 0, provider: "forgejo" }, local: true }],
  ["agent-delegation", "GET", "/api/a2a/agents", undefined],
];

const ask = async (method: string, path: string, body: unknown) => {
  const r = await fetch(base + path, {
    method,
    headers: body === undefined ? {} : { "content-type": "application/json" },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  return { status: r.status, text: await r.text() };
};

test("each flag's routes and blocks follow it: refused off, saying how to turn it on, and answered on", async () => {
  for (const [flag, method, path, body] of GATED) {
    const off = await ask(method, path, body);
    expect(off.status, `${flag} off: ${method} ${path}`).toBeGreaterThanOrEqual(400);
    expect(off.text, `${flag} off: ${method} ${path}`).toContain(`\`arugulad flags ${flag} on\``);
    expect((await setFlag(flag, true)).ok, flag).toBe(true);
    const on = await ask(method, path, body);
    expect(on.text, `${flag} on: ${method} ${path}`).not.toContain("arugulad flags");
    expect((await setFlag(flag, false)).ok, flag).toBe(true);
    // And off again, with no restart.
    expect((await ask(method, path, body)).text, `${flag} off again`).toContain(`\`arugulad flags ${flag} on\``);
  }
});

const setSharing = (guest_ssh: boolean, as?: string) =>
  fetch(`${base}/api/sharing`, {
    method: "PUT",
    headers: { "content-type": "application/json", ...(as ? { "tailscale-user-login": as } : {}) },
    body: JSON.stringify({ guest_ssh }),
  });

const sharingDialog = (page: Page) => page.getByRole("dialog", { name: "Guest SSH access" });

test("guest ssh is a sharing setting, off by default: the machine's settings turn the ssh invite on", async ({ page }) => {
  // Not a flag any more (native #205): the old name is refused, saying where it went.
  const old = await setFlag("guest-ssh", true);
  expect(old.ok).toBe(false);
  expect(await old.text()).toContain("sharing setting");
  const pane = await start(page);
  expect(await shown(page, pane)).toEqual([]);
  const off = await ask("POST", "/api/guests", { pane });
  expect(off.status).toBe(404);
  expect(off.text).toContain("`arugulad sharing guest-ssh on`");

  await page.locator(".ws-head [data-account-menu]").click();
  await page.getByRole("menuitem", { name: "Guest SSH access…" }).click();
  const sw = sharingDialog(page).getByRole("switch", { name: "Guest SSH access on this machine" });
  await expect(sw).not.toBeChecked();
  await sw.check();
  await expect.poll(() => page.evaluate(() => window.__arugula.client.guestSsh())).toBe(true);
  expect(JSON.parse(readFileSync(join(state, "sharing.json"), "utf8"))).toEqual({ guest_ssh: true });
  await sharingDialog(page).getByRole("button", { name: "Done", exact: true }).click();
  expect(await shown(page, pane)).toEqual(["guest-ssh"]);
});

test("an ssh invite from the pane's menu works with Guest SSH access on, and is refused once it's off", async ({ page }) => {
  expect((await setSharing(true)).ok).toBe(true);
  const pane = await start(page);
  await page.evaluate(() => window.__arugula.client.loadFeatures());
  await paneEl(page, pane).click({ button: "right", position: { x: 60, y: 60 } });
  await page.getByRole("menuitem", { name: "Invite over ssh…" }).click();
  await expect(page.locator(".prompt-backdrop input")).toHaveValue(/^ssh -p \d+ .* g[0-9a-f]{32}@127\.0\.0\.1$/);
  await page.keyboard.press("Escape");
  for (const i of (await (await fetch(`${base}/api/guests`)).json()) as { id: number }[]) {
    await fetch(`${base}/api/guests/${i.id}`, { method: "DELETE" });
  }
  // Off: the menu loses it, and the route behind it says why.
  expect((await setSharing(false)).ok).toBe(true);
  const r = await ask("POST", "/api/guests", { pane });
  expect(r.status).toBe(404);
  expect(r.text).toContain("`arugulad sharing guest-ssh on`");
  expect(await shown(page, pane)).toEqual([]);
});

test("the swarm's views are no flag: swarm-themes is retired", async ({ page }) => {
  // Every swarm offers its four views (they left Labs); the old name is
  // refused as any retired flag is, and an old file naming it is ignored.
  const r = await setFlag("swarm-themes", true);
  expect(r.ok).toBe(false);
  await open(page);
  await page.goto("/#swarm");
  await expect(page.locator(".swarm")).toBeVisible();
  await expect(page.locator("[data-theme-pick]")).toHaveCount(4);
});

test("Docs are no flag: docs is retired, and the rail offers them with every flag off", async ({ page }) => {
  // Doc blocks, the doc tool and the Docs page left Labs (native #204); the
  // old name is refused as any retired flag is.
  const r = await setFlag("docs", true);
  expect(r.ok).toBe(false);
  expect(await r.text()).toContain("isn't a flag any more");
  await open(page);
  await expect(page.locator(".ws-rail [data-open-docs]")).toBeVisible();
  expect((await ask("GET", "/api/docs", undefined)).status).toBe(200);
});

test("chat is no flag: chat is retired, and control's by team", async () => {
  const r = await setFlag("chat", true);
  expect(r.ok).toBe(false);
  expect(await r.text()).toContain("isn't a flag any more");
  expect(onNow()).toEqual([]);
});

test("the flags are the owner's: someone else can't read or change them", async () => {
  const other = "friend@example.com";
  expect((await setFlag("vms", true, other)).status).toBe(403);
  expect((await fetch(`${base}/api/flags`, { headers: { "tailscale-user-login": other } })).status).toBe(403);
  expect(onNow()).toEqual([]);
  // Guest SSH access too (native #205).
  expect((await setSharing(true, other)).status).toBe(403);
  expect((await fetch(`${base}/api/sharing`)).ok).toBe(true);
  expect(((await (await fetch(`${base}/api/sharing`)).json()) as { guest_ssh: boolean }).guest_ssh).toBe(false);
});
