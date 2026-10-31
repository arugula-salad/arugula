// #376: the panes page's rail in WebKit. Projects is its first place, above
// Panes, and a click lands on the platform page in the same tab (the app's
// and an installed PWA's way; a plain browser's own tab is the platform
// links' spec). `Projects ↗` is no longer in the top bar.

import { expect, test, type Page } from "@playwright/test";
import { closeContexts } from "./helpers";
import { TeamControl, home, show } from "./team-fixture";

const t = new TeamControl("rail376");

test.describe.configure({ mode: "serial" });
// Playwright's WebKit stops loading the page once the service worker is
// registered (real Safari doesn't), so none here.
test.use({ serviceWorkers: "block" });
test.afterAll(async ({ browser }) => {
  await closeContexts({ browser });
  t.stop();
});

let alice: Page;
let app = "";

test("Alice, a team and a machine, on a control with a platform page", async ({ browser }) => {
  test.setTimeout(180_000);
  await t.start(["--platform-host", "app.localhost"]);
  app = t.base.replace("//127.0.0.1", "//app.localhost");
  alice = await t.person(browser, "alice");
  await alice.evaluate(() => window.__arugula.control!.createTeam("Acme"));
  await t.addMachine(alice, "alices");
  await home(alice);
  await show(alice, "alices");
});

test("Projects is the rail's first place, above Panes, and the bar has no Projects ↗", async () => {
  const places = alice.locator(".ws-rail [data-open-projects], .ws-rail [data-open-panes]");
  await expect(places).toHaveCount(2);
  const order = await alice.locator(".ws-rail .ws-rail-button").evaluateAll((els) => els.map((e) => e.querySelector(".ws-rail-label")!.textContent));
  expect(order.slice(0, 2)).toEqual(["Projects", "Panes"]);
  const projects = alice.locator(".ws-rail [data-open-projects]");
  const panes = alice.locator(".ws-rail [data-open-panes]");
  const [p, n] = await Promise.all([projects.boundingBox(), panes.boundingBox()]);
  expect(p!.y).toBeLessThan(n!.y);
  await expect(projects).toHaveAttribute("href", `${app}/`);
  // The way to the platform page is the rail's now, not the bar's.
  await expect(alice.locator("[data-open-projects]")).toHaveCount(1);
  await expect(alice.getByText("Projects ↗")).toHaveCount(0);
  await expect(alice.locator(".ws-head, .bar").locator("a", { hasText: "Projects" })).toHaveCount(0);
});

test("in a browser, Projects opens the platform page in a tab of its own", async () => {
  const [opened] = await Promise.all([alice.context().waitForEvent("page"), alice.locator(".ws-rail [data-open-projects]").click()]);
  await expect(opened).toHaveURL(`${app}/`, { timeout: 15_000 });
  await opened.close();
});

// #288: in the desktop app the platform page replaces this window.
test("in the app, Projects lands on the platform page in this window", async () => {
  // macOS's WebKit only for now: Playwright's Linux WebKit throws its own
  // "Assertion error" when a page goes to the platform page's origin (#402).
  test.skip(process.platform === "linux", "Playwright's Linux WebKit asserts on a cross-origin navigation (#402)");
  const shell = await alice.context().newPage();
  await shell.addInitScript(() => Object.assign(window, { __arugulaApp: { name: "Arugula app on test-mac", signedIn: true } }));
  await shell.goto(`${t.base}/`);
  const pages = alice.context().pages().length;
  await shell.locator(".ws-rail [data-open-projects]").click();
  await shell.waitForURL((u) => u.origin === app, { timeout: 15_000 });
  expect(new URL(shell.url()).pathname).toBe("/");
  expect(alice.context().pages().length).toBe(pages);
});
