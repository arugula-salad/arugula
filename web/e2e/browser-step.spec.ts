// #236: settings flows that leave for another site and finish on the server
// strand the desktop app's window: the site opens in the person's browser
// and the window never updates. In the app (`__arugulaApp`, faked here) the
// GitHub App's install and Upgrade open the site in the browser and show
// "Finish in your browser", which asks again when the window regains focus.
// Faked: GitHub's answer (the repo link's 409 `install_app`, then linked),
// Stripe's checkout and `/api/billing`; `window.open` is recorded, not
// followed (the app's own hand-off to the browser is crates/desktop's).
// With the app's `open_outside` command (#364; faked here, the xvfb
// platform test runs the real one) the page asks the app and doesn't open,
// and the step asks as soon as it has the address, with no second click
// (#381); *Open again* asks once more. An app without the command (its
// invoke rejects) keeps the open button and its click's `window.open`.
// In a plain browser the same buttons still redirect in the tab.

import { expect, test, type Page } from "@playwright/test";
import { closeContexts } from "./helpers";
import { TeamControl, home } from "./team-fixture";

const t = new TeamControl("browser-step");
const INSTALL = "https://github.example/apps/arugula/installations/new";
const CHECKOUT = "https://stripe.example/checkout/cs_test_1";

test.describe.configure({ mode: "serial" });
test.afterAll(async ({ browser }) => {
  await closeContexts({ browser });
  t.stop();
});

let alice: Page;
let app = "";
let project = "";

/** Alice's page on the platform, in the desktop app or not; `window.open`
 * records what it was asked to open. */
