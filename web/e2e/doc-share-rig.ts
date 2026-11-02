// The rig the shared-Doc specs stand up: a real control (with a block domain
// or without), a machine joined to it and the owner's headless device
// (doc-share.spec.ts, #1066; the prototypes' checks in WebKit,
// doc-share.webkit.spec.ts, #506), and a visitor with nothing carried over.

import { spawn, type ChildProcess } from "node:child_process";
import { mkdirSync, mkdtempSync, rmSync } from "node:fs";
import type { AddressInfo } from "node:net";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { expect, type Browser, type BrowserContext, type Page } from "@playwright/test";
import { Device } from "../fixtures/device.ts";
import { fakeGithub } from "../fixtures/fakes.ts";
import { labs } from "./labs";
import { ANY, controlPort, daemonPort } from "./ports";
import { fill, newDoc, prototype, prototypeRan, type Attacker, type Post } from "./doc-steps";

export const DOMAIN = "blocks.localhost";

export interface Rig {
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

export async function up(url: string) {
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
export async function rig(blockDomain: string | null, browser?: Page): Promise<Rig> {
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


export const share = (r: Rig, d: number, method = "doc_share") => r.post(`/api/blocks/${d}/call/${method}`, {}).then((v) => v.url!);

/** The link in a fresh context: nothing carried over, so what shows is
 * what the link gives anyone. */
export async function visitor(browser: Browser, opts: Parameters<Browser["newContext"]>[0] = {}): Promise<{ ctx: BrowserContext; page: Page; seen: string[]; violations: string[] }> {
  const ctx = await browser.newContext({ storageState: { cookies: [], origins: [] }, extraHTTPHeaders: {}, ...opts });
  const page = await ctx.newPage();
  const seen: string[] = [];
  const violations: string[] = [];
  page.on("request", (q) => seen.push(q.url()));
  page.on("console", (m) => /Content Security Policy|Refused to/i.test(m.text()) && violations.push(m.text()));
  return { ctx, page, seen, violations };
}


/** A Doc whose first section is the hostile prototype (doc-steps.ts), shared;
 * the link. */
export async function prototypeLink(r: Rig, a: Attacker, title = "Probe doc"): Promise<string> {
  const [{ id: term }] = (await (await fetch(`${r.mini}/api/panes`)).json()) as { id: number }[];
  const d = await newDoc(r.post, term, title);
  await fill(r.post, d, "s1", `Findings:\n\n${prototype(a)}\nAfter it.`);
  return share(r, d);
}

/** The prototype in a shared Doc's page, a visitor's, in `scheme`: it ran in
 * its frame, on the proto origin the page was told of, and got nowhere. With
 * no block domain it says so and has no frame (#506). */
export async function prototypeInViewer(r: Rig, browser: Browser, a: Attacker, scheme: "light" | "dark", opts: Parameters<Browser["newContext"]>[0] = {}) {
  const url = await prototypeLink(r, a);
  const v = await visitor(browser, { colorScheme: scheme, ...opts });
  try {
    await v.page.goto(url);
    await expect(v.page.locator(".doc-title")).toHaveText("Probe doc", { timeout: 15_000 });
    const hasDomain = new URL(url).hostname.startsWith("docs.");
    if (!hasDomain) {
      await expect(v.page.locator("[data-prototype-note]")).toHaveText("Prototypes need a block domain on this page.");
      await expect(v.page.locator("iframe")).toHaveCount(0);
      return;
    }
    const proto = `${new URL(url).protocol}//proto.${new URL(url).host.slice("docs.".length)}`;
    // Click to run: a card with the title, and nothing framed or asked of the
    // runner's origin until the reader clicks.
    await expect(v.page.locator("[data-prototype-run]")).toContainText("Probe");
    await expect(v.page.locator("[data-prototype-run-button]")).toHaveText("Run prototype");
    await v.page.waitForTimeout(500);
    await expect(v.page.locator("iframe")).toHaveCount(0);
    expect(v.seen.filter((u) => u.startsWith(proto)), "nothing asked of the runner before the click").toEqual([]);
    await v.page.locator("[data-prototype-run-button]").click();
    await expect(v.page.locator("[data-prototype-run]")).toHaveCount(0);
    await expect(v.page.locator("[data-prototype-frame]")).toHaveAttribute("src", `${proto}/`);
    await prototypeRan(v.page, a, scheme);
    expect(await v.ctx.cookies(), "no cookie came or was set").toEqual([]);
    // The page asked only its own origin and the proto origin's one page.
    const own = new URL(url).origin;
    // (The prototype's own <img> to the attacker is a request the browser starts and its
    // CSP refuses; that nothing arrived is `prototypeRan`'s check on the server's side.)
    const strangers = v.seen.filter((u) => !u.startsWith(own) && !u.startsWith(proto) && !u.startsWith(a.url));
    expect(strangers, strangers.join("\n")).toEqual([]);
    expect(v.seen.filter((u) => u.startsWith(proto)).map((u) => new URL(u).pathname)).toEqual(["/"]);

    // The proto page on its own: still sandboxed (its CSP says so), so its
    // origin is opaque, it reads no cookie, and nothing else is served there.
    const solo = await v.ctx.newPage();
    await solo.goto(`${proto}/`);
    expect(await solo.evaluate(() => self.origin)).toBe("null");
    expect(await solo.evaluate(() => { try { return document.cookie; } catch { return "blocked"; } })).toBe("blocked");
    for (const path of ["/control.json", "/index.html", "/.arugula/boot.js"]) {
      expect((await v.ctx.request.get(`${proto}${path}`)).status(), path).toBe(404);
    }
  } finally {
    await v.ctx.close();
  }
}
