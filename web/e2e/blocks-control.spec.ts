// M50 (#150): blocks through control. A machine joined to a control that
// has a block domain, with no block flags of its own, shows a port and an
// editor block inside control's page, each on its own origin on control's
// block domain (`b-<site key>.<domain>`). The frame's worker carries the
// block's requests over a Noise channel through control's relay, the
// page's shim carries Vite's hot-reload WebSocket, and control's page signs
// each frame's grant. A browser that can't run the block's worker falls
// back to the machine's own block site, or says there's none.
//
// The block domain is `blocks.localhost`: Chromium sends every
// `*.localhost` name to loopback and counts it as a secure context, so
// control's one listener answers the blocks' origins with no DNS,
// certificate or proxy.

import { spawn, type ChildProcess } from "node:child_process";
import { mkdirSync, mkdtempSync, realpathSync, rmSync, writeFileSync } from "node:fs";
import type { Server } from "node:http";
import type { AddressInfo } from "node:net";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { expect, test, type BrowserContext, type Frame, type Page } from "@playwright/test";
import { createServer, type ViteDevServer } from "vite";
import { fakeGithub } from "../fixtures/fakes.ts";
import { closeContexts, menu, paneEl } from "./helpers";
import type { PaneId } from "../src/shared/proto";
import { ANY, controlPort } from "./ports";

test.afterAll(closeContexts);
test.describe.configure({ mode: "serial" });

const DOMAIN = "blocks.localhost";
let base = "";
let PORT = 0;
const procs: ChildProcess[] = [];
let dir = "";
let gh: Server;
let vite: ViteDevServer | undefined;
let VITE = 0;
let project = "";
let ctx: BrowserContext;
let page: Page;

test.use({ baseURL: async ({}, use) => use(base) });

const files: Record<string, string> = {
  "index.html": `<!doctype html><html><head><title>Dev app</title></head>
<body><h1 id="h">…</h1><script type="module" src="/main.js"></script></body></html>`,
  "label.js": `export const label = "hello";`,
  "main.js": `import { label } from "./label.js";
const h = document.querySelector("#h");
h.textContent = label;
if (import.meta.hot) import.meta.hot.accept("./label.js", (m) => (h.textContent = m.label));`,
};
const write = (name: string, text: string) => writeFileSync(join(project, name), text);

async function up(url: string) {
  for (let i = 0; i < 100; i++) {
    try {
      if ((await fetch(url)).status < 500) return;
    } catch {
      // not yet
    }
    await new Promise((r) => setTimeout(r, 100));
  }
  throw new Error(`${url} didn't come up`);
}

test.beforeAll(async () => {
  // Resolved: VS Code reports the real path.
  dir = realpathSync(mkdtempSync(join(tmpdir(), "arugula-e2e-blocks-control-")));
  project = join(dir, "app");
  mkdirSync(project);
  for (const [name, text] of Object.entries(files)) write(name, text);
  // A dev server whose page sets and reads a cookie, as a session would.
  vite = await createServer({
    root: project,
    configFile: false,
    logLevel: "silent",
    plugins: [
      {
        name: "cookies",
        configureServer(s) {
          s.middlewares.use("/set", (_req, res) => {
            res.setHeader("set-cookie", "sid=s3cret; Path=/; HttpOnly");
            res.end("set");
          });
          s.middlewares.use("/cookie", (req, res) => res.end(`cookie=${req.headers.cookie ?? ""}`));
        },
      },
    ],
  });
  await vite.listen();
  VITE = (vite.httpServer!.address() as AddressInfo).port;

  gh = fakeGithub(0);
  await new Promise((r) => gh.once("listening", r));
  const github = `http://127.0.0.1:${(gh.address() as AddressInfo).port}`;
  const db = join(dir, "control", "control.db");
  mkdirSync(join(dir, "control"));
  procs.push(
    spawn(
      "../target/debug/arugula-control",
      [
        ...["--listen", ANY, "--public-url", "http://127.0.0.1:0", "--db", db, "--block-domain", DOMAIN],
        ...["--github-client-id", "id", "--github-client-secret", "s", "--static-dir", "dist"],
        ...["--github-url", github, "--github-api", github],
      ],
      { stdio: process.env.E2E_CONTROL_LOG ? "inherit" : "ignore" },
    ),
  );
  PORT = await controlPort(db, procs.at(-1));
  base = `http://127.0.0.1:${PORT}`;
  await up(`${base}/control.json`);
});

