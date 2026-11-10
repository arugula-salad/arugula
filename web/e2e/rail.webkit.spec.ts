// #376: the panes page's rail in WebKit. Projects is its first place, above
// Panes, and a click lands on the platform page in the same tab (the app's
// and an installed PWA's way; a plain browser's own tab is the platform
// links' spec). `Projects ↗` is no longer in the top bar.

import { expect, test, type Page } from "@playwright/test";
import { closeContexts } from "./helpers";
import { TeamControl, home, opened, show } from "./team-fixture";

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
  await expect(projects).toHaveAttribute("href", `${app}/?back`);
  // The way to the platform page is the rail's now, not the bar's.
  await expect(alice.locator("[data-open-projects]")).toHaveCount(1);
  // Docs is beside it, after Chat: the platform's /docs, in a tab of its own (Arugula #418).
  const docs = alice.locator(".ws-rail [data-open-docs-all]");
  await expect(docs).toHaveCount(1);
  await expect(docs).toHaveAttribute("href", `${app}/docs`);
  await expect(docs).toHaveAttribute("target", "arugula-platform");
  await expect(docs).not.toHaveClass(/\bhere\b/);
  await expect(alice.getByText("Projects ↗")).toHaveCount(0);
  await expect(alice.locator(".ws-head, .bar").locator("a", { hasText: "Projects" })).toHaveCount(0);
});

// Arugula #796: a machine's Docs is the rail's Docs, under the panes' header.
test("on the machine's Docs the rail lights Docs, not Panes, and the header is the panes'", async () => {
  await alice.locator("[data-machine-docs]").click();
  await expect(alice.locator("[data-docs-page]")).toBeVisible();
  await expect(alice.locator(".ws-rail [data-open-docs-all]")).toHaveAttribute("aria-current", "page");
  await expect(alice.locator(".ws-rail [data-open-panes]")).not.toHaveAttribute("aria-current", "page");
  await expect(alice.locator(".app-bar")).toHaveCount(0);
  await expect(alice.locator(".ws-head [data-machine-docs]")).toHaveAttribute("aria-current", "page");
  await alice.locator(".ws-head [data-machine-panes]").click();
  await expect(alice.locator("[data-docs-page]")).toHaveCount(0);
  await expect(alice.locator(".ws-rail [data-open-panes]")).toHaveAttribute("aria-current", "page");
  await expect(alice.locator(".ws-rail [data-open-docs-all]")).not.toHaveAttribute("aria-current", "page");
});

test("in a browser, Projects opens the platform page in a tab of its own", async () => {
  const [opened] = await Promise.all([alice.context().waitForEvent("page"), alice.locator(".ws-rail [data-open-projects]").click()]);
  await expect(opened).toHaveURL(`${app}/`, { timeout: 15_000 });
  await opened.close();
});

// #288: in the desktop app the platform page replaces this window.
test("in the app, Projects lands on the platform page in this window", async () => {
  // Playwright's Linux WebKit throws its own "Assertion error" when an open
  // page goes to another origin (#402), so the navigation is cancelled where
  // the app asks for it (the Navigation API's `navigate` event), and the
  // address it asked for opens in a new page.
  const shell = await alice.context().newPage();
  await shell.addInitScript(() => {
    Object.assign(window, { __arugulaApp: { name: "Arugula app on test-mac", signedIn: true } });
    (window as { __asked?: string }).__asked = undefined;
    (window as unknown as { navigation: EventTarget }).navigation.addEventListener("navigate", (e) => {
      const ev = e as Event & { destination: { url: string } };
      (window as { __asked?: string }).__asked = ev.destination.url;
      ev.preventDefault();
    });
  });
  await shell.goto(`${t.base}/`);
  const pages = alice.context().pages().length;
  await shell.locator(".ws-rail [data-open-projects]").click();
  // It asked for this window to go to the platform page, not for a tab of its own.
  await expect.poll(() => shell.evaluate(() => (window as { __asked?: string }).__asked)).toBeTruthy();
  const asked = new URL((await shell.evaluate(() => (window as { __asked?: string }).__asked))!);
  expect(asked.origin).toBe(app);
  expect(asked.pathname).toBe("/");
  expect(alice.context().pages().length).toBe(pages);
  const landed = await opened(shell, asked.href);
  await expect(landed).toHaveURL((u) => u.origin === app);
  expect(alice.context().pages().length).toBe(pages + 1);
});
