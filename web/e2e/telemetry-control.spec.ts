// M104: control's page asks the account once whether to send usage
// counts, with a card in the corner. Not under a test runner unless a spec
// says so (this one does, `__arugulaTelemetry`). Put away unanswered, it
// asks again on the next load; answered, never again. Yes sends what the
// page counted as `page day` (to a fake PostHog here); the switch under
// Sign-in and account turns it off. Then each machine the account owns asks
// once (M103), shown there, after the account's question is answered.

import { spawn, type ChildProcess } from "node:child_process";
import { existsSync, mkdtempSync, readFileSync, rmSync } from "node:fs";
import { createServer, type Server } from "node:http";
import type { AddressInfo } from "node:net";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { expect, test, type Page } from "@playwright/test";
import { fakeGithub } from "../fixtures/fakes.ts";
import { closeContexts } from "./helpers";
import { ANY, controlPort, listen } from "./ports";

test.afterAll(closeContexts);

let base = "";
const procs: ChildProcess[] = [];
const dirs: string[] = [];
let gh: Server;
let github = "";
let posthog: Server;
/** Events that reached the fake PostHog. */
const events: { event: string; distinct_id: string; properties: Record<string, unknown> }[] = [];

test.use({ baseURL: async ({}, use) => use(base) });

function temp(what: string) {
  const d = mkdtempSync(join(tmpdir(), `arugula-e2e-telemetry-${what}-`));
  dirs.push(d);
  return d;
}

test.beforeAll(async () => {
  gh = fakeGithub(0);
  await new Promise((r) => gh.once("listening", r));
  github = `http://127.0.0.1:${(gh.address() as AddressInfo).port}`;
  posthog = createServer((req, res) => {
    let body = "";
    req.on("data", (d) => (body += d));
    req.on("end", () => {
      if (req.url === "/batch/") events.push(...(JSON.parse(body) as { batch: typeof events }).batch);
      res.writeHead(200, { "content-type": "application/json" }).end("{}");
    });
  });
  const ph = `http://127.0.0.1:${await listen(posthog)}`;
  const db = join(temp("db"), "control.db");
  procs.push(
    spawn(
      "../target/debug/arugula-control",
      [
        ...["--listen", ANY, "--public-url", "http://127.0.0.1:0", "--db", db],
        ...["--github-client-id", "id", "--github-client-secret", "s", "--static-dir", "dist"],
        ...["--github-url", github, "--github-api", github],
        // Product analytics on, to a fake: only then does the page ask.
        ...["--posthog-api-key", "phc_e2e", "--posthog-host", ph],
      ],
      { stdio: "ignore" },
    ),
  );
  base = `http://127.0.0.1:${await controlPort(db, procs.at(-1))}`;
  for (let i = 0; i < 100; i++) {
    try {
      if ((await fetch(`${base}/control.json`)).ok) break;
    } catch {
      // not yet
    }
    await new Promise((r) => setTimeout(r, 100));
  }
});

test.afterAll(() => {
  for (const p of procs) p.kill("SIGKILL");
  gh?.close();
  posthog?.close();
  for (const d of dirs) rmSync(d, { recursive: true, force: true });
});

const card = (p: Page) => p.getByRole("dialog", { name: "Help improve Arugula?" });
const answer = async (p: Page) =>
  ((await (await p.request.get("/api/me/telemetry")).json()) as { on: boolean | null }).on;

