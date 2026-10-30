// Live presence's switch (Arugula #341). Alice owns team Acme with chat on.
// Without the `live-presence` Labs flag neither page shows the switch. With
// it, both do, at Off. Watching chosen on control's page is the platform
// page's too (it's kept on the account). Live is the window's own: chosen
// on the platform page, control's page still says Watching. Off on one
// page is off on the other. A local control (team-fixture.ts,
// `--platform-host app.localhost`), as boards-platform.spec.ts.

import { expect, test, type Page } from "@playwright/test";
import { closeContexts } from "./helpers";
import { TeamControl } from "./team-fixture";

const t = new TeamControl("live-presence");

test.describe.configure({ mode: "serial" });
test.afterAll(async ({ browser }) => {
  await closeContexts({ browser });
  t.stop();
});

let alice: Page;
let platform: Page;
let team = "";
let app = "";

const sw = (p: Page) => p.locator("[data-live-switch]");

/** Choose one of the switch's three from the header's menu. */
async function choose(p: Page, label: "Off" | "Watching" | "Live (my camera)") {
  await sw(p).click();
  await p.getByRole("menuitemradio", { name: label, exact: true }).click();
}

/** The switch's state after a reload: what control kept, as a page reads it. */
async function afterReload(p: Page) {
  await p.reload();
  await expect(sw(p)).toBeVisible({ timeout: 15_000 });
  return sw(p).getAttribute("data-live-switch");
}

test("without its flag, neither page has the switch", async ({ browser }) => {
  test.setTimeout(120_000);
  await t.start(["--platform-host", "app.localhost"]);
  app = t.base.replace("//127.0.0.1", "//app.localhost");
  alice = await t.person(browser, "alice");
  await alice.evaluate(() => window.__arugula.control!.createTeam("Acme"));
  team = await alice.evaluate(() => window.__arugula.control!.teams[0].team);
  await alice.evaluate((tm) => window.__arugula.control!.setLabsFlag("chat", true, tm), team);
  await alice.reload();
  // The account menu's presence lines are there (chat is on), live's aren't.
  await alice.locator("[data-account-menu]").click();
  await expect(alice.getByRole("menuitemradio", { name: "Appear offline" })).toBeVisible({ timeout: 15_000 });
  await expect(alice.getByRole("menuitemradio", { name: "Watching", exact: true })).toHaveCount(0);
  await alice.keyboard.press("Escape");
  await expect(sw(alice)).toHaveCount(0);

  platform = await alice.context().newPage();
  await platform.goto(`${app}/`);
  await expect(platform.locator("[data-platform-bar]")).toBeVisible({ timeout: 15_000 });
  await expect(sw(platform)).toHaveCount(0);
});

test("with it, both pages say Off; Watching on one is the other's too", async () => {
  await alice.evaluate((tm) => window.__arugula.control!.setLabsFlag("live-presence", true, tm), team);
  await expect.poll(() => afterReload(alice), { timeout: 30_000 }).toBe("off");
  await choose(alice, "Watching");
  await expect(sw(alice)).toHaveAttribute("data-live-switch", "watching");
  await expect.poll(() => afterReload(platform), { timeout: 30_000 }).toBe("watching");
  // The account menu offers the same three, Watching checked.
  await platform.locator(".platform-me").click();
  await expect(platform.getByRole("menuitemradio", { name: "Watching", exact: true })).toHaveAttribute("aria-checked", "true");
  await platform.keyboard.press("Escape");
});

test("Live is the window's own, and Off on one page is off on the other", async () => {
  await choose(platform, "Live (my camera)");
  await expect(sw(platform)).toHaveAttribute("data-live-switch", "live");
  // Control's page: still Watching (its camera is its own).
  expect(await afterReload(alice)).toBe("watching");
  // A reload ends Live in its window, and leaves Watching.
  expect(await afterReload(platform)).toBe("watching");
  await choose(platform, "Off");
  await expect(sw(platform)).toHaveAttribute("data-live-switch", "off");
  await expect.poll(() => afterReload(alice), { timeout: 30_000 }).toBe("off");
});
