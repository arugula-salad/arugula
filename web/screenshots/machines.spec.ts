// `just screenshots`, Machines (Lift 6a, #1483; the site's pictures that
// Swarm's made before, #1544): the same fleet of six real daemons
// (e2e/fake-fleet.ts) with about 360 real panes between them, each a
// process doing scripted work at its own pace, under a scratch HOME. Agents
// ask through Claude Code's real hooks, and a batch of tests fails on one
// machine. Every name is made up. It's shot in the app (./app-page.tsx), on
// Machines: the sessions beside a busy tab and its head line.

import { createServer } from "node:net";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { devices, expect, test, type Page } from "@playwright/test";
import { FakeFleet } from "../e2e/fake-fleet";
import { fixturePage } from "../e2e/fixture-page";
import { inProject } from "../e2e/helpers";
import { P } from "./team";

// `just screenshots` sets SHOTS_OUT (arugula-salad/site's apps/site/public/img).
const out = process.env.SHOTS_OUT ?? join(dirname(fileURLToPath(import.meta.url)), "../../../site/apps/site/public/img");
const MACHINES = ["priya-mbp", "marcus-mbp", "build-01", "build-02", "build-03", "team-box"];
const PROJECTS = ["server", "web", "ios", "infra", "docs"];
/** Panes per machine, about 360 in all. */
const PER_MACHINE = 60;

// The fleet's daemons inherit this: no "a newer release is out" chip.
process.env.ARUGULA_NO_UPDATE_CHECK = "true";

let fake: FakeFleet;
let base = "";

/** A free port from the kernel (well away from the e2e tests' 7750–7789). */
function freePort(): Promise<number> {
  return new Promise((resolve, reject) => {
    const s = createServer();
    s.once("error", reject);
    s.listen(0, "127.0.0.1", () => {
      const port = (s.address() as { port: number }).port;
      s.close(() => resolve(port));
    });
  });
}

const pick = <T,>(a: T[]): T => a[Math.floor(Math.random() * a.length)];

/** What one pane runs: mostly projects, some plain directories; a mix of
 * busy and quiet work, and a few shells at their prompt. */
function work(): [string, string | undefined] {
  const where = Math.random() < 0.8 ? pick(PROJECTS) : pick(["scratch", "var"]);
  const command = pick([
    "cargo build", "cargo test", "cargo build", "go build ./...", "go test ./...", "make -j8",
    "pytest -x", "pytest -x", "npm run dev", "uvicorn app:main --reload", "docker compose logs -f",
    "tail -f worker.log", "journalctl -f", "nvim src/main.rs", "vim notes.md",
    undefined, undefined, undefined, undefined,
  ]);
  return [where, command];
}

/** Up to `n` of `jobs` at once. */
async function batch<T>(jobs: (() => Promise<T>)[], n = 8) {
  const queue = [...jobs];
  await Promise.all(Array.from({ length: n }, async () => {
    for (let job = queue.shift(); job; job = queue.shift()) await job();
  }));
}

test.describe.configure({ mode: "serial" });

test.beforeAll(async () => {
  test.setTimeout(240_000);
  fake = new FakeFleet(PROJECTS);
  for (const m of MACHINES) await fake.machine(m, await freePort());
  base = fake.machines[0].url;
  await batch(MACHINES.flatMap((m) => Array.from({ length: PER_MACHINE }, () => () => {
    const [where, command] = work();
    return fake.run(m, where, command);
  })));
  // What needs someone: agents asking, a failed batch, a long build done.
  await fake.agentAsks("priya-mbp", "server", "cargo test -p server");
  await fake.agentAsks("marcus-mbp", "server", "git push origin main");
  await fake.agentAsks("team-box", "infra", "terraform plan -out=tfplan");
  await fake.trouble("build-02", 4);
  await fake.finish("build-01");
});

test.afterAll(() => fake?.stop());

// The app's page is a fixture page this origin serves: Chrome counts it as
// public, so it needs this to reach the daemons.
test.use({ permissions: ["local-network-access"] });

let moved = false;
/** Every machine's sessions in Game night. */
async function intoProject(page: Page) {
  if (moved) return;
  for (const m of fake.machines) {
    await page.goto(`${m.url}/`);
    await expect.poll(() => page.evaluate(() => window.__arugula?.client.state !== null && window.__arugula?.client.state !== undefined)).toBe(true);
    await inProject(page, P, "Game night");
  }
  moved = true;
}

async function machines(page: Page) {
  await intoProject(page);
  const body = await fixturePage("../screenshots/app-page.tsx");
  await page.route(/\/p\/prj_/, (r) => r.fulfill({ contentType: "text/html", body }));
  await page.goto(`${base}/p/${P}/machines?machine=priya-mbp`);
  await expect(page.locator("[data-pane-toolbar], [data-pane]").first()).toBeVisible({ timeout: 30_000 });
  // Let the panes fill and their state come through.
  await page.waitForTimeout(8000);
  const hint = page.getByRole("button", { name: "Got it", exact: true });
  if (await hint.isVisible()) await hint.click();
}

test.describe("desktop", () => {
  test.use({ viewport: { width: 1440, height: 900 }, deviceScaleFactor: 2 });
  test("Machines: the sessions beside a busy tab", async ({ page }) => {
    await machines(page);
    await page.screenshot({ path: join(out, "machines.png") });
  });
});

test.describe("phone", () => {
  const { viewport, userAgent, deviceScaleFactor, isMobile, hasTouch } = devices["Pixel 7"];
  test.use({ viewport, userAgent, deviceScaleFactor, isMobile, hasTouch });
  test("Machines on a phone", async ({ page }) => {
    await machines(page);
    await page.screenshot({ path: join(out, "machines-phone.png") });
  });
});
