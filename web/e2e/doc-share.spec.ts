// #1066: sharing a Doc with a link, against a real control and a daemon
// joined to it. The link opens in a fresh browser context (no cookies, no
// session) at the viewer control serves: `docs.<block domain>` when control
// has a block domain, `/d/<id>` on its own origin when it hasn't. Text, a
// chart, a drawing and a picture show, light and dark, on a desktop and a
// Pixel 7; an Update changes what the same URL shows; Unshare breaks it; a
// part that was tampered with, a wrong key or none says only that the link
// is broken; a service worker can't be registered. The block's menu, and
// the account's Shared Docs…, are the owner's way in.
//
// The block domain is `blocks.localhost`: Chromium sends every `*.localhost`
// name to loopback, so control's one listener answers `docs.blocks.localhost`
// with no DNS or certificate.

import { spawn, type ChildProcess } from "node:child_process";
import { randomBytes } from "node:crypto";
import { mkdirSync, mkdtempSync, readdirSync, readFileSync, rmSync } from "node:fs";
import type { AddressInfo } from "node:net";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { expect, test, type Browser, type BrowserContext, type Page } from "@playwright/test";
import { Device } from "../fixtures/device.ts";
import { fakeGithub } from "../fixtures/fakes.ts";
import { hex } from "../src/shared/docs/colours";
import { example } from "./doc-examples";
import { newDoc, png, putAsset, fill, seeEmbeds, type Post } from "./doc-steps";
import { closeContexts, menu, reset, panes, paneEl } from "./helpers";
import { labs } from "./labs";
import { pixel7 } from "./phones";
import { ANY, controlPort, daemonPort } from "./ports";
import "./local-token";

test.afterAll(closeContexts);

const DOMAIN = "blocks.localhost";
const MARK = "ZEBRA-QUARTZ-4471";
const SHOTS = process.env.DOC_LINK_SHOTS;

interface Rig {
  control: string;
  mini: string;
  state: string;
  /** The account's headless device; none for a rig the page signed in. */
  me: Device;
  post: Post & ((path: string, data: unknown) => Promise<{ block?: number; url?: string }>);
  /** Where a link is opened: the viewer's base. */
  viewer: string;
  /** Where control's database and its logs' files are. */
  dbDir: string;
  daemon: ChildProcess;
  dispose: () => Promise<void>;
}

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

/** A control (with a block domain or without), a machine joined to it,
 * and the owner's headless device. */
