// The Mac app's window, without the app (#551): the page in a browser
// context of its own, routed as crates/desktop routes it. Only the app's
// native parts are here, each as the Rust does it:
//
// - main.rs `home`, `page_at`: the window opens the daemon's page through
//   its sign-in link; once joined, the app's sign-in page, or control's
//   page once the app is signed in.
// - main.rs `on_navigation`, `on_new_window`, `ours`, `open_outside`: the
//   daemon's, control's and the app's own pages stay in the window;
//   anything else (an approval link, GitHub) opens in the person's
//   browser, a new tab there. Control's own GitHub sign-in can't finish in
//   the window: it goes to the app's sign-in page.
// - main.rs `follow_join`, cloud.rs `moves`: every 2 s the app asks the
//   daemon whether it joined; when it did, a window on the daemon's page
//   moves to the new home.
// - cloud.rs `init_script`, `cloud_status`, `cloud_signin`, `await_grant`,
//   `cloud_local`: what crates/desktop/dist/signin.html invokes.
//
// Everything else is the real page, the real daemon and the real control.

import { createHash, randomBytes } from "node:crypto";
import { readFileSync } from "node:fs";
import { createServer, type Server } from "node:http";
import { resolve } from "node:path";
import type { BrowserContext, Page, Route } from "@playwright/test";
import { asPerson, type Person } from "./world";

/** The app's own pages (cloud.rs `app_url`, as on Windows: a URL the
 * window can load; macOS uses tauri://localhost). */
const APP = "http://tauri.localhost";
const SIGNIN = readFileSync(resolve("../crates/desktop/dist/signin.html"), "utf8");
const JOIN_POLL = 2_000;

export class App {
  /** cloud.json's `signed_in`, kept across restarts. */
  private signedIn = new Set<string>();
  private localOnly = false;
  private control: string | null = null;
  private name = "";
  private place: string | null = null;
  private poll?: NodeJS.Timeout;
  private grants: Server[] = [];
  /** Tabs the app opened in the person's browser, newest last. */
  readonly opened: Page[] = [];
  /** Called with each URL the app sends to the browser. */
  onOutside?: (url: string) => void;

  private constructor(
    readonly person: Person,
    readonly ctx: BrowserContext,
  ) {}

  /** The app's webview: storage of its own that outlives a restart. */
  static async install(person: Person, browser: import("@playwright/test").Browser): Promise<App> {
    const ctx = await browser.newContext({
      viewport: { width: 1200, height: 760 },
    });
    const app = new App(person, ctx);
    await app.read();
    await ctx.addInitScript(asPerson);
    await ctx.addInitScript(
      ({ name, control }) => {
        Object.assign(window, {
          __arugulaApp: { name, platform: "macos", nativeCalls: false },
        });
        // signin.html's invoke(), answered by the test (cloud.rs's commands).
        const call = (
          window as unknown as {
            __appInvoke: (cmd: string) => Promise<unknown>;
          }
        ).__appInvoke;
        Object.assign(window, {
          __TAURI__: { core: { invoke: (cmd: string) => call(cmd) } },
        });
        // on_new_window gets the whole URL, #fragment and all; a request
        // (what route() sees) never carries the fragment. So new windows
        // are caught here and handed over as they are.
        const newWindow = (
          window as unknown as {
            __appNewWindow: (url: string) => Promise<void>;
          }
        ).__appNewWindow;
        addEventListener(
          "click",
          (e) => {
            const a = (e.target as Element | null)?.closest?.("a[target=_blank]") as HTMLAnchorElement | null;
            if (!a || !a.href) return;
            e.preventDefault();
            void newWindow(a.href);
          },
          true,
        );
        window.open = ((url?: string | URL) => {
          if (url !== undefined) void newWindow(new URL(String(url), location.href).href);
          return null;
        }) as typeof window.open;
        if (control && location.origin === new URL(control).origin)
          addEventListener("DOMContentLoaded", () => {
            const s = document.createElement("style");
            s.textContent = "[data-signin=passkey] { display: none !important; }";
            document.head.appendChild(s);
          });
      },
      { name: app.deviceName(), control: person.world.control },
    );
    await ctx.exposeBinding("__appInvoke", (source, cmd: string) => app.invoke(source.page, cmd));
    await ctx.exposeBinding("__appNewWindow", (_source, url: string) => app.newWindow(url));
    await ctx.route("**/*", (route) => app.route(route));
    return app;
  }

  deviceName() {
    return this.name ? `Arugula app on ${this.name}` : "Arugula app";
  }

  /** cloud.rs `local`: the daemon's /api/host, with the local token. */
  private async read() {
    try {
      const r = await fetch(`${this.person.daemon}/api/host`, {
        headers: { authorization: `Bearer ${this.person.token}` },
      });
      const v = (await r.json()) as {
        name?: string;
        control?: string;
        control_state?: { state?: string; kind?: string; name?: string };
      };
      this.name = v.name ?? "this machine";
      const dropped = v.control_state?.state === "dropped";
      this.control = dropped ? null : (v.control?.replace(/\/$/, "") ?? null);
      const s = v.control_state;
      this.place =
        s?.state === "joined" ? (s.kind === "team" ? (s.name ? `the team ${s.name}` : "a team") : s.name ? `${s.name}'s account` : "your account") : null;
    } catch {
      // the daemon is restarting: keep what was read
    }
  }

  private pageAt(path: string) {
    return `${this.person.daemon}/auth?token=${this.person.token}&next=${encodeURIComponent(path).replace(/%2F/g, "/")}`;
  }

  /** main.rs `home`. */
  home() {
    if (this.control && !this.localOnly) return this.signedIn.has(this.control) ? `${this.control}/` : `${APP}/signin.html`;
    return this.pageAt("/");
  }

