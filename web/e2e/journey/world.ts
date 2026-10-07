// The world a journey runs in (#551): an Arugula control of the run's own
// with a GitHub that asks a first-time user to authorize the app, as the
// real one does, and people, each with a home directory, a daemon (what
// the Mac app's launch agent runs) and a browser of their own.
//
// Nothing is approved, joined or signed in ahead: a person starts with
// an empty home and no account.

import { spawn, type ChildProcess } from "node:child_process";
import { mkdirSync, mkdtempSync, readFileSync, rmSync, symlinkSync } from "node:fs";
import { createServer, type IncomingMessage, type Server } from "node:http";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import type { Browser, BrowserContext } from "@playwright/test";
import { githubId } from "../../fixtures/fakes.ts";
import { ANY, controlPort, daemonPort, listen } from "../ports";

export const BIN = resolve("../target/debug");

/** A browser that isn't Playwright's to the page: the client greets a
 * person (Getting started) but never automation (welcome.spec.ts). */
export const asPerson = () => Object.defineProperty(Navigator.prototype, "webdriver", { get: () => false });

export class World {
  control = "";
  github = "";
  private procs: ChildProcess[] = [];
  private dirs: string[] = [];
  private gh?: Server;

  temp(what: string) {
    const d = mkdtempSync(join(tmpdir(), `arugula-journey-${what}-`));
    this.dirs.push(d);
    return d;
  }

  spawn(cmd: string, args: string[], env: NodeJS.ProcessEnv = process.env) {
    const p = spawn(cmd, args, { stdio: ["ignore", "pipe", "pipe"], env });
    this.procs.push(p);
    return p;
  }

  async start() {
    this.gh = fakeGithub();
    this.github = `http://127.0.0.1:${await listen(this.gh)}`;
    const db = join(this.temp("control"), "control.db");
    const p = this.spawn(`${BIN}/arugula-control`, [
      ...["--listen", ANY, "--public-url", "http://127.0.0.1:0", "--db", db],
      ...["--github-client-id", "id", "--github-client-secret", "s", "--static-dir", "dist"],
      ...["--github-url", this.github, "--github-api", this.github],
    ]);
    this.control = `http://127.0.0.1:${await controlPort(db, p)}`;
    for (let i = 0; i < 100; i++) {
      try {
        if ((await fetch(`${this.control}/control.json`)).ok) return;
      } catch {
        // not yet
      }
      await new Promise((r) => setTimeout(r, 100));
    }
    throw new Error("control didn't come up");
  }

  /** Someone with a fresh home, signed in to GitHub (not to Arugula) in
   * their own browser, with nothing of Arugula's yet. */
  async person(browser: Browser, login: string): Promise<Person> {
    const home = this.temp(login);
    const ctx = await browser.newContext({
      viewport: { width: 1100, height: 720 },
    });
    await ctx.addInitScript(asPerson);
    await ctx.addCookies([{ name: "gh_user", value: login, url: this.github }]);
    return new Person(this, login, home, ctx);
  }

  stop() {
    for (const p of this.procs) p.kill("SIGKILL");
    this.gh?.close();
    for (const d of this.dirs) rmSync(d, { recursive: true, force: true });
  }
}

export class Person {
  daemon = "";
  token = "";
  private proc?: ChildProcess;
  constructor(
    readonly world: World,
    readonly login: string,
    readonly home: string,
    /** Their own browser (Safari, Chrome: wherever links open). */
    readonly browser: BrowserContext,
  ) {}

  get state() {
    return join(this.home, ".local/state/arugula");
  }

  /** What a person's shell has after the Mac app's first start: its
   * PATH, with ~/.local/bin holding the `arugula` the app links there
   * (only that one: #550). */
  shellEnv(): NodeJS.ProcessEnv {
    return {
      HOME: this.home,
      PATH: `${this.home}/.local/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin`,
      TERM: "xterm-256color",
      LANG: process.env.LANG ?? "en_US.UTF-8",
    };
  }

  /** The app's first start: the daemon as its launch agent runs it (no
   * flags of the person's; the test's own are only for the run: a free
   * port, a plain shell, and this run's control standing in for the
   * hosted one), and the CLI linked into ~/.local/bin (install_cli). */
  async installApp(machine: string) {
    mkdirSync(join(this.home, ".local/bin"), { recursive: true });
    symlinkSync(`${BIN}/arugula`, join(this.home, ".local/bin/arugula"));
    this.proc = this.world.spawn(
      `${BIN}/arugulad`,
      [
        ...["--listen", ANY, "--name", machine, "--control", this.world.control],
        ...["--shell", "bash --norc --noprofile", "--no-manager-env", "--tailscale-socket", "/nonexistent/sock"],
      ],
      {
        ...this.shellEnv(),
        ARUGULA_KEEP_PANES: "true",
        ARUGULA_CLAUDE_IDE_DIR: this.world.temp("ide"),
      },
    );
    this.daemon = `http://127.0.0.1:${await daemonPort(this.state, this.proc)}`;
    this.token = readFileSync(join(this.state, "local-token"), "utf8").trim();
  }
}

function body(req: IncomingMessage): Promise<string> {
  return new Promise((ok) => {
    let b = "";
    req.on("data", (d) => (b += d));
    req.on("end", () => ok(b));
  });
}

/** GitHub as a first-time user meets it: signed in to GitHub already
 * (the `gh_user` cookie), asked once to authorize the OAuth app. */
function fakeGithub(): Server {
  return createServer(async (req, res) => {
    const u = new URL(req.url!, "http://github");
    const cookies = req.headers.cookie ?? "";
    const who = /(?:^|;\s*)gh_user=(\w+)/.exec(cookies)?.[1] ?? "nobody";
    if (u.pathname === "/login/oauth/authorize") {
      const authorized = new RegExp(`(?:^|;\\s*)gh_ok_${who}=1`).test(cookies);
      if (!authorized && u.searchParams.get("authorize") !== "1") {
        u.searchParams.set("authorize", "1");
        res
          .writeHead(200, { "content-type": "text/html" })
          .end(
            `<!doctype html><title>Authorize application</title><main style="font:16px system-ui;max-width:28rem;margin:4rem auto">` +
              `<h1>Authorize Arugula</h1><p>Arugula by arugula-salad wants to access your <b>${who}</b> account.</p>` +
              `<p>Public data only: your login and avatar.</p>` +
              `<a role="button" href="${u.pathname}${u.search}" style="display:inline-block;padding:8px 16px;background:#1f883d;color:#fff;border-radius:6px;text-decoration:none">Authorize arugula-salad</a></main>`,
          );
        return;
      }
      const back = new URL(u.searchParams.get("redirect_uri")!);
      back.searchParams.set("code", `c0de.${who}`);
      back.searchParams.set("state", u.searchParams.get("state")!);
      res
        .writeHead(302, {
          location: back.href,
          "set-cookie": `gh_ok_${who}=1; Path=/`,
        })
        .end();
    } else if (u.pathname === "/login/oauth/access_token") {
      const code = new URLSearchParams(await body(req)).get("code") ?? "";
      res.writeHead(200, { "content-type": "application/json" }).end(
        JSON.stringify({
          access_token: `gho_${code.replace(/^c0de\./, "")}`,
        }),
      );
    } else if (u.pathname === "/user") {
      const login = String(req.headers.authorization ?? "").replace(/^(Bearer|token) gho_/, "");
      res.writeHead(200, { "content-type": "application/json" }).end(JSON.stringify({ id: githubId(login), login }));
    } else res.writeHead(404).end();
  });
}
