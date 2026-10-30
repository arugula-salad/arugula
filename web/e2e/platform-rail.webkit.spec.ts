// #377: the platform page's rail in WebKit. The panes page's rail (#376) down
// its left at a computer's width, Chat lit on chat, Projects lit in a project,
// and Panes, a link to control's page, landing there in the same tab: the
// desktop app keeps one window per page (#288). The bar has no wordmark,
// Control or Chat of its own.

import { expect, test, type Page } from "@playwright/test";
import { closeContexts } from "./helpers";
import { TeamControl, home, show } from "./team-fixture";

const t = new TeamControl("rail377");

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

test("Alice, a team with chat on, a machine, and the platform page", async ({ browser }) => {
  test.setTimeout(180_000);
  await t.start(["--platform-host", "app.localhost"]);
  app = t.base.replace("//127.0.0.1", "//app.localhost");
  alice = await t.person(browser, "alice");
  await alice.evaluate(() => window.__arugula.control!.createTeam("Acme"));
  const team = await alice.evaluate(() => window.__arugula.control!.teams[0].team);
  await alice.evaluate((tm) => window.__arugula.control!.setLabsFlag("chat", true, tm), team);
  await t.addMachine(alice, "alices");
  await home(alice);
  await show(alice, "alices");
  await alice.goto(`${app}/`);
  await expect(alice.locator("[data-platform-bar]")).toBeVisible({ timeout: 15_000 });
});

test("the rail is down the platform page's left, its places in order, and the bar has no Control, wordmark or Chat", async () => {
  const rail = alice.locator(".ws-rail");
  await expect(rail).toBeVisible();
  const labels = await rail.locator(".ws-rail-button .ws-rail-label").allTextContents();
  expect(labels.slice(0, 3)).toEqual(["Projects", "Panes", "Swarm"]);
  expect(labels.at(-1)).toBe("Settings");
  for (const hook of ["projects", "panes", "swarm", "chat", "docs"]) await expect(rail.locator(`[data-open-${hook}]`)).toHaveCount(1);
  const box = (await rail.boundingBox())!;
  expect(box.x).toBe(0);
  expect(box.y).toBe(0);
  expect(box.height).toBeGreaterThan(300);
  // Beside the bar, not above it.
  expect((await alice.locator("[data-platform-bar]").boundingBox())!.x).toBeGreaterThanOrEqual(box.width);
  const bar = alice.locator("[data-platform-bar]");
  await expect(bar.locator("[data-platform-shell]")).toHaveCount(0);
  await expect(bar.locator("[data-platform-wordmark]")).toHaveCount(0);
  await expect(bar.locator("[data-platform-chat]")).toHaveCount(0);
  // The project switcher still says where you are.
  await expect(bar.locator("[data-project-switcher]")).toContainText("All projects");
  await expect(rail.locator("[data-open-projects]")).toHaveAttribute("aria-current", "page");
});

test("Chat is lit on chat, and Projects is not", async () => {
  await alice.locator(".ws-rail [data-open-chat]").click();
  await expect(alice.locator("[data-chat-control]")).toBeVisible({ timeout: 15_000 });
  await expect(alice).toHaveURL(/\/p\/prj_[0-9a-f]+-acme\/chat/);
  const chat = alice.locator(".ws-rail [data-open-chat]");
  await expect(chat).toHaveClass(/\bhere\b/);
  await expect(chat).toHaveAttribute("aria-current", "page");
  await expect(alice.locator(".ws-rail [data-open-projects]")).not.toHaveClass(/\bhere\b/);
  // The rail is on the chat page too, in the same place.
  expect((await alice.locator(".ws-rail").boundingBox())!.x).toBe(0);
  await expect(alice.locator("[data-platform-bar] [data-platform-chat]")).toHaveCount(0);
});

test("Projects is lit inside a project, and takes you back to the list", async () => {
  await alice.goto(`${app}/`);
  await alice.locator("section[data-platform-home] [data-project]", { hasText: "Acme" }).click();
  await expect(alice).toHaveURL(/\/p\/prj_[0-9a-f]+-acme$/);
  const projects = alice.locator(".ws-rail [data-open-projects]");
  await expect(projects).toHaveClass(/\bhere\b/);
  await expect(alice.locator(".ws-rail [data-open-chat]")).not.toHaveClass(/\bhere\b/);
  // The bar keeps the project's places and its own Settings.
  await expect(alice.locator("[data-platform-bar] [data-platform-home]")).toBeVisible();
  await expect(alice.locator("[data-platform-bar] [data-platform-settings]")).toBeVisible();
  // In the page: no reload.
  await alice.evaluate(() => Object.assign(window, { stayed: true }));
  await projects.click();
  await expect(alice).toHaveURL(`${app}/`);
  expect(await alice.evaluate(() => (window as unknown as { stayed?: boolean }).stayed)).toBe(true);
});

test("Panes lands on control's page in this tab, with the rail in the same spot, and Chat goes back", async () => {
  await alice.goto(`${app}/`);
  await expect(alice.locator("[data-platform-bar]")).toBeVisible({ timeout: 15_000 });
  const panes = alice.locator(".ws-rail [data-open-panes]");
  expect(await panes.getAttribute("target")).toBeNull();
  const spot = (await alice.locator(".ws-rail").boundingBox())!;
  const pages = alice.context().pages().length;
  await panes.click();
  await alice.waitForURL((u) => u.origin === t.base, { timeout: 15_000 });
  expect(alice.context().pages().length).toBe(pages);
  await expect(alice.locator(".ws-rail")).toBeVisible({ timeout: 15_000 });
  const there = (await alice.locator(".ws-rail").boundingBox())!;
  expect({ x: there.x, y: there.y, w: there.width }).toEqual({ x: spot.x, y: spot.y, w: spot.width });
  // Back to Chat from control's rail: a browser opens the platform page in its tab.
  const [chatPage] = await Promise.all([alice.context().waitForEvent("page"), alice.locator(".ws-rail [data-open-chat]").click()]);
  await expect(chatPage.locator(".ws-rail [data-open-chat]")).toHaveClass(/\bhere\b/, { timeout: 15_000 });
  expect(new URL(chatPage.url()).origin).toBe(app);
  await chatPage.close();
});