test("the card asks once, yes sends counts, and the switch turns them off", async ({ browser }) => {
  test.setTimeout(90_000);
  const ctx = await browser.newContext();
  const page = await ctx.newPage();
  await page.goto("/");
  await page.locator("[data-signin=github]").click();
  await page.locator("[data-stored-codes]").check();
  await page.locator("[data-saved-codes]").click();
  await expect(page.locator("[data-account-menu]").first()).toBeVisible({ timeout: 20_000 });
  // Under the test runner, it doesn't ask.
  await page.waitForTimeout(1000);
  await expect(card(page)).toHaveCount(0);
  expect(await answer(page)).toBeNull();

  // A spec that wants it: asked, and put away, it asks again next load.
  await ctx.addInitScript(() => Object.assign(window, { __arugulaTelemetry: true }));
  await page.reload();
  await expect(card(page)).toBeVisible({ timeout: 20_000 });
  await expect(card(page)).toContainText("from your account on this page");
  await card(page).getByRole("button", { name: "Not now" }).click();
  await expect(card(page)).toHaveCount(0);
  expect(await answer(page)).toBeNull();
  await page.reload();
  await expect(card(page)).toBeVisible({ timeout: 20_000 });

  // Yes: kept, and never asked again.
  await card(page).getByRole("button", { name: "Yes, send them" }).click();
  await expect(card(page)).toHaveCount(0);
  expect(await answer(page)).toBe(true);
  await expect.poll(() => events.some((e) => e.event === "page opted in"), { timeout: 30_000 }).toBe(true);
  const asked = page.waitForResponse((r) => r.url().endsWith("/api/me/telemetry"));
  await page.reload();
  await asked;
  await expect(page.locator("[data-account-menu]").first()).toBeVisible({ timeout: 20_000 });
  await page.waitForTimeout(500);
  await expect(card(page)).toHaveCount(0);

  // What it counted goes as the page goes away: Settings was opened. The
  // page's `pagehide` here, not a navigation's: a send during a real
  // unload may or may not be let through by then, which isn't ours to test.
  await expect(page.locator("html[data-usage-counts]")).toHaveCount(1, { timeout: 20_000 });
  await page.locator("[data-account-menu]").first().click();
  await page.keyboard.press("Escape");
  await page.evaluate(() => dispatchEvent(new PageTransitionEvent("pagehide")));
  await expect.poll(() => events.find((e) => e.event === "page day"), { timeout: 30_000 }).toBeTruthy();
  const day = events.find((e) => e.event === "page day")!;
  expect(day.properties.view_settings).toBe(1);
  expect(Object.keys(day.properties).filter((k) => k.startsWith("view_"))).toEqual(["view_settings"]);
  expect(events.some((e) => e.event === "page opted in")).toBe(true);

  // The switch under Sign-in and account: on, and turned off.
  await page.goto("/");
  await page.locator("[data-account-menu]").first().click();
  await page.getByText("Sign-in and account…").click();
  const sw = page.getByRole("switch", { name: "Usage reports from this page" });
  await expect(sw).toBeChecked();
  await sw.uncheck();
  await expect.poll(() => answer(page)).toBe(false);
  await expect(sw).not.toBeChecked();
  // Control refuses counts now, whatever a page sends.
  const refused = await page.request.post("/api/telemetry/page", {
    data: { views: { swarm: 1 } },
    headers: { origin: base },
  });
  expect(refused.status()).toBe(403);
  expect(((await refused.json()) as { reason: string }).reason).toBe("telemetry_off");
  await page.reload();
  await expect(page.locator("[data-account-menu]").first()).toBeVisible({ timeout: 20_000 });
  await page.waitForTimeout(500);
  await expect(card(page)).toHaveCount(0);
});

test("a machine the account owns asks once on control's page, after the account's question", async ({ browser }) => {
  test.setTimeout(120_000);
  const ctx = await browser.newContext();
  // A new account (the test before answered for its own).
  await ctx.addCookies([{ name: "fake_github_login", value: "machine-owner", url: github }]);
  await ctx.addInitScript(() => Object.assign(window, { __arugulaTelemetry: true }));
  const page = await ctx.newPage();
  await page.goto("/");
  await page.locator("[data-signin=github]").click();
  await page.locator("[data-stored-codes]").check();
  await page.locator("[data-saved-codes]").click();
  await expect(card(page)).toBeVisible({ timeout: 20_000 });

  // One machine of the account's, nothing keeping its telemetry off, its
  // reports going nowhere.
  const state = temp("mac");
  const env: NodeJS.ProcessEnv = { ...process.env, ARUGULA_TELEMETRY_URL: "http://127.0.0.1:9" };
  for (const k of ["CI", "DO_NOT_TRACK", "ARUGULA_TELEMETRY", "NEXTEST_RUN_ID"]) delete env[k];
  const joining = spawn("../target/debug/arugulad", ["join", base, "--name", "mac", "--state-dir", state], { stdio: ["pipe", "pipe", "ignore"], env });
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
      { stdio: "ignore", env: { ...env, ARUGULA_WISP_TOKEN_FILE: "/nonexistent" } },
    ),
  );
  await expect
    .poll(async () => ((await (await ctx.request.get("/api/directory")).json()) as { daemons: { online: boolean }[] }).daemons.map((d) => d.online), {
      timeout: 20_000,
    })
    .toEqual([true]);
  await page.goto("/");
  await expect.poll(() => page.evaluate(() => !!window.__arugula?.client.e2e && window.__arugula.client.connected), { timeout: 20_000 }).toBe(true);

  // The account's question first, alone: the machine's waits.
  await expect(card(page)).toBeVisible({ timeout: 20_000 });
  await expect(card(page)).toContainText("from your account on this page");
  await page.waitForTimeout(1000);
  await expect(card(page)).toHaveCount(1);
  await card(page).getByRole("button", { name: "No thanks" }).click();

  // Then the machine's, with its preview.
  await expect(card(page)).toBeVisible({ timeout: 20_000 });
  await expect(card(page)).toContainText("from this machine: versions, which features are used");
  await card(page).getByText("What's sent").click();
  await expect(card(page).locator("[data-telemetry-preview] pre")).toContainText('"preview_only": true');
  await card(page).getByRole("button", { name: "Yes, send them" }).click();
  await expect(card(page)).toHaveCount(0);
  const consent = join(state, "telemetry.json");
  await expect.poll(() => existsSync(consent) && (JSON.parse(readFileSync(consent, "utf8")) as { telemetry: boolean }).telemetry).toBe(true);
  expect((JSON.parse(readFileSync(consent, "utf8")) as { via: string }).via).toBe("web");
  expect(await answer(page)).toBe(false);

  // Answered both: never asked again.
  await page.reload();
  await expect.poll(() => page.evaluate(() => !!window.__arugula?.client.e2e && window.__arugula.client.connected), { timeout: 20_000 }).toBe(true);
  await page.waitForTimeout(1500);
  await expect(card(page)).toHaveCount(0);
  await ctx.close();
});
