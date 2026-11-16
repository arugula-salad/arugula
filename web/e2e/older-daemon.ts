// A released daemon with this checkout's page in front of it (Arugula
// #808), for the specs `just older-daemons` runs (playwright.older.config.ts).
// The daemon serves the page it was built with; here every request for one
// of the page's files is answered from web/dist instead, as control serves
// its newest page to an older machine, and the API and the socket reach the
// daemon as they would. The daemon runs with a home of its own, so an old
// one's defaults touch nothing of the person's.

import { spawn, type ChildProcess } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, rmSync, statSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { test as base, type BrowserContext } from "@playwright/test";
import { keyHeaders, localToken, signIn } from "./local-token";
import { ANY, daemonPort } from "./ports";

export interface OlderDaemon {
  version: string;
  bin: string;
}

const DIST = fileURLToPath(new URL("../dist/", import.meta.url));

/** Serve the page's own files from web/dist to `context`'s pages on `url`;
 * the daemon gets the rest. */
export async function pageFromDist(context: BrowserContext, url: string) {
  if (!existsSync(join(DIST, "index.html"))) throw new Error("no web/dist/index.html: run `just web`");
  await context.route(`${url}/**`, async (route) => {
    const path = new URL(route.request().url()).pathname;
    const file = path === "/" ? "index.html" : decodeURIComponent(path.slice(1));
    const at = join(DIST, file);
    if (file.startsWith("api/") || file.includes("..") || !existsSync(at) || !statSync(at).isFile()) return route.continue();
    // The page keeps the daemon's headers (its policy, its key's cookie).
    if (file === "index.html") return route.fulfill({ response: await route.fetch(), path: at });
    return route.fulfill({ path: at });
  });
}

/** A call to the daemon at `url` as its owner: with the page key, and the
 * local token's cookie for daemons from before the key (0.42). */
export function api(url: string, path: string, body?: unknown): Promise<Response> {
  return fetch(url + path, {
    method: body === undefined ? "GET" : "POST",
    headers: { ...keyHeaders, cookie: `arugula_${new URL(url).port}=${localToken}`, ...(body === undefined ? {} : { "content-type": "application/json" }) },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
}

/** Start `d` on a free port, with a state directory and a home of its own. */
export async function startDaemon(d: OlderDaemon): Promise<{ proc: ChildProcess; url: string; dir: string; version: string }> {
  const dir = mkdtempSync(join(tmpdir(), `arugula-older-${d.version}-`));
  const home = join(dir, "home");
  const state = join(dir, "state");
  // Labs on, as the suite runs (labs.ts), and the Agents page's flag.
  mkdirSync(state, { recursive: true });
  writeFileSync(join(state, "flags.json"), JSON.stringify({ flags: { labs: true, "agent-delegation": true } }));
  for (const sub of [".cache", ".config", ".local/share", "run"]) mkdirSync(join(home, sub), { recursive: true });
  const proc = spawn(
    d.bin,
    [
      ...["--listen", ANY, "--state-dir", state, "--shell", "bash --norc --noprofile", "--no-manager-env"],
      ...["--tailscale-socket", "/nonexistent/tailscaled.sock", "--no-update-check", "--no-claude-ide", "--name", "older"],
    ],
    {
      stdio: ["ignore", "ignore", process.env.OLDER_DAEMON_LOG ? "inherit" : "ignore"],
      env: {
        ...process.env,
        HOME: home,
        XDG_CACHE_HOME: join(home, ".cache"),
        XDG_CONFIG_HOME: join(home, ".config"),
        XDG_DATA_HOME: join(home, ".local/share"),
        XDG_STATE_HOME: join(home, ".local/state"),
        XDG_RUNTIME_DIR: join(home, "run"),
        ARUGULA_CLAUDE_IDE_DIR: join(home, "ide"),
        ARUGULA_AGENTS_DIR: join(home, "agents"),
        ARUGULA_INFISICAL_BIN: "/bin/false",
        ARUGULA_GH_BIN: "/bin/false",
        ARUGULA_CHANT: "",
        RUST_LOG: "warn",
      },
    },
  );
  const port = await daemonPort(state, proc);
  const url = `http://127.0.0.1:${port}`;
  for (let i = 0; ; i++) {
    try {
      if ((await api(url, "/api/host")).ok) break;
    } catch {
      // not up yet
    }
    if (i > 100) throw new Error(`daemon ${d.version} didn't answer on ${url}`);
    await new Promise((r) => setTimeout(r, 100));
  }
  const host = (await (await api(url, "/api/host")).json()) as { version?: string };
  if (host.version && host.version !== d.version) throw new Error(`${d.bin} says it is ${host.version}, not ${d.version}`);
  return { proc, url, dir, version: d.version };
}

/** A spec's test: `daemon` is the project's released daemon, running, and
 * every page's files are this checkout's. */
export const test = base.extend<{ errors: string[] }, { older: OlderDaemon; daemon: { url: string; version: string } }>({
  older: [{ version: "", bin: "" }, { option: true, scope: "worker" }],
  daemon: [
    async ({ older }, use) => {
      const d = await startDaemon(older);
      await use({ url: d.url, version: d.version });
      d.proc.kill("SIGKILL");
      rmSync(d.dir, { recursive: true, force: true });
    },
    { scope: "worker" },
  ],
  baseURL: async ({ daemon }, use) => use(daemon.url),
  context: async ({ context, daemon }, use) => {
    await pageFromDist(context, daemon.url);
    // Signed in as a person is, by the link `arugula web` opens: an old
    // daemon doesn't read the page key the browser carries.
    const p = await context.newPage();
    await p.goto(signIn(daemon.url));
    await p.close();
    await use(context);
  },
  // What the page threw, uncaught or caught by a page's guard (which says
  // so on the console, shared/ui/guard.tsx): a test fails on any.
  errors: async ({ page }, use) => {
    const errors: string[] = [];
    page.on("pageerror", (e) => errors.push(`${e.name}: ${e.message}`));
    page.on("console", (m) => {
      if (m.type() === "error" && m.text().startsWith("A page failed to draw")) errors.push(m.text());
    });
    await use(errors);
  },
});