  private ours(url: URL) {
    if (["about:", "blob:", "data:"].includes(url.protocol)) return true;
    if (url.origin === APP) return true;
    if (this.control && url.origin === new URL(this.control).origin) return true;
    return url.origin === new URL(this.person.daemon).origin;
  }

  private isDaemonPage(url: string) {
    try {
      return new URL(url).origin === new URL(this.person.daemon).origin;
    } catch {
      return false;
    }
  }

  private async route(route: Route) {
    const req = route.request();
    const url = new URL(req.url());
    if (url.origin === APP) {
      if (url.pathname === "/signin.html")
        return route.fulfill({
          status: 200,
          contentType: "text/html",
          body: SIGNIN,
        });
      return route.fulfill({ status: 404, body: "" });
    }
    if (!req.isNavigationRequest()) return route.continue();
    // A new window's first navigation has no frame yet (on_new_window).
    let page: Page | null = null;
    try {
      if (req.frame().parentFrame() !== null) return route.continue();
      page = req.frame().page();
    } catch {
      page = null;
    }
    // on_navigation: control's GitHub sign-in can't finish here.
    if (this.control && req.url().startsWith(`${this.control}/auth/github`)) {
      this.signedIn.delete(this.control);
      await route.abort();
      void (page ?? this.window).goto(`${APP}/signin.html`).catch(() => {});
      return;
    }
    if (this.ours(url)) return route.continue();
    // on_new_window / on_navigation: the person's browser takes it.
    await route.abort();
    for (const p of this.ctx.pages()) if (p.opener() && p.url() === "about:blank") void p.close().catch(() => {});
    await this.outside(req.url());
  }

  /** main.rs `on_new_window`: ours in a window of the app's, anything
   * else in the browser. */
  async newWindow(url: string) {
    if (this.ours(new URL(url))) {
      const w = await this.ctx.newPage();
      await w.goto(url);
    } else await this.outside(url);
  }

  /** main.rs `open_outside`: a new tab in the person's browser. */
  async outside(url: string) {
    this.onOutside?.(url);
    const tab = await this.person.browser.newPage();
    this.opened.push(tab);
    await tab.goto(url);
  }

  /** The browser tab the app opened last. */
  get lastOpened(): Page {
    const p = this.opened.at(-1);
    if (!p) throw new Error("the app hasn't opened anything in the browser");
    return p;
  }

  /** The app starts: one window at home, and the join poll. */
  async launch(): Promise<Page> {
    await this.read();
    const w = await this.ctx.newPage();
    await w.goto(this.home());
    let was = this.control;
    this.poll = setInterval(async () => {
      await this.read();
      const now = this.control;
      if (now === was) return;
      this.localOnly = false;
      for (const p of this.ctx.pages()) {
        const url = p.url();
        const moves = was === null ? this.isDaemonPage(url) : url.startsWith(was) || (url.startsWith(APP) && new URL(url).pathname === "/signin.html");
        if (moves) void p.goto(this.home()).catch(() => {});
      }
      was = now;
    }, JOIN_POLL);
    return w;
  }

  /** The app quits: its windows close; the daemon and the webview's
   * storage stay. */
  async quit() {
    clearInterval(this.poll);
    for (const g of this.grants) g.close();
    for (const p of this.ctx.pages()) await p.close();
  }

  get window(): Page {
    // Not a popup the app is about to turn away (on_new_window).
    const p =
      this.ctx
        .pages()
        .filter((x) => !x.opener())
        .at(-1) ?? this.ctx.pages().at(-1);
    if (!p) throw new Error("the app has no window");
    return p;
  }

  private async invoke(page: Page, cmd: string): Promise<unknown> {
    if (cmd === "cloud_status") {
      await this.read();
      return {
        control: this.control,
        name: this.deviceName(),
        machine: this.name,
        place: this.place,
        auto: false,
      };
    }
    if (cmd === "cloud_local") {
      this.localOnly = true;
      await page.goto(this.pageAt("/"));
      return null;
    }
    if (cmd === "cloud_signin") return this.signin(page);
    throw new Error(`no command ${cmd}`);
  }

  /** cloud.rs `cloud_signin` and `await_grant`. */
  private async signin(page: Page) {
    const control = this.control;
    if (!control) throw "this machine isn't joined to Arugula cloud";
    const verifier = randomBytes(32).toString("hex");
    const challenge = createHash("sha256").update(verifier).digest("hex");
    let ticket = "";
    const grants = createServer((req, res) => {
      const u = new URL(req.url!, "http://127.0.0.1");
      const grant = u.pathname === "/arugula-signin" && u.searchParams.get("ticket") === ticket ? (u.searchParams.get("grant") ?? "") : "";
      if (!/^[0-9a-f]+$/.test(grant)) return res.writeHead(404).end();
      res.writeHead(303, { location: `${control}/#app-done` }).end();
      grants.close();
      this.signedIn.add(control);
      void page.goto(`${control}/#app-redeem=${ticket}.${grant}.${verifier}`).catch(() => {});
    });
    this.grants.push(grants);
    await new Promise<void>((ok) => grants.listen(0, "127.0.0.1", ok));
    const port = (grants.address() as { port: number }).port;
    const r = await fetch(`${control}/auth/app`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ name: this.deviceName(), challenge, port }),
    });
    const t = (await r.json()) as {
      ticket: string;
      code: string;
      url: string;
      error?: string;
    };
    if (!r.ok) throw t.error ?? "control refused the sign-in";
    ticket = t.ticket;
    void this.outside(t.url);
    return { code: t.code, url: t.url };
  }
}