test.afterAll(async () => {
  await ctx?.close();
  await vite?.close();
  for (const p of procs) p.kill("SIGKILL");
  gh?.close();
  if (dir) rmSync(dir, { recursive: true, force: true });
});

/** The frame of block `b`, once it's loaded from its origin on control's
 * block domain. */
async function frameOf(b: PaneId): Promise<Frame> {
  const want = new RegExp(`^http://b-[0-9a-f]{32}\\.${DOMAIN.replace(".", "\\.")}:${PORT}/`);
  let frame: Frame | null = null;
  await expect
    .poll(async () => {
      const el = await paneEl(page, b).locator("iframe").elementHandle();
      frame = (await el?.contentFrame()) ?? null;
      return want.test(frame?.url() ?? "") ? frame!.url() : null;
    })
    .not.toBeNull();
  return frame!;
}

const blocks = (type: string) => page.evaluate((t) => window.__arugula.client.state!.panes.filter((p) => p.type === t).map((p) => p.id), type);

test("a port and an editor on a machine with no block flags open inside control's page", async ({ browser }) => {
  test.setTimeout(600_000);
  ctx = await browser.newContext();
  page = await ctx.newPage();
  // For the last test: a block opened with `?no-sw` sees no service workers.
  await page.addInitScript(() => {
    if (location.hostname.endsWith(".blocks.localhost") && location.search.includes("no-sw")) delete (Navigator.prototype as { serviceWorker?: unknown }).serviceWorker;
  });
  await page.goto("/");
  await page.locator("[data-signin=github]").click();
  await page.locator("[data-stored-codes]").check();
  await page.locator("[data-saved-codes]").click();

  // A machine of this account, reached only through control's relay, with
  // no --block-listen.
  const state = join(dir, "mac");
  const joining = spawn("../target/debug/arugulad", ["join", base, "--name", "mac", "--state-dir", state], { stdio: ["pipe", "pipe", "ignore"] });
  procs.push(joining);
  const link = await new Promise<string>((res) => {
    let out = "";
    joining.stdout!.on("data", (d) => {
      out += d;
      const m = out.match(/(http\S+#join=[A-Z0-9.-]+)/);
      if (m) res(m[1]);
    });
  });
  const exited = new Promise<number | null>((r) => joining.on("exit", r));
  await page.goto(link);
  await page.locator("[data-approve-join]").click();
  expect(await exited).toBe(0);
  procs.push(
    spawn(
      "../target/debug/arugulad",
      [
        ...["--listen", ANY, "--name", "mac", "--state-dir", state],
        ...["--shell", "bash --norc --noprofile", "--no-manager-env", "--tailscale-socket", "/nonexistent/sock"],
      ],
      // code-server keeps its own logs under XDG_DATA_HOME: not the user's.
      { stdio: "ignore", env: { ...process.env, ARUGULA_WISP_TOKEN_FILE: "/nonexistent", XDG_DATA_HOME: join(dir, "data") } },
    ),
  );
  await expect
    .poll(async () => ((await (await ctx.request.get("/api/directory")).json()) as { daemons: { online: boolean }[] }).daemons.map((d) => d.online), {
      timeout: 20_000,
    })
    .toEqual([true]);
  await page.goto("/");
  await expect.poll(() => page.evaluate(() => !!window.__arugula?.client.e2e && window.__arugula.client.connected), { timeout: 20_000 }).toBe(true);

  // The machine has no block sites of its own, and says blocks show
  // through control (its own page won't offer them).
  await expect
    .poll(() => page.evaluate(async () => (await (await window.__arugula.client.request("GET", "/api/host")).json<{ features: unknown }>()).features), { timeout: 20_000 })
    .toMatchObject({ blocks: false, block_channels: true });
  await page.evaluate(() => window.__arugula.client.loadFeatures());

  // The machine's terminal, and *Open a port…* from its menu.
  await expect.poll(() => blocks("terminal"), { timeout: 20_000 }).toHaveLength(1);
  const [term] = await blocks("terminal");
  await expect(paneEl(page, term)).toBeVisible();
  await menu(page, paneEl(page, term), "Open a port…");
  await page.locator(".prompt input").fill(String(VITE));
  await page.keyboard.press("Enter");
  await expect.poll(() => blocks("browser")).toHaveLength(1);
  const [port] = await blocks("browser");

  // Its own origin on control's block domain, through the frame's worker.
  const view = page.frameLocator(`[data-pane="${port}"] iframe`);
  await expect(view.locator("#h")).toHaveText("hello", { timeout: 30_000 });
  const frame = await frameOf(port);
  expect(await frame.evaluate(() => !!navigator.serviceWorker.controller)).toBe(true);
  expect(await paneEl(page, port).locator("iframe").getAttribute("sandbox")).toBe("allow-scripts allow-forms allow-same-origin");
  // Chrome's Local Network Access, for a direct route from its pages (#849).
  expect(await paneEl(page, port).locator("iframe").getAttribute("allow")).toBe("local-network; local-network-access");
  // It went through control's relay, as this machine does, and says so in
  // the button's words: on the block, and in the host menu.
  await expect(paneEl(page, port).locator("[data-block-route]")).toHaveText("relayed", { timeout: 15_000 });
  await expect(page.locator(".ws-head [data-connection]")).toHaveAttribute("data-connection", "relayed");
  await page.locator(".host-button").click();
  await expect(page.getByRole("menuitem", { name: "Blocks: relayed" })).toBeVisible();
  await page.keyboard.press("Escape");
  // The frame can't read control's page.
  expect(await frame.evaluate(() => {
    try {
      return String(parent.document.title);
    } catch {
      return "blocked";
    }
  })).toBe("blocked");

  // Hot reload: Vite's WebSocket rides the page's own channel.
  await frame.evaluate(() => ((window as unknown as { marker: number }).marker = 42));
  write("label.js", `export const label = "hot";`);
  await expect(view.locator("#h")).toHaveText("hot", { timeout: 15_000 });
  expect(await frame.evaluate(() => (window as unknown as { marker?: number }).marker)).toBe(42);

  // The worker keeps the block's cookies and sends them on.
  expect(await frame.evaluate(async () => (await fetch("/set")).text())).toBe("set");
  expect(await frame.evaluate(async () => (await fetch("/cookie")).text())).toBe("cookie=sid=s3cret");

  // A hard reload of the frame comes back through the worker, and says
  // which way again.
  await frame.evaluate(() => location.reload());
  await expect(view.locator("#h")).toHaveText("hot", { timeout: 15_000 });
  await expect(paneEl(page, port).locator("[data-block-route]")).toHaveText("relayed");

  // An editor on the app's folder, the same way.
  await page.evaluate((path) => window.__arugula.client.api("/api/blocks", { type: "editor", config: { path: `${path}/main.js` }, from_pane: null }), project);
  await expect.poll(() => blocks("editor"), { timeout: 20_000 }).toHaveLength(1);
  const [ed] = await blocks("editor");
  await page.evaluate((b) => window.__arugula.client.setActive(b), ed);
  // The first run downloads code-server (about 230 MB).
  await expect
    .poll(() => page.evaluate((b) => (window.__arugula.client.blocks.get(b)?.state as { server?: unknown } | null)?.server, ed), { timeout: 540_000 })
    .toEqual({ is: "running" });
  const vs = page.frameLocator(`[data-pane="${ed}"] iframe`);
  await expect(vs.locator(".monaco-workbench")).toBeVisible({ timeout: 60_000 });
  const edFrame = await frameOf(ed);
  // Its file, opened on the block origin's remote.
  await expect(vs.locator(".monaco-editor .view-lines", { hasText: "import.meta.hot" }).first()).toBeVisible({ timeout: 60_000 });
  expect(new URL(edFrame.url()).searchParams.get("payload")).toContain(`vscode-remote://${new URL(edFrame.url()).host}/`);
});

test("a browser that can't run the block's worker is told so", async () => {
  // Another port block is another origin, with no worker yet; this page
  // hides service workers from it (the init script, below), as a browser
  // that has none in a cross-site frame would. The machine has no block
  // site of its own to fall back to, so the block says so.
  const [term] = await blocks("terminal");
  await page.evaluate(
    ({ port, term }) => window.__arugula.client.api("/api/blocks", { type: "browser", config: { port, path: "/?no-sw" }, split: term, host: null, local: false }),
    { port: VITE, term },
  );
  await expect.poll(() => blocks("browser")).toHaveLength(2);
  const [, second] = await blocks("browser");
  await page.evaluate((b) => window.__arugula.client.setActive(b), second);
  const card = paneEl(page, second).locator("[data-block-nowhere]");
  await expect(card).toContainText("can't show this block through Arugula control", { timeout: 30_000 });
  await expect(card).toContainText("no service workers");
  await expect(card).toContainText("no block site of its own");
  // It never connected, so it says no route.
  await expect(paneEl(page, second).locator("[data-block-route]")).toHaveCount(0);
});