async function rig(blockDomain: string | null, browser?: Page): Promise<Rig> {
  const procs: ChildProcess[] = [];
  const dirs: string[] = [];
  const temp = (what: string) => {
    const d = mkdtempSync(join(tmpdir(), `arugula-e2e-docshare-${what}-`));
    dirs.push(d);
    return d;
  };
  const gh = fakeGithub(0);
  await new Promise((r) => gh.once("listening", r));
  const github = `http://127.0.0.1:${(gh.address() as AddressInfo).port}`;
  const db = join(temp("db"), "control.db");
  mkdirSync(join(db, ".."), { recursive: true });
  procs.push(
    spawn(
      "../target/debug/arugula-control",
      [
        ...["--listen", ANY, "--public-url", "http://127.0.0.1:0", "--db", db, ...(blockDomain ? ["--block-domain", blockDomain] : [])],
        ...["--github-client-id", "id", "--github-client-secret", "s", "--static-dir", "dist"],
        ...["--github-url", github, "--github-api", github],
        // The database is on the machine's disk, which a CI pod shares
        // with its node, and other pods can fill that past the guard's 80%.
        "--no-disk-guard",
      ],
      { stdio: process.env.E2E_CONTROL_LOG ? "inherit" : "ignore" },
    ),
  );
  const port = await controlPort(db, procs.at(-1));
  const control = `http://127.0.0.1:${port}`;
  await up(`${control}/control.json`);

  // The account is a headless device's, or (with `browser`) the one the page
  // signs in as, which approves the machine as a person would.
  const me = browser ? undefined : await Device.signIn({ control, login: "jake" });
  if (browser) {
    await browser.goto(`${control}/`);
    await browser.locator("[data-signin=github]").click();
    await browser.locator("[data-stored-codes]").check();
    await browser.locator("[data-saved-codes]").click();
  }
  const state = labs(temp("mini"));
  const joining = spawn("../target/debug/arugulad", ["join", control, "--name", "mini", "--state-dir", state, ...(me ? ["--account", me.keys.id] : [])], {
    stdio: ["ignore", "pipe", "ignore"],
  });
  procs.push(joining);
  const code = await new Promise<string>((res) => {
    let out = "";
    joining.stdout!.on("data", (d) => {
      out += d;
      const m = out.match(me ? /#join=([A-Z0-9.-]+)\s/ : /(http\S+#join=[A-Z0-9.-]+)/);
      if (m) res(m[1]);
    });
  });
  const exited = new Promise<number | null>((r) => joining.on("exit", r));
  if (me) await me.approveJoin(code);
  else {
    await browser!.goto(code);
    await browser!.locator("[data-approve-join]").click();
  }
  expect(await exited).toBe(0);
  const d = spawn(
    "../target/debug/arugulad",
    [
      ...["--listen", ANY, "--name", "mini", "--state-dir", state],
      ...["--shell", "bash --norc --noprofile", "--no-manager-env", "--tailscale-socket", "/nonexistent/sock", "--no-claude-ide"],
    ],
    { stdio: "ignore" },
  );
  procs.push(d);
  const mini = `http://127.0.0.1:${await daemonPort(state, d)}`;
  await up(`${mini}/api/host`);
  if (me) await me.waitOnline("mini");
  else {
    await expect
      .poll(async () => ((await (await browser!.context().request.get(`${control}/api/directory`)).json()) as { daemons: { online: boolean }[] }).daemons.map((x) => x.online), { timeout: 30_000 })
      .toEqual([true]);
  }

  const post: Rig["post"] = async (path, data) => {
    const r = await fetch(mini + path, { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify(data) });
    const text = await r.text();
    if (!r.ok) throw new Error(`${path}: ${r.status} ${text}`);
    return JSON.parse(text);
  };
  const viewer = blockDomain ? `http://docs.${blockDomain}:${port}` : `${control}/d`;
  return {
    control,
    mini,
    state,
    me: me as Device,
    post,
    daemon: d,
    viewer,
    dbDir: dirname(db),
    dispose: async () => {
      for (const p of procs) p.kill("SIGKILL");
      gh.close();
      for (const dir of dirs) rmSync(dir, { recursive: true, force: true });
    },
  };
}

/** A Doc with a title, text and a picture (s1), a chart (s2), and a drawing
 * (s3) left for later. */
async function doc(r: Rig): Promise<number> {
  const [{ id: term }] = (await (await fetch(`${r.mini}/api/panes`)).json()) as { id: number }[];
  const d = await newDoc(r.post, term, `Report ${MARK}`);
  await putAsset(r.post, d, "red.png", png(40, 24));
  // A picture no section shows, big enough that the snapshot is two parts.
  await putAsset(r.post, d, "pad.png", Buffer.concat([png(1, 1), randomBytes(1_300_000)]));
  await fill(r.post, d, "s1", `Findings ${MARK}\n\n![a red square](assets/red.png)`);
  await fill(r.post, d, "s2", example("bar-chart.md"));
  return d;
}

const share = (r: Rig, d: number, method = "doc_share") => r.post(`/api/blocks/${d}/call/${method}`, {}).then((v) => v.url!);

/** The link in a fresh context: nothing carried over, so what shows is
 * what the link gives anyone. */
async function visitor(browser: Browser, opts: Parameters<Browser["newContext"]>[0] = {}): Promise<{ ctx: BrowserContext; page: Page; seen: string[]; violations: string[] }> {
  const ctx = await browser.newContext({ storageState: { cookies: [], origins: [] }, ...opts });
  const page = await ctx.newPage();
  const seen: string[] = [];
  const violations: string[] = [];
  page.on("request", (q) => seen.push(q.url()));
  page.on("console", (m) => /Content Security Policy|Refused to/i.test(m.text()) && violations.push(m.text()));
  return { ctx, page, seen, violations };
}

/** What the page draws: its title, text, picture, chart and (when filled) drawing. */
async function seeTheDoc(page: Page, drawing = false) {
  const block = page.locator("[data-doc-block]");
  await expect(block.locator(".doc-title")).toHaveText(`Report ${MARK}`, { timeout: 15_000 });
  await expect(block.locator('[data-section="s1"]')).toContainText(`Findings ${MARK}`);
  const img = block.locator('[data-section="s1"] img.doc-image');
  await expect(img).toBeVisible();
  await expect.poll(() => img.evaluate((i: HTMLImageElement) => i.complete && i.naturalWidth)).toBe(40);
  await expect(block.locator('[data-section="s2"] svg.chart rect[data-bar]')).toHaveCount(3);
  if (drawing) {
    const d = block.locator('[data-section="s3"] img.doc-drawing').first();
    await expect(d).toBeVisible();
    await expect.poll(() => d.evaluate((i: HTMLImageElement) => i.complete && i.naturalWidth > 0)).toBe(true);
  }
}

const noticeOf = (page: Page, kind: string) => page.locator(`[data-doc-notice="${kind}"]`);

for (const mode of [DOMAIN, null] as const) {
  test.describe(mode ? "on the docs host of a control with a block domain" : "on /d/ of a control with none", () => {
    test.describe.configure({ mode: "serial" });
    let r: Rig;
    test.beforeAll(async () => {
      test.setTimeout(120_000);
      r = await rig(mode);
    });
    test.afterAll(async () => {
      await r?.dispose();
    });

    test("the link opens in a fresh context with no session: text, picture, chart and drawing, light and dark", async ({ browser }) => {
      const d = await doc(r);
      // An old drawing in hex, and one in the named colours (#256).
      await fill(r.post, d, "s3", `${example("drawing.md")}\n${example("named-drawing.md")}`);
      const url = await share(r, d);
      expect(url).toMatch(new RegExp(`^${r.viewer.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")}/[0-9a-f]{32}#k=[A-Za-z0-9_-]{43}$`));
      const origin = new URL(url).origin;

      for (const scheme of ["light", "dark"] as const) {
        for (const [name, opts] of [["desktop", {}], ["pixel7", pixel7]] as const) {
          const v = await visitor(browser, { colorScheme: scheme, ...opts });
          await v.page.goto(url);
          await seeTheDoc(v.page, true);
          await expect(v.page.locator("[data-doc-progress]")).toContainText("3 of 3 sections written");
          // The page follows the system's colors.
          const bg = await v.page.evaluate(() => getComputedStyle(document.body).backgroundColor);
          expect(bg, scheme).toBe(scheme === "light" ? "rgb(239, 241, 245)" : "rgb(17, 17, 27)");
          // The Doc wears its own paper in each (#253).
          const paper = await v.page.evaluate(() => getComputedStyle(document.querySelector(".doc")!).backgroundColor);
          expect(paper, scheme).toBe(scheme === "light" ? "rgb(248, 246, 240)" : "rgb(26, 27, 24)");
          // The named drawing in this scheme's colours, on that paper; the hex one on its light backing (#256).
          const [old, named] = await v.page.locator('[data-section="s3"] img.doc-drawing').evaluateAll((is) =>
            is.map((i) => ({ src: decodeURIComponent((i as HTMLImageElement).src), named: i.hasAttribute("data-named"), bg: getComputedStyle(i).backgroundColor })),
          );
          expect(old, scheme).toMatchObject({ named: false, bg: "rgb(248, 246, 240)" });
          expect(old.src).toContain('fill="#e8f0fe"');
          expect(named, scheme).toMatchObject({ named: true, bg: "rgba(0, 0, 0, 0)" });
          expect(named.src).toContain(`fill="${hex("leaf-soft", scheme)}" stroke="${hex("leaf", scheme)}"`);
          // Fresh: no cookie came or was set, and it asked only its own origin
          // for the page, its files and its parts.
          expect(await v.ctx.cookies(), `${name} ${scheme}`).toEqual([]);
          expect(v.seen.every((u) => u.startsWith(origin)), v.seen.join("\n")).toBe(true);
          const paths = v.seen.map((u) => new URL(u).pathname);
          const link = new URL(url).pathname;
          expect(paths.filter((p) => p.startsWith("/api") || p.includes("/ws") || p.endsWith(".webmanifest")), paths.join("\n")).toEqual([]);
          expect(paths.filter((p) => p.includes("/data/")).every((p) => p.startsWith(`${link}/data/`)), paths.join("\n")).toBe(true);
          // The key stayed in the fragment.
          expect(v.seen.some((u) => u.includes("#k=") || u.includes(url.split("#k=")[1]!))).toBe(false);
          expect(v.violations, "the page's CSP refused nothing").toEqual([]);
          if (SHOTS && mode) {
            mkdirSync(SHOTS, { recursive: true });
            // The page scrolls inside itself, so "full page" is a viewport as tall as the Doc.
            const tall = await v.page.evaluate(() => (document.querySelector(".doc") as HTMLElement).scrollHeight);
            await v.page.setViewportSize({ width: v.page.viewportSize()!.width, height: tall + 40 });
            await v.page.screenshot({ path: join(SHOTS, `viewer-${name}-${scheme}.png`) });
          }
          await v.ctx.close();
        }
      }
    });

    // #254: callouts and stat tiles, through the link, light and dark.
    test("the link draws callouts and stat tiles in either scheme", async ({ browser }) => {
      const d = await doc(r);
      await fill(r.post, d, "s3", `> [!TIP]\n> Share it.\n\n> [!CAUTION]\n> Not the key.\n\n${example("stats.md")}`);
      const url = await share(r, d);
      for (const scheme of ["light", "dark"] as const) {
        const v = await visitor(browser, { colorScheme: scheme });
        await v.page.goto(url);
        await seeTheDoc(v.page);
        const s3 = v.page.locator('[data-section="s3"]');
        await expect(s3.locator("[data-callout]")).toHaveCount(3);
        await expect(s3.locator("[data-callout=tip] .md-callout-title")).toHaveText("Tip");
        await expect(s3.locator(".doc-stat")).toHaveCount(3);
        await expect(s3.locator(".doc-stat").first()).toContainText("1,284");
        expect(v.violations, "the page's CSP refused nothing").toEqual([]);
        if (SHOTS && mode) {
          mkdirSync(SHOTS, { recursive: true });
          await s3.screenshot({ path: join(SHOTS, `viewer-callouts-${scheme}.png`) });
        }
        await v.ctx.close();
      }
    });

    test("the Doc's drawing and its script never run on the page", async ({ browser }) => {
      const d = await doc(r);
      await fill(r.post, d, "s3", example("script-drawing.md"));
      const v = await visitor(browser);
      await v.page.goto(await share(r, d));
      await seeTheDoc(v.page, true);
      expect(await v.page.evaluate(() => (window as unknown as { __pwned?: number }).__pwned)).toBeUndefined();
      await v.ctx.close();
    });

    test("an Update shows the new section at the same URL on reload, and Unshare breaks the link", async ({ browser }) => {
      const d = await doc(r);
      const url = await share(r, d);
      const v = await visitor(browser);
      await v.page.goto(url);
      await seeTheDoc(v.page);
      await expect(v.page.locator('[data-section="s3"]')).toHaveAttribute("data-pending", "");

      await fill(r.post, d, "s3", `A late section ${MARK}-LATE`);
      // Not yet: a link shows the Doc as it was shared.
      await v.page.reload();
      await expect(v.page.locator('[data-section="s3"]')).toHaveAttribute("data-pending", "");
      expect(await share(r, d, "doc_share_update")).toBe(url);
      await v.page.reload();
      await expect(v.page.locator('[data-section="s3"]')).toContainText(`${MARK}-LATE`);
      await expect(v.page.locator('[data-section="s3"]')).not.toHaveAttribute("data-pending", "");

      // Control lists it, with the size and an open count, and knows no title.
      const links = await r.me.api<{ id: string; size: number; opens: number }[]>("/api/doc-links");
      expect(links.some((l) => url.includes(l.id) && l.size > 0)).toBe(true);
      expect(JSON.stringify(links)).not.toContain(MARK);

      await r.post(`/api/blocks/${d}/call/doc_unshare`, {});
      await v.page.reload();
      await expect(noticeOf(v.page, "gone")).toContainText("This Doc isn't shared any more");
      await expect(v.page.locator("[data-doc-block]")).toHaveCount(0);
      await expect(v.page.locator("body")).not.toContainText(MARK);
      expect((await r.me.api<{ id: string }[]>("/api/doc-links")).some((l) => url.includes(l.id))).toBe(false);
      await v.ctx.close();
    });

    test("a tampered part, a wrong key, a missing key or a bad link says only that it is broken", async ({ browser }) => {
      const d = await doc(r);
      const url = await share(r, d);
      const broken = async (to: string, route?: (p: Page) => Promise<void>) => {
        const v = await visitor(browser);
        await route?.(v.page);
        await v.page.goto(to);
        await expect(noticeOf(v.page, "broken")).toContainText("This link is broken");
        // Nothing of the Doc: no block, no title, no text, no picture, no partial render.
        await expect(v.page.locator("[data-doc-block], .doc-title, img")).toHaveCount(0);
        await expect(v.page.locator("body")).not.toContainText(MARK);
        await expect(v.page.locator("body")).not.toContainText("Findings");
        await v.ctx.close();
      };
      // A flipped bit in the first part and in the last.
      for (const n of [0, 1]) {
        await broken(url, async (page) => {
          await page.route(`**/data/${n}?*`, async (route) => {
            const res = await route.fetch();
            const body = Buffer.from(await res.body());
            body[body.length - 1]! ^= 1;
            await route.fulfill({ response: res, body });
          });
        });
      }
      const [base, key] = url.split("#k=");
      // The first character: the last one's low bits are base64's padding,
      // so changing it can leave the key as it was (one key in sixteen).
      const other = (key!.startsWith("A") ? "B" : "A") + key!.slice(1);
      await broken(`${base}#k=${other}`);
      await broken(base!);
      await broken(`${base}#k=short`);
      // Another link's parts under this one's key.
      const d2 = await doc(r);
      const url2 = await share(r, d2);
      const id2 = new URL(url2).pathname.split("/").at(-1)!;
      await broken(url, async (page) => {
        await page.route("**/data/*", async (route) => route.continue({ url: route.request().url().replace(/[0-9a-f]{32}/, id2) }));
      });
    });

    test("a service worker can't be registered on the page", async ({ browser }) => {
      const d = await doc(r);
      const v = await visitor(browser);
      await v.page.goto(await share(r, d));
      await seeTheDoc(v.page);
      const why = await v.page.evaluate(() =>
        navigator.serviceWorker.register("/sw.js").then(
          () => "registered",
          (e: Error) => e.name,
        ),
      );
      expect(why).not.toBe("registered");
      expect(await v.page.evaluate(() => navigator.serviceWorker.getRegistrations().then((rs) => rs.length))).toBe(0);
      await v.ctx.close();
    });

    test("a link whose Update lands mid-load is read again from its start", async ({ browser }) => {
      const d = await doc(r);
      const url = await share(r, d);
      await fill(r.post, d, "s3", `Landed ${MARK}-MID`);
      const v = await visitor(browser);
      let updated = false;
      // The first part request is held until the Update is served: the page's
      // `?v=1` then 404s, and it starts over from `meta`.
      await v.page.route("**/data/0?v=1", async (route) => {
        if (!updated) {
          updated = true;
          await share(r, d, "doc_share_update");
        }
        await route.continue();
      });
      await v.page.goto(url);
      await expect(v.page.locator('[data-section="s3"]')).toContainText(`${MARK}-MID`, { timeout: 15_000 });
      expect(updated).toBe(true);
      await v.ctx.close();
    });

    test("the snapshot is ciphertext on control: its database holds no word of the Doc", async () => {
      const d = await doc(r);
      await share(r, d);
      // Its parts are rows of control.db (and its write-ahead log, until it is folded in).
      const files = readdirSync(r.dbDir).filter((f) => f.startsWith("control.db"));
      expect(files).toContain("control.db");
      for (const f of files) {
        const bytes = readFileSync(join(r.dbDir, f));
        for (const word of [MARK, "Findings", "Report", "red.png", "Sales by month"]) expect(bytes.includes(word), `${f} holds ${word}`).toBe(false);
      }
    });
  });
}

/** Right-click the Doc block until its menu has `item` (the block's state,
 * and whether it can be shared, arrive just after the page does). */
async function menuHas(page: Page, block: ReturnType<Page["locator"]>, item: string) {
  await expect
    .poll(async () => {
      await block.click({ button: "right", position: { x: 60, y: 60 } });
      await page.getByRole("menu").waitFor();
      const has = await page.getByRole("menuitem", { name: item, exact: true }).count();
      if (!has) await page.keyboard.press("Escape");
      return has;
    })
    .toBe(1);
}

test.describe("the owner's menus", () => {
  test.describe.configure({ mode: "serial" });
  let r: Rig;
  test.beforeAll(async () => {
    test.setTimeout(120_000);
    r = await rig(DOMAIN);
  });
  test.afterAll(async () => {
    await r?.dispose();
  });

  test("a Doc block's menu shares, copies, updates and unshares", async ({ page }) => {
    const d = await doc(r);
    await page.goto(r.mini);
    const block = page.locator(`[data-doc-block="${d}"]`);
    await expect(block.locator(".doc-title")).toBeVisible({ timeout: 20_000 });

    // Not shared: Share link…, and none of the others.
    await menuHas(page, block, "Share link…");
    for (const gone of ["Copy link", "Update shared copy", "Unshare"]) await expect(page.getByRole("menuitem", { name: gone, exact: true })).toHaveCount(0);
    await page.getByRole("menuitem", { name: "Share link…" }).click();
    const dialog = page.locator("[data-doc-link-dialog]");
    await expect(dialog).toBeVisible();
    const url = (await dialog.locator("[data-doc-link-url]").textContent())!;
    expect(url).toMatch(new RegExp(`^${r.viewer}/[0-9a-f]{32}#k=[A-Za-z0-9_-]{43}$`));
    if (SHOTS) {
      mkdirSync(SHOTS, { recursive: true });
      await page.screenshot({ path: join(SHOTS, "share-dialog.png") });
    }
    await dialog.getByRole("button", { name: "Done" }).click();
    await expect(dialog).toHaveCount(0);
    // The state everyone sees says it's shared, and not where.
    await expect.poll(() => page.evaluate((id) => JSON.stringify(window.__arugula.client.blocks.get(id)?.state), d)).toContain('"shared":{');
    expect(await page.evaluate((id) => JSON.stringify(window.__arugula.client.blocks.get(id)?.state), d)).not.toContain("#k=");

    // Shared: the other three.
    await menuHas(page, block, "Unshare");
    await expect(page.getByRole("menuitem", { name: "Copy link", exact: true })).toBeVisible();
    await expect(page.getByRole("menuitem", { name: "Share link…" })).toHaveCount(0);
    if (SHOTS) await page.screenshot({ path: join(SHOTS, "block-menu-shared.png") });
    await page.keyboard.press("Escape");

    // Update says what it does, and Cancel changes nothing.
    const meta = async () => (await (await fetch(`${url.split("#")[0]}/data/meta`)).json()) as { ver: number };
    expect((await meta()).ver).toBe(1);
    await fill(r.post, d, "s3", `Menu update ${MARK}`);
    await menuHas(page, block, "Update shared copy");
    await page.getByRole("menuitem", { name: "Update shared copy" }).click();
    const update = page.locator("[data-doc-update-dialog]");
    await expect(update).toContainText("Update changes what the link shows. To take something back, Unshare.");
    await update.getByRole("button", { name: "Cancel" }).click();
    expect((await meta()).ver).toBe(1);
    await menuHas(page, block, "Update shared copy");
    await page.getByRole("menuitem", { name: "Update shared copy" }).click();
    await page.locator("[data-doc-update-go]").click();
    await expect.poll(async () => (await meta()).ver).toBe(2);

    // Copy link puts the same URL on the clipboard.
    await page.context().grantPermissions(["clipboard-read", "clipboard-write"], { origin: r.mini });
    await menuHas(page, block, "Copy link");
    await page.getByRole("menuitem", { name: "Copy link" }).click();
    await expect.poll(() => page.evaluate(() => navigator.clipboard.readText())).toBe(url);

    // Unshare asks, then the link is gone and the menu offers Share again.
    await menuHas(page, block, "Unshare");
    await page.getByRole("menuitem", { name: "Unshare", exact: true }).click();
    await expect(page.locator("[data-confirm-dialog]")).toBeVisible();
    await page.locator("[data-cancel-remove]").click();
    expect((await fetch(`${url.split("#")[0]}/data/meta`)).status).toBe(200);
    await menuHas(page, block, "Unshare");
    await page.getByRole("menuitem", { name: "Unshare", exact: true }).click();
    await expect(page.locator("[data-confirm-remove]")).toBeEnabled();
    await page.locator("[data-confirm-remove]").click();
    await expect.poll(async () => (await fetch(`${url.split("#")[0]}/data/meta`)).status).toBe(404);
    await menuHas(page, block, "Share link…");
  });

  test("a shared Doc whose pane closed is on the Docs page and reopens with its share menu; Delete says its link stops working, unshares and removes it", async ({ page }) => {
    const d = await doc(r);
    const url = await share(r, d);
    const meta = async () => (await fetch(`${url.split("#")[0]}/data/meta`)).status;
    await page.goto(r.mini);
    const first = page.locator(`[data-doc-block="${d}"]`);
    await expect(first.locator(".doc-title")).toBeVisible({ timeout: 20_000 });
    await menuHas(page, first, "Unshare");
    await page.keyboard.press("Escape");

    // Close the pane, as a person does; the Doc stays, and is shared still.
    await menu(page, first, "Close pane");
    await expect(first).toHaveCount(0);
    await page.locator(".ws-rail [data-open-docs]").click();
    const listed = async () => ((await (await fetch(`${r.mini}/api/docs`)).json()) as { id: string; open?: number | null; shared?: object | null }[]);
    const id = (await listed()).find((x) => x.shared && x.open == null)!.id;
    const mine = page.locator(`[data-doc="${id}"]`);
    await expect(mine.locator("[data-doc-shared]")).toBeVisible();
    await mine.locator("[data-open-doc]").click();

    // Reopened: its content, and its share menu is back, not Share link….
    await expect(page.locator("[data-docs-page]")).toHaveCount(0);
    const now = await page.evaluate(() => window.__arugula.client.active()!);
    expect(now).not.toBe(d);
    const block = page.locator(`[data-doc-block="${now}"]`);
    await expect(block.locator(".doc-title")).toHaveText(`Report ${MARK}`);
    await expect(block.locator('[data-section="s1"]')).toContainText(`Findings ${MARK}`);
    await menuHas(page, block, "Update shared copy");
    await expect(page.getByRole("menuitem", { name: "Share link…" })).toHaveCount(0);
    await page.keyboard.press("Escape");

    // Delete warns about the link, then unshares it at control and removes the Doc.
    await page.locator(".ws-rail [data-open-docs]").click();
    await page.locator(`[data-doc="${id}"] [data-delete-doc]`).click();
    await expect(page.locator("[data-delete-shared]")).toHaveText("Its link stops working.");
    expect(await meta()).toBe(200);
    await page.locator("[data-confirm-remove]").click();
    await expect(page.locator(`[data-doc="${id}"]`)).toHaveCount(0);
    await expect.poll(meta).toBe(404);
    expect((await listed()).some((x) => x.id === id)).toBe(false);
    await expect(block).toHaveCount(0);
  });

  test("an agent on the owner's CLI can't share, update, unshare or read a link back", async () => {
    const d = await doc(r);
    await share(r, d);
    for (const method of ["doc_share", "doc_share_update", "doc_unshare", "doc_shares"]) {
      const res = await fetch(`${r.mini}/api/blocks/${d}/call/${method}`, { method: "POST", headers: { "content-type": "application/json", "x-arugula-agent": "1" }, body: "{}" });
      expect(res.status, method).toBe(403);
      expect(await res.text()).toContain("ask with the doc tool's share");
    }
    const id = (await r.me.api<{ id: string }[]>("/api/doc-links")).at(-1)!.id;
    for (const method of ["GET", "DELETE"]) {
      const res = await fetch(`${r.mini}/api/doc-links/${id}`, { method, headers: { "x-arugula-agent": "1" } });
      expect(res.status, method).toBe(403);
    }
    // The owner's own call reads it back; it is still shared.
    expect(((await (await fetch(`${r.mini}/api/doc-links/${id}`)).json()) as { url: string }).url).toContain(`/${id}#k=`);
  });
});

test("on a machine that hasn't joined control, Share link… is disabled and says why", async ({ page }) => {
  await reset(page);
  const [term] = await panes(page);
  const post: Post = async (path, data) => {
    const res = await page.request.post(path, { data });
    expect(res.ok(), path).toBe(true);
    return res.json();
  };
  const d = await newDoc(post, term);
  const block = page.locator(`[data-doc-block="${d}"]`);
  await expect(block).toBeVisible({ timeout: 15_000 });
  await expect
    .poll(async () => {
      await block.click({ button: "right", position: { x: 60, y: 60 } });
      await page.getByRole("menu").waitFor();
      const item = page.getByRole("menuitem", { name: "Share link…" });
      const off = (await item.count()) === 1 && (await item.isDisabled());
      if (!off) await page.keyboard.press("Escape");
      return off;
    })
    .toBe(true);
  await expect(page.getByRole("menuitem", { name: "Share link…" })).toHaveAttribute("title", "this machine hasn't joined control: sharing needs control");
  await page.keyboard.press("Escape");
  void menu;
  void paneEl;
});

test.describe("Shared Docs… in the account menu", () => {
  test.describe.configure({ mode: "serial" });
  let r: Rig;
  let page: Page;
  test.beforeAll(async ({ browser }) => {
    test.setTimeout(180_000);
    const ctx = await browser.newContext({ storageState: { cookies: [], origins: [] } });
    page = await ctx.newPage();
    r = await rig(DOMAIN, page);
  });
  test.afterAll(async () => {
    await r?.dispose();
  });

  /** The control page, with the machine connected and its flags known. */
  async function onControl() {
    await page.goto(`${r.control}/`);
    await expect.poll(() => page.evaluate(() => !!window.__arugula?.client.e2e && window.__arugula.client.connected), { timeout: 30_000 }).toBe(true);
    // Docs have no flag (native #204): what waits is the machine's features.
    await expect.poll(() => page.evaluate(() => window.__arugula.client.features !== null), { timeout: 20_000 }).toBe(true);
  }
  const open = async () => {
    await page.locator("[data-account-menu]").click();
    await page.getByRole("menuitem", { name: "Shared Docs…" }).click();
    await expect(page.locator("[data-shared-docs]")).toBeVisible();
  };

  test("lists the account's links, copies one through its machine and unshares it", async () => {
    const d = await doc(r);
    const url = await share(r, d);
    await page.context().grantPermissions(["clipboard-read", "clipboard-write"], { origin: r.control });
    await onControl();
    await open();
    const id = new URL(url).pathname.split("/").at(-1)!;
    const row = page.locator(`[data-shared-doc="${id}"]`);
    await expect(row).toBeVisible();
    await expect(row).toContainText("mini");
    await expect(row).toContainText(/\d+(\.\d)? (KB|MB)/);
    await expect(row).toContainText("opened");
    // Control knows no title or text: the row's title comes from its
    // machine (UI audit #174), and control's own list has none of it.
    await expect(row.locator("[data-shared-doc-title]")).toHaveText(`Report ${MARK}`);
    const listed = await page.evaluate(async () => (await fetch("/api/doc-links", { credentials: "same-origin" })).text());
    expect(listed).not.toContain(MARK);
    if (SHOTS) await page.screenshot({ path: join(SHOTS, "shared-docs.png") });

    await expect(row.locator("[data-copy-doc-link]")).toBeEnabled();
    await row.locator("[data-copy-doc-link]").click();
    await expect.poll(() => page.evaluate(() => navigator.clipboard.readText())).toBe(url);

    await row.locator("[data-unshare-doc]").click();
    await page.locator("[data-confirm-remove]").click();
    await expect(row).toHaveCount(0);
    expect((await fetch(`${url.split("#")[0]}/data/meta`)).status).toBe(404);
    // Through its machine: the Doc's block says it isn't shared.
    await expect.poll(() => page.evaluate((b) => JSON.stringify(window.__arugula.client.blocks.get(b)?.state?.["shared"] ?? null), d)).toBe("null");
  });

  test("Open on a row opens the Doc on its machine, and lands on its pane", async () => {
    const d = await doc(r);
    const url = await share(r, d);
    const id = new URL(url).pathname.split("/").at(-1)!;
    // Closed on its machine: the Doc is in its library, in no pane.
    await r.post(`/api/panes/${d}/close`, {});
    await onControl();
    await open();
    const row = page.locator(`[data-shared-doc="${id}"]`);
    await expect(row.locator("[data-open-shared-doc]")).toBeEnabled();
    await row.locator("[data-open-shared-doc]").click();

    // The panel is gone and the pane in front is the Doc's block, with its title.
    await expect(page.locator("[data-shared-docs]")).toHaveCount(0);
    await expect.poll(() => page.evaluate(() => window.__arugula.client.info(window.__arugula.client.active() ?? -1)?.type)).toBe("doc");
    const pane = await page.evaluate(() => window.__arugula.client.active()!);
    expect(pane).not.toBe(d);
    const block = page.locator(`[data-doc-block="${pane}"]`);
    await expect(block.locator(".doc-title")).toHaveText(`Report ${MARK}`);
    await expect(block.locator('[data-section="s1"]')).toContainText(`Findings ${MARK}`);
    const library = (await (await fetch(`${r.mini}/api/docs`)).json()) as { open?: number | null; shared?: object | null }[];
    expect(library.filter((x) => x.open === pane && x.shared)).toHaveLength(1);
    // The next test goes on from the panel, as the last one left it.
    await open();
  });

  test("with its machine offline Copy link is off, saying so, and Unshare goes through control", async () => {
    const d = await doc(r);
    const url = await share(r, d);
    const id = new URL(url).pathname.split("/").at(-1)!;
    r.daemon.kill("SIGKILL");
    await expect
      .poll(async () => ((await (await page.context().request.get(`${r.control}/api/directory`)).json()) as { daemons: { online: boolean }[] }).daemons.map((x) => x.online), { timeout: 60_000 })
      .toEqual([false]);
    // The page the last test left open, on its machine: control's word that it
    // is offline reaches it on a refresh. (A page loaded now would wait on the
    // machine and show no menu, so the panel is opened by its event.)
    await page.evaluate(() => dispatchEvent(new Event("arugula:control-refresh")));
    await expect.poll(() => page.evaluate(() => window.__arugula.control?.daemons.map((x) => x.online)), { timeout: 20_000 }).toEqual([false]);
    await page.locator("[data-shared-docs]").getByRole("button", { name: "Done", exact: true }).click();
    await page.evaluate(() => dispatchEvent(new CustomEvent("arugula:control-panel", { detail: "shared-docs" })));
    const row = page.locator(`[data-shared-doc="${id}"]`);
    await expect(row).toBeVisible();
    await expect(row.locator("[data-copy-doc-link]")).toBeDisabled();
    await expect(row.locator("[data-copy-doc-link]")).toHaveAttribute("title", "its machine is offline");
    await expect(row.locator("[data-open-shared-doc]")).toBeDisabled();
    await expect(row.locator("[data-open-shared-doc]")).toHaveAttribute("title", "its machine is offline");
    await row.locator("[data-unshare-doc]").click();
    await page.locator("[data-confirm-remove]").click();
    await expect(row).toHaveCount(0);
    expect((await fetch(`${url.split("#")[0]}/data/meta`)).status).toBe(404);
  });
});
