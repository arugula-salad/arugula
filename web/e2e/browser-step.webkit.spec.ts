// #381 in WebKit (Safari's engine, where the desktop app's window runs): in
// the app with the `open_outside` command (faked here), Add passkey ↗ asks
// the app to open control's #account on the first click and shows the
// waiting state, with no second "Open … in your browser" click; *Open again*
// asks once more. An app without the command keeps the open button.
// chrome's copy is control.spec.ts's, with the platform pages' in
// browser-step.spec.ts.

import { spawn, type ChildProcess } from "node:child_process";
import { mkdtempSync, rmSync } from "node:fs";
import { createServer, type Server } from "node:http";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { expect, test, type Page } from "@playwright/test";
import { controlPanel } from "./helpers";
import { ANY, controlPort, listen } from "./ports";

let base = "";
const procs: ChildProcess[] = [];
const dirs: string[] = [];
let gh: Server;

test.describe.configure({ mode: "serial" });
// Playwright's WebKit stops loading the page once the service worker is
// registered (real Safari doesn't), so none here.
test.use({ baseURL: async ({}, use) => use(base), serviceWorkers: "block" });

test.beforeAll(async () => {
  gh = createServer((req, res) => {
    const u = new URL(req.url!, "http://github");
    if (u.pathname === "/login/oauth/authorize") {
      const back = new URL(u.searchParams.get("redirect_uri")!);
      back.searchParams.set("code", "c0de");
      back.searchParams.set("state", u.searchParams.get("state")!);
      res.writeHead(302, { location: back.href }).end();
    } else if (u.pathname === "/login/oauth/access_token") {
      res.writeHead(200, { "content-type": "application/json" }).end(JSON.stringify({ access_token: "gho_test" }));
    } else if (u.pathname === "/user") {
      res.writeHead(200, { "content-type": "application/json" }).end(JSON.stringify({ id: 7, login: "stranger" }));
    } else res.writeHead(404).end();
  });
  const github = `http://127.0.0.1:${await listen(gh)}`;
  const dir = mkdtempSync(join(tmpdir(), "arugula-e2e-browser-step-"));
  dirs.push(dir);
  const db = join(dir, "control.db");
  procs.push(
    spawn(
      "../target/debug/arugula-control",
      [
        ...["--listen", ANY, "--public-url", "http://127.0.0.1:0", "--db", db],
        ...["--github-client-id", "id", "--github-client-secret", "s", "--static-dir", "dist"],
        ...["--github-url", github, "--github-api", github],
      ],
      { stdio: "ignore" },
    ),
  );
  base = `http://127.0.0.1:${await controlPort(db, procs.at(-1))}`;
});

test.afterAll(() => {
  for (const p of procs) p.kill("SIGKILL");
  gh?.close();
  for (const d of dirs) rmSync(d, { recursive: true, force: true });
});

const phase = (page: Page) => page.evaluate(() => window.__arugula?.control?.phase).catch(() => undefined);
const asked = (p: Page) => p.evaluate(() => (window as unknown as { asked: unknown[][] }).asked);
const opened = (p: Page) => p.evaluate(() => (window as unknown as { opened: unknown[][] }).opened);

test("Add passkey ↗ opens the browser on one click in WebKit, and Open again asks again (#381)", async ({ browser }) => {
  const ctx = await browser.newContext({ baseURL: base, serviceWorkers: "block" });
  const page = await ctx.newPage();
  await page.goto("/");
  await page.locator("[data-signin=github]").click();
  await page.locator("[data-stored-codes]").check();
  await page.locator("[data-saved-codes]").click();
  await expect.poll(() => phase(page), { timeout: 20_000 }).toBe("ready");

  await page.evaluate(() => {
    const asked: unknown[][] = [];
    Object.assign(window, {
      __arugulaApp: { name: "Arugula app on test-mac" },
      asked,
      opened: [] as unknown[][],
      __TAURI__: { core: { invoke: async (...a: unknown[]) => void asked.push(a) } },
    });
    window.open = (...a: unknown[]) => ((window as unknown as { opened: unknown[][] }).opened.push(a), null);
    window.__arugula.control!.info.passkeys = true;
  });
  await controlPanel(page, "account");
  await page.locator("[data-add-passkey]").click();
  await expect(page.locator("[data-browser-step-waiting]")).toBeVisible();
  await expect(page.locator("[data-browser-step-open]")).toHaveCount(0);
  expect(await asked(page)).toEqual([["open_outside", { url: `${base}/#account` }]]);
  await page.locator("[data-browser-step-again-open]").click();
  await expect.poll(() => asked(page)).toEqual([
    ["open_outside", { url: `${base}/#account` }],
    ["open_outside", { url: `${base}/#account` }],
  ]);
  expect(await opened(page)).toEqual([]);
  await page.locator("[data-browser-step-cancel]").click();

  // An app from before the command refuses it: the open button stays.
  await page.evaluate(() => {
    Object.assign(window, { __TAURI__: { core: { invoke: async () => Promise.reject("command open_outside not found") } } });
  });
  await page.locator("[data-add-passkey]").click();
  await expect(page.locator("[data-browser-step-open]")).toBeVisible();
  await expect(page.locator("[data-browser-step-waiting]")).toHaveCount(0);
  expect(await opened(page)).toEqual([]);
  await ctx.close();
});
