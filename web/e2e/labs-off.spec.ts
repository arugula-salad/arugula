// The default, which is what a stranger gets: a machine without the `labs`
// flag shows no chat or threads, no VM or sandbox, and no
// "Invite over ssh" (Guest SSH access is a sharing setting, off by default,
// native #205). The rest of the suite
// runs with labs on (e2e/labs.ts), so this is the only proof of the default,
// and it is in the set that always runs (`E2E_SET=rest`).
//
// The daemon here is set up for every one of those (a sandbox provider
// and guest ssh), so only the missing
// file hides them: and the last test sets the flag, on the running daemon, and
// loads the page again to see them all. (developer-settings.spec.ts turns
// them on one at a time, from the page.)

import { spawn, type ChildProcess } from "node:child_process";
import { existsSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { expect, test, type Page } from "@playwright/test";
import { closeContexts, open, paneEl, ready, sheetTab } from "./helpers";
import { labs } from "./labs";
import { ANY, daemonPort } from "./ports";

const OWNER = "me@example.com";
let base = "";
let state = "";
let daemon: ChildProcess | undefined;

test.use({ baseURL: async ({}, use) => use(base) });
test.describe.configure({ mode: "serial" });
test.afterAll(closeContexts);

test.beforeAll(async () => {
  state = mkdtempSync(join(tmpdir(), "arugula-e2e-labs-off-"));
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

const api = (path: string, body?: unknown, as?: string) =>
  fetch(base + path, {
    method: body === undefined ? "GET" : "POST",
    headers: { ...(body === undefined ? {} : { "content-type": "application/json" }), ...(as ? { "tailscale-user-login": as } : {}) },
    body: body === undefined ? undefined : JSON.stringify(body),
  });

const features = async () => ((await (await api("/api/host")).json()) as { features: Record<string, boolean | string[]> }).features;

/** What this page shows to a stranger's eye, wherever labs would put it. */
const MENU_HIDDEN = [/^Thread\b/, /^Session thread/, /\bVM\b/, /^Sandboxes/, /^Invite over ssh/, /^Developer settings/];

async function menuItems(page: Page) {
  const items = (await page.getByRole("menuitem").allInnerTexts()).map((s) => s.trim());
  expect(items.length).toBeGreaterThan(3);
  return items;
}

async function noLabsItems(page: Page, pane: number) {
  // The pane's menu: still the usual, and still the read-only link.
  await paneEl(page, pane).click({ button: "right", position: { x: 60, y: 60 } });
  await expect(page.getByRole("menuitem", { name: "Share read-only link…" })).toBeVisible();
  for (const item of await menuItems(page)) for (const h of MENU_HIDDEN) expect(item, `pane menu: ${item}`).not.toMatch(h);
  await expect(page.getByRole("menuitem", { name: "Invite over ssh…" })).toHaveCount(0);
  // An app's box, by its address, is everyone's (native #206).
  await expect(page.getByRole("menuitem", { name: "Open an app…" })).toBeVisible();
  await page.keyboard.press("Escape");
  // The session menu and the + button's.
  for (const [where, button] of [
    [page.locator("[data-crumb-menu]"), "left"],
    [page.locator(".new-tab"), "right"],
  ] as const) {
    await where.click({ button });
    await expect(page.getByRole("menuitem").first()).toBeVisible();
    for (const item of await menuItems(page)) for (const h of MENU_HIDDEN) expect(item, `menu: ${item}`).not.toMatch(h);
    await page.keyboard.press("Escape");
  }
}

test("the host says labs is off, and what follows it", async () => {
  expect(await features()).toEqual({
    ...{ labs: false, dev: false, blocks: false, vms: false, block_channels: false },
    ...{ guest_ssh: false, flags: [] },
  });
});

test("links to Forgejo and GitLab stay plain links; GitHub's still open as blocks", async ({ page }) => {
  await open(page);
  await expect.poll(() => page.evaluate(() => window.__arugula.client.features !== null)).toBe(true);
  const kinds = (uris: string[]) =>
    page.evaluate((us) => us.map((u) => (window.__arugula.client as unknown as { forgeLink(u: string): string | null }).forgeLink(u)), uris);
  expect(
    await kinds([
      "https://git.example.test/o/r/pulls/3",
      "https://git.example.test/o/r/issues/3",
      "https://gitlab.com/g/p/-/merge_requests/2",
      "https://gitlab.com/g/p/-/issues/2",
      "https://github.com/o/r/pull/3",
      "https://github.com/o/r/issues/3",
    ]),
  ).toEqual([null, null, null, null, "pr", "issue"]);
});

test("on a desktop: no chat or threads, no VMs, no ssh invite", async ({ page }) => {
  await open(page);
  const pane = await page.evaluate(() => window.__arugula.client.state!.panes[0].id);
  await ready(page, pane);
  await expect.poll(() => page.evaluate(() => window.__arugula.client.features !== null)).toBe(true);
  expect(await page.evaluate(() => window.__arugula.client.hasLabs())).toBe(false);

  // The bar: Panes and Swarm, no Chat.
  await expect(page.locator("[data-open-swarm]")).toBeVisible();
  await expect(page.locator("[data-open-chat]")).toHaveCount(0);
  // The Docs page (#1494) isn't Labs' (native #204): in the rail, and at
  // its address.
  await expect(page.locator(".ws-rail [data-open-docs]")).toBeVisible();
  await page.goto("/#docs");
  await expect(page.locator("[data-docs-page]")).toBeVisible();
  await open(page);
  await ready(page, pane);
  await noLabsItems(page, pane);

  // The palette has no chat entry (and does have the rest). A chat block
  // ("Start a chat") isn't Labs: it's an agent block without tools.
  await page.keyboard.press("ControlOrMeta+Shift+KeyP");
  const palette = page.getByRole("dialog", { name: "Command palette" });
  await expect(palette).toBeVisible();
  await palette.getByRole("combobox", { name: "Command" }).fill("chat");
  await expect(palette.getByRole("option", { name: /chat/i }).filter({ hasNotText: "Start a chat" })).toHaveCount(0);
  await palette.getByRole("combobox", { name: "Command" }).fill("docs");
  await expect(palette.getByRole("option", { name: /Go to Docs/ })).toBeVisible();
  await palette.getByRole("combobox", { name: "Command" }).fill("split");
  await expect(palette.getByRole("option").first()).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(palette).toBeHidden();
});

/** The Labs half of the web client is one chunk (src/labs.ts), named `labs-<hash>.js`. */
const LABS_CHUNK = /\/assets\/labs-[\w-]+\.js/;

test("without labs the page never requests the Labs chunk", async ({ page }) => {
  const asked: string[] = [];
  page.on("request", (r) => asked.push(r.url()));
  await open(page);
  const pane = await page.evaluate(() => window.__arugula.client.state!.panes[0].id);
  await ready(page, pane);
  await expect.poll(() => page.evaluate(() => window.__arugula.client.features !== null)).toBe(true);
  // Open the places a Labs session would load it for: the menus and the swarm.
  await noLabsItems(page, pane);
  await page.goto("/#swarm");
  await expect(page.locator(".swarm")).toBeVisible();
  // And the Docs page (#1494): everyone's since native #204, a chunk of
  // its own, not Labs'.
  await page.goto("/#docs");
  await expect(page.locator("[data-docs-page]")).toBeVisible();
  // The page's own scripts did load, so the check looks at something.
  expect(asked.some((u) => /\/assets\/main-[\w-]+\.js/.test(u))).toBe(true);
  expect(asked.filter((u) => LABS_CHUNK.test(u))).toEqual([]);
});

test("without labs the swarm still offers its four views, and keeps what this browser saved", async ({ page }) => {
  // The views left Labs (the swarm-themes flag is retired): Theme is a
  // setting of the swarm's for everyone, kept in the browser.
  await page.addInitScript(() => localStorage.setItem("arugula.swarm.theme", "hive"));
  await open(page);
  await page.goto("/#swarm");
  await expect(page.locator(".swarm")).toBeVisible();
  await expect(page.locator(".swarm-bar")).toBeVisible();
  await expect.poll(() => page.evaluate(() => window.__arugula.client.features !== null)).toBe(true);
  await expect(page.locator("[data-theme-pick]")).toHaveCount(4);
  await expect(page.locator(".swarm")).toHaveAttribute("data-theme", "hive");
  await page.locator('[data-theme-pick="blocks"]').click();
  await expect(page.locator(".swarm")).toHaveAttribute("data-theme", "blocks");
  // Its tiles show no unread: chat is control's (M125), and there's none here.
  // (The fleet reads its panes after the swarm draws: wait for them.)
  await expect.poll(() => page.evaluate(() => window.__arugula.fleet.panes.length)).toBeGreaterThan(0);
  expect(await page.evaluate(() => window.__arugula.fleet.panes.every((p) => !p.unread && !p.mention))).toBe(true);
  // The rest of the bar is still there.
  await expect(page.locator('[data-g="machine"]')).toBeVisible();
});

test("an older daemon's mention notification opens only the pane", async ({ page }) => {
  const pane = ((await (await api("/api/panes")).json()) as { id: number }[])[0].id;
  // A cold load, as a tap on the notification is: not a navigation within a page.
  await page.goto(`/#pane=${pane}&thread=pane-${pane}`);
  await expect.poll(() => page.evaluate(() => window.__arugula?.client.connected)).toBe(true);
  await expect.poll(() => page.evaluate((p) => window.__arugula.client.active() === p, pane)).toBe(true);
  // Wait for the features, and then a moment more, since the failure would
  // be a drawer that opens late.
  await expect.poll(() => page.evaluate(() => window.__arugula.client.features !== null)).toBe(true);
  await page.waitForTimeout(500);
  await expect(page.locator(".thread-panel")).toHaveCount(0);
  await expect(paneEl(page, pane)).toBeVisible();
});

test.describe("on a phone", () => {
  test.use({ viewport: { width: 390, height: 760 }, isMobile: true, hasTouch: true });
  test("the sheet has no chat or VM entries", async ({ page }) => {
    await open(page);
    await expect.poll(() => page.evaluate(() => window.__arugula.client.features !== null)).toBe(true);
    await page.locator(".sheet-button").tap();
    await sheetTab(page, "More");
    // Here has chat when there is any, else New chat; apps are More's.
    await expect(page.locator("[data-open-chat]")).toHaveCount(0);
    await sheetTab(page, "More");
    // What it does have.
    await expect(page.locator("[data-open-swarm]")).toBeVisible();
    await expect(page.locator("[data-open-pr]")).toBeVisible();
    await expect(page.locator("[data-open-app]")).toBeVisible();
    await expect(page.locator("[data-open-chat]")).toHaveCount(0);
    for (const name of ["New VM tab", "Sandboxes"]) await expect(page.getByRole("button", { name })).toHaveCount(0);
    await expect(page.locator(".session-unread, .thread-badge")).toHaveCount(0);
  });
});

test("a daemon from before labs, which says nothing of it, gets none of it either", async ({ page }) => {
  // What `GET /api/host` says on an older daemon: every feature on (huddles'
  // `calls`, Managoat's `fountain` and the studio login's `studio` too, from
  // before they were removed),
  // and no `labs`. Control serves this page to such daemons too.
  await page.route("**/api/host", async (r) => {
    const res = await r.fetch();
    const host = (await res.json()) as { features?: Record<string, boolean> };
    host.features = { blocks: false, vms: true, fountain: true, studio: true, threads: true, calls: true };
    await r.fulfill({ response: res, json: host });
  });
  await open(page);
  const pane = await page.evaluate(() => window.__arugula.client.state!.panes[0].id);
  await ready(page, pane);
  await expect.poll(() => page.evaluate(() => window.__arugula.client.features?.vms)).toBe(true);
  expect(await page.evaluate(() => window.__arugula.client.hasLabs())).toBe(false);
  expect(await page.evaluate(() => window.__arugula.client.has("vms"))).toBe(false);
  await expect(page.locator("[data-open-chat]")).toHaveCount(0);
  await expect(page.locator(".thread-badge, .session-unread")).toHaveCount(0);
  await noLabsItems(page, pane);
});

test("a Doc needs no flag: made and shown with Labs off, and sharing still asks for control", async ({ page }) => {
  // Docs left Labs (native #204): an old flags.json that turned `docs` off
  // is ignored, as any retired name is.
  writeFileSync(join(state, "flags.json"), JSON.stringify({ flags: { docs: false } }));
  const pane = ((await (await api("/api/panes")).json()) as { id: number }[])[0].id;
  const made = (await (await api("/api/blocks", { type: "doc", config: { title: "Saved" }, split: pane, local: true })).json()) as { block: number };
  try {
    expect((await api(`/api/blocks/${made.block}/call/doc_outline`, { title: "Saved", sections: [{ heading: "One", intent: "the first" }] })).ok).toBe(true);
    await open(page);
    await expect.poll(() => page.evaluate(() => window.__arugula.client.features !== null)).toBe(true);
    await ready(page, pane);
    await expect(page.locator(`[data-doc-block="${made.block}"]`)).toBeVisible();
    await expect(paneEl(page, made.block)).not.toContainText("This block is in Labs");
    // Sharing is still an explicit act of the owner's, and it needs control
    // (#1066): graduating published nothing.
    const shared = await api(`/api/blocks/${made.block}/call/doc_share`, {});
    expect(shared.status).toBe(400);
    expect(await shared.text()).toContain("hasn't joined control");
  } finally {
    await api(`/api/panes/${made.block}/close`, {});
    rmSync(join(state, "flags.json"), { force: true });
  }
});

test("setting the flag on the running daemon brings it all back, with no restart", async ({ page }) => {
  expect(existsSync(join(state, "flags.json"))).toBe(false);
  const asked: string[] = [];
  page.on("request", (r) => asked.push(r.url()));
  labs(state);
  expect(await features()).toEqual({
    ...{ labs: true, dev: true, blocks: false, vms: true, block_channels: false },
    // `labs`, the one switch from before, is every flag (#665), and keeps guest ssh on as
    // Guest SSH access, a sharing setting (native #205).
    guest_ssh: true,
    flags: ["sandbox-providers", "chant-workspaces", "additional-forges", "agent-delegation"],
  });

  await open(page);
  const pane = await page.evaluate(() => window.__arugula.client.state!.panes[0].id);
  await ready(page, pane);
  await expect.poll(() => page.evaluate(() => window.__arugula.client.hasLabs())).toBe(true);
  // Chat is control's (M125), on its platform page (M155): a machine on its
  // own has none, and offers none, even with the flag.
  await expect(page.locator("[data-open-chat]")).toHaveCount(0);
  await expect(page.locator(".thread-badge")).toHaveCount(0);
  await open(page);
  await ready(page, pane);
  // Now the Labs chunk was fetched, since this machine has them.
  expect(asked.filter((u) => LABS_CHUNK.test(u)).length).toBeGreaterThan(0);
  await paneEl(page, pane).click({ button: "right", position: { x: 60, y: 60 } });
  for (const name of ["Invite over ssh…", "New VM pane on the right", "Open an app…"]) {
    await expect(page.getByRole("menuitem", { name })).toBeVisible();
  }
  await page.keyboard.press("Escape");

  await page.goto("/#swarm");
  await expect(page.locator("[data-theme-pick]")).toHaveCount(4);

  // And the Docs page: in the rail, and at its address.
  await expect(page.locator(".ws-rail [data-open-docs]")).toBeVisible();
  await page.goto("/#docs");
  await expect(page.locator("[data-docs-page]")).toBeVisible();
});