async function visit(browser: import("@playwright/test").Browser, path: string, desktop: boolean, commands: boolean | "old" = false) {
  const ctx = await browser.newContext();
  await ctx.addCookies(await alice.context().cookies());
  await ctx.addInitScript(([d, c]) => {
    if (d) Object.assign(window, { __arugulaApp: { name: "Arugula app on test-mac", platform: "macos" } });
    const asked: unknown[][] = [];
    (window as unknown as { asked: unknown[][] }).asked = asked;
    if (c === true) Object.assign(window, { __TAURI__: { core: { invoke: async (...a: unknown[]) => void asked.push(a) } } });
    // An app from before open_outside: Tauri rejects a command it hasn't got.
    if (c === "old") Object.assign(window, { __TAURI__: { core: { invoke: async (...a: unknown[]) => (asked.push(a), Promise.reject("command open_outside not found")) } } });
    (window as unknown as { opened: unknown[][] }).opened = [];
    window.open = ((...a: unknown[]) => ((window as unknown as { opened: unknown[][] }).opened.push(a), null)) as typeof window.open;
  }, [desktop, commands] as const);
  // The sites that are left for aren't there; the tab's redirect still shows.
  await ctx.route(/^https:\/\/(github|stripe)\.example\//, (r) => r.fulfill({ contentType: "text/html", body: "<title>elsewhere</title>" }));
  const p = await ctx.newPage();
  await p.goto(`${app}${path}`);
  await expect(p.locator("[data-platform-bar]").first()).toBeVisible({ timeout: 15_000 });
  return p;
}

const opened = (p: Page) => p.evaluate(() => (window as unknown as { opened: unknown[][] }).opened);
const asked = (p: Page) => p.evaluate(() => (window as unknown as { asked: unknown[][] }).asked);
const focus = (p: Page) => p.evaluate(() => window.dispatchEvent(new Event("focus")));

test("setup: Alice on the platform with a project", async ({ browser }) => {
  test.setTimeout(180_000);
  await t.start(["--platform-host", "app.localhost"]);
  app = t.base.replace("//127.0.0.1", "//app.localhost");
  alice = await t.person(browser, "alice");
  await alice.evaluate(() => window.__arugula.control!.createTeam("Acme"));
  await home(alice);
  await alice.goto(app);
  await expect(alice.locator("section[data-platform-home] [data-project]").first()).toBeVisible({ timeout: 15_000 });
  await alice.locator("section[data-platform-home] [data-project]", { hasText: "Acme" }).click();
  project = /\/p\/(prj_[0-9a-f]+)/.exec(alice.url())![1];
});

/** The repo routes as GitHub's App leaves them: 409 until installed. */
async function fakeRepos(p: Page, state: { installed: boolean; posts: number }) {
  const row = { repo_id: 7, full_name: "acme/site", private: false, linked_by: null, linked_at: 1, unlinked_at: null, purge_after: null, mirror_content: false };
  await p.context().route(`**/api/projects/${project}/repos`, (r) => {
    if (r.request().method() === "GET") return r.fulfill({ json: { project, repos: state.installed ? [row] : [] } });
    state.posts++;
    return state.installed
      ? r.fulfill({ json: row })
      : r.fulfill({ status: 409, json: { error: "Install the GitHub App first", reason: "install_app", install_url: INSTALL } });
  });
}

test("the app: Link opens GitHub in the browser, and coming back links the repo", async ({ browser }) => {
  const p = await visit(browser, `/p/${project}/settings`, true);
  const state = { installed: false, posts: 0 };
  await fakeRepos(p, state);
  await p.reload();
  await p.locator("[data-link-repo]").fill("acme/site");
  await p.getByRole("button", { name: "Link", exact: true }).click();
  const step = p.locator("[data-browser-step]");
  await expect(step.locator("[data-browser-step-open]")).toHaveText("Install the GitHub App ↗");
  await expect(step.locator("[data-browser-step-open]")).toHaveAttribute("title", "Opens in your browser");
  expect(await opened(p)).toEqual([]);
  await expect(p).toHaveURL(new RegExp(`${project}.*/settings`));

  await step.locator("[data-browser-step-open]").click();
  expect(await opened(p)).toEqual([[INSTALL, "_blank", "noopener"]]);
  await expect(step).toContainText("Finish in your browser, then come back here.");

  // Back in the window before it's installed: asked, not yet.
  const before = state.posts;
  await focus(p);
  await expect.poll(() => state.posts).toBe(before + 1);
  await expect(step).toBeVisible();
  // Not installed yet is waiting, not an error.
  await expect(step.locator(".control-error")).toHaveCount(0);
  await expect(p.locator("[data-linked-repo]")).toHaveCount(0);

  // Installed: the next focus links it, and the step goes.
  state.installed = true;
  await focus(p);
  await expect(p.locator('[data-linked-repo="acme/site"]')).toBeVisible();
  await expect(step).toHaveCount(0);
  await p.context().close();
});

test("the app: Check again asks, and Cancel stops asking on focus", async ({ browser }) => {
  const p = await visit(browser, `/p/${project}/settings`, true);
  const state = { installed: false, posts: 0 };
  await fakeRepos(p, state);
  await p.reload();
  await p.locator("[data-link-repo]").fill("acme/site");
  await p.getByRole("button", { name: "Link", exact: true }).click();
  await p.locator("[data-browser-step-open]").click();
  const n = state.posts;
  await p.locator("[data-browser-step-again]").click();
  await expect.poll(() => state.posts).toBe(n + 1);
  await p.locator("[data-browser-step-cancel]").click();
  await expect(p.locator("[data-browser-step]")).toHaveCount(0);
  await focus(p);
  await p.waitForTimeout(300);
  expect(state.posts).toBe(n + 1);
  await p.context().close();
});

test("a plain browser: Link still goes to GitHub in the tab, no step", async ({ browser }) => {
  const p = await visit(browser, `/p/${project}/settings`, false);
  await fakeRepos(p, { installed: false, posts: 0 });
  await p.reload();
  await p.locator("[data-link-repo]").fill("acme/site");
  await p.getByRole("button", { name: "Link", exact: true }).click();
  await expect(p).toHaveURL(INSTALL);
  expect(await opened(p)).toEqual([]);
  await p.context().close();
});

/** `/api/billing` and checkout, with the plan Stripe has set so far. */
async function fakeBilling(p: Page, state: { plan: string; checkouts: number }) {
  await p.context().route("**/api/billing", (r) =>
    r.fulfill({ json: { billing: true, plan: state.plan, relay: { bytes: 0, allowance: 1e9, warning: false, slowed: false }, sandbox_minutes: 0, teams: [] } }),
  );
  await p.context().route("**/api/billing/checkout", (r) => (state.checkouts++, r.fulfill({ json: { url: CHECKOUT } })));
}

test("the app: Upgrade opens checkout in the browser, and the new plan shows when you're back", async ({ browser }) => {
  const p = await visit(browser, "/settings", true);
  const state = { plan: "personal", checkouts: 0 };
  await fakeBilling(p, state);
  await p.reload();
  await p.locator("[data-open-plan]").click();
  await p.getByRole("button", { name: /^Add a payment method/ }).click();
  const open = p.locator("[data-browser-step-open]");
  // The address is fetched before the click, so the open is a plain click's.
  await expect(open).toBeEnabled();
  await expect(open).toHaveText("Continue to checkout ↗");
  expect(state.checkouts).toBe(1);
  await open.click();
  expect(await opened(p)).toEqual([[CHECKOUT, "_blank", "noopener"]]);

  await focus(p);
  await expect(p.locator("[data-browser-step-waiting]")).toBeVisible();
  state.plan = "paid";
  await focus(p);
  await expect(p.locator("[data-plan-now]")).toHaveText("You're on a paid plan.");
  await expect(p.locator("[data-browser-step]")).toHaveCount(0);
  await p.context().close();
});

test("the app with open_outside: Upgrade asks the app on its own, the page opens nothing, and Open again asks again (#381)", async ({ browser }) => {
  const p = await visit(browser, "/settings", true, true);
  await fakeBilling(p, { plan: "personal", checkouts: 0 });
  await p.reload();
  await p.locator("[data-open-plan]").click();
  await p.getByRole("button", { name: /^Add a payment method/ }).click();
  // No open button and no second click: the waiting state is up.
  await expect(p.locator("[data-browser-step-waiting]")).toBeVisible();
  await expect(p.locator("[data-browser-step-open]")).toHaveCount(0);
  expect(await asked(p)).toEqual([["open_outside", { url: CHECKOUT }]]);
  expect(await opened(p)).toEqual([]);

  // A focus doesn't open it again; Open again does.
  await focus(p);
  await p.waitForTimeout(300);
  expect(await asked(p)).toHaveLength(1);
  await p.locator("[data-browser-step-again-open]").click();
  await expect.poll(() => asked(p)).toEqual([
    ["open_outside", { url: CHECKOUT }],
    ["open_outside", { url: CHECKOUT }],
  ]);
  expect(await opened(p)).toEqual([]);
  await p.context().close();
});

test("an app without open_outside: nothing opens on its own, and the open button's click does (#381)", async ({ browser }) => {
  const p = await visit(browser, "/settings", true, "old");
  await fakeBilling(p, { plan: "personal", checkouts: 0 });
  await p.reload();
  await p.locator("[data-open-plan]").click();
  await p.getByRole("button", { name: /^Add a payment method/ }).click();
  const open = p.locator("[data-browser-step-open]");
  await expect(open).toBeEnabled();
  await expect(p.locator("[data-browser-step-waiting]")).toHaveCount(0);
  // The auto-open asked once and was refused; the page opened nothing.
  expect(await asked(p)).toEqual([["open_outside", { url: CHECKOUT }]]);
  expect(await opened(p)).toEqual([]);
  await open.click();
  await expect(p.locator("[data-browser-step-waiting]")).toBeVisible();
  // The click asks again, is refused again, and opens the old way.
  await expect.poll(() => opened(p)).toEqual([[CHECKOUT, "_blank", "noopener"]]);
  await p.context().close();
});

test("a plain browser: Upgrade still goes to checkout in the tab, no step", async ({ browser }) => {
  const p = await visit(browser, "/settings", false);
  await fakeBilling(p, { plan: "personal", checkouts: 0 });
  await p.reload();
  await p.locator("[data-open-plan]").click();
  await p.getByRole("button", { name: /^Add a payment method/ }).click();
  await expect(p).toHaveURL(CHECKOUT);
  expect(await opened(p)).toEqual([]);
  await p.context().close();
});
