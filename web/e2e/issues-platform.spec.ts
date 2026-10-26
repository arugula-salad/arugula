// Native issues on the platform page (M201): a project's list, a new
// issue, its thread (M196) and its timeline (M194) in one column, and
// closing it, by role. Alice owns team Acme, with chat's Labs flag on
// (issues come with it); Bob is a viewer. Alice opens an issue and
// comments; Bob, who can't open one, reads it on a phone, comments, and
// Alice has it over the live stream; Alice closes it as not planned and
// the timeline says so. What's new in an issue's thread is on its row and
// the Issues place, and in the inbox's New in threads, never on the Chat
// button (chat doesn't list the thread); opening the issue, or Done in the
// inbox, reads it. A local control (team-fixture.ts, `--platform-host
// app.localhost`), as platform.spec.ts runs it.

import { expect, test, type Page } from "@playwright/test";
import { closeContexts } from "./helpers";
import { TeamControl, home } from "./team-fixture";

const t = new TeamControl("issues");

test.describe.configure({ mode: "serial" });
test.afterAll(async ({ browser }) => {
  await closeContexts({ browser });
  t.stop();
});

let alice: Page;
let bob: Page;
let team = "";
let app = "";
let issuePath = "";

async function platform(p: Page, path = "/") {
  await p.goto(`${app}${path}`);
  await expect(p.locator("[data-platform-bar], [data-chat]").first()).toBeVisible({ timeout: 15_000 });
}

test("a team's project lists its issues, and an owner opens one", async ({ browser }) => {
  test.setTimeout(180_000);
  await t.start(["--platform-host", "app.localhost"]);
  app = t.base.replace("//127.0.0.1", "//app.localhost");
  alice = await t.person(browser, "alice");
  await alice.evaluate(() => window.__arugula.control!.createTeam("Acme"));
  team = await alice.evaluate(() => window.__arugula.control!.teams[0].team);
  await alice.evaluate((tm) => window.__arugula.control!.setLabsFlag("chat", true, tm), team);
  // Bob, a viewer.
  const link = await alice.evaluate((tm) => window.__arugula.control!.invite(tm, "viewer", true), team);
  bob = await t.person(browser, "bob", { viewport: { width: 390, height: 844 }, isMobile: true, hasTouch: true });
  await bob.goto(link);
  await bob.locator("[data-accept-invite]").click();
  await alice.evaluate(() => window.__arugula.control!.refresh());
  await expect(alice.locator("[data-admit-yes]")).toBeVisible({ timeout: 15_000 });
  await alice.locator("[data-admit-yes]").click();
  await expect.poll(() => alice.evaluate(() => window.__arugula.control!.teams[0].roster.members.length), { timeout: 15_000 }).toBe(2);

  await platform(alice);
  await alice.locator("section[data-platform-home] [data-project]", { hasText: "Acme" }).click();
  await alice.locator("[data-platform-issues]").click();
  await expect(alice).toHaveURL(/\/p\/prj_[0-9a-f]+-acme\/issues$/);
  await expect(alice.locator("[data-issues-none]")).toContainText("No open issues");

  await alice.locator("[data-issue-new]").click();
  await alice.locator("[data-issue-title-input]").fill("Search misses hyphenated words");
  await alice.locator("[data-issue-body-input]").fill("Searching for dry-run finds nothing.");
  // Labels with the issue page's picker (UI audit #163): New label… twice.
  for (const label of ["bug", "search"]) {
    await alice.locator("[data-issue-labels-pick]").click();
    await alice.getByRole("menuitem", { name: "New label…" }).click();
    await alice.locator(".prompt input").fill(label);
    await alice.locator(".prompt input").press("Enter");
  }
  await expect(alice.locator("[data-issue-labels-input] [data-issue-label]")).toHaveText(["bug", "search"]);
  await alice.locator("[data-issue-create]").click();
  await expect(alice).toHaveURL(/\/p\/prj_[0-9a-f]+-acme\/issues\/iss_[0-9a-f]{32}-search-misses-hyphenated-words$/, { timeout: 15_000 });
  issuePath = new URL(alice.url()).pathname;
  const view = alice.locator("[data-issue]");
  await expect(view.locator("[data-issue-title]")).toHaveText("Search misses hyphenated words");
  await expect(view.locator("[data-issue-state]")).toHaveText("Open");
  await expect(view.locator("[data-issue-labels] [data-issue-label]")).toHaveText(["bug", "search"]);
  await expect(view.locator("[data-issue-body]")).toContainText("dry-run finds nothing");

  // A comment is a message in the issue's thread.
  const box = view.locator(".thread-compose textarea");
  await box.fill("I can reproduce it");
  await box.press("Enter");
  await expect(view.locator(".thread-msg").last()).toContainText("I can reproduce it");

  // The list has it, and finds it by a word.
  await alice.locator("[data-issue-back]").click();
  await expect(alice.locator("[data-issue-row]")).toHaveCount(1);
  await alice.locator("[data-issues-search]").fill("hyphenated");
  await expect(alice).toHaveURL(/q=hyphenated/);
  await expect(alice.locator("[data-issue-row]")).toHaveCount(1);
  await alice.locator("[data-issues-search]").fill("nothinglikeit");
  await expect(alice.locator("[data-issues-none]")).toContainText("No open issues match", { timeout: 10_000 });
});

test("a viewer reads and comments on a phone, and can't open or close one", async () => {
  test.setTimeout(90_000);
  await home(bob);
  await platform(bob, issuePath);
  const view = bob.locator("[data-issue]");
  await expect(view.locator("[data-issue-title]")).toHaveText("Search misses hyphenated words", { timeout: 15_000 });
  await expect(view.locator(".thread-msg").last()).toContainText("I can reproduce it");
  await expect(view.locator("[data-issue-close], [data-issue-edit-title], [data-issue-labels-edit]")).toHaveCount(0);
  // The phone layout: nothing wider than the screen.
  const wide = await bob.evaluate(() => {
    const main = document.querySelector(".platform-main")!;
    return main.scrollWidth - main.clientWidth;
  });
  expect(wide).toBeLessThanOrEqual(0);
  await view.locator(".thread-compose textarea").fill("Same here on the phone");
  await view.locator(".thread-compose textarea").press("Enter");
  await expect(view.locator(".thread-msg").last()).toContainText("Same here on the phone");
  // Alice has it with no reload: a dot on its row and a count on Issues.
  await alice.locator("[data-issues-search]").fill("");
  await expect(alice.locator("[data-issue-row] [data-thread-unread]")).toHaveAttribute("data-thread-unread", "1", { timeout: 15_000 });
  await expect(alice.locator("[data-platform-issues] [data-place-unread]")).toHaveAttribute("data-place-unread", "1");
  // Control's page has the thread unread too, and its Chat button doesn't
  // count it: chat doesn't list an issue's thread.
  const shell = await alice.context().newPage();
  await home(shell);
  await expect
    .poll(() => shell.evaluate(() => window.__arugula.chat.rooms.find((r) => r.kind === "object")?.unread ?? -1), { timeout: 15_000 })
    .toBe(1);
  expect(await shell.evaluate(() => window.__arugula.chat.unread())).toEqual({ n: 0, mention: false });
  await expect(shell.locator(".ws-rail [data-open-chat] .ws-rail-count")).toHaveCount(0);
  await shell.close();
  // Opening the issue reads its thread.
  await alice.locator("[data-issue-row] a").first().click();
  await expect(alice.locator("[data-issue] .thread-msg").last()).toContainText("Same here on the phone", { timeout: 15_000 });
  await alice.locator("[data-issue-back]").click();
  await expect(alice.locator("[data-issue-row]")).toHaveCount(1);
  await expect(alice.locator("[data-issue-row] [data-thread-unread], [data-platform-issues] [data-place-unread]")).toHaveCount(0);
  await alice.locator("[data-issue-row] a").first().click();
  await expect(alice.locator("[data-issue] [data-issue-title]")).toHaveText("Search misses hyphenated words");

  // A viewer has no New issue, and the new issue page says why.
  await platform(bob, issuePath.replace(/\/issues\/.*$/, "/issues"));
  await expect(bob.locator("[data-issue-row]")).toHaveCount(1, { timeout: 15_000 });
  await expect(bob.locator("[data-issue-new]")).toHaveCount(0);
  await platform(bob, issuePath.replace(/\/issues\/.*$/, "/issues/new"));
  await expect(bob.locator("[data-issue-new-viewer]")).toBeVisible();
});

test("the owner closes it as not planned, and the timeline and the lists say so", async () => {
  const view = alice.locator("[data-issue]");
  await view.locator("[data-issue-close-as]").click();
  await alice.getByRole("menuitem", { name: /Not planned/ }).click();
  await expect(view.locator("[data-issue-state]")).toHaveText("Closed: not planned");
  await expect(view.locator('[data-issue-event="closed"]')).toContainText("alice closed this as not planned");
  // The event sits after the comments it came after, in one column.
  const order = await view.locator(".thread-msg, [data-issue-event]").evaluateAll((els) => els.map((e) => (e.hasAttribute("data-issue-event") ? "event" : "msg")));
  expect(order).toEqual(["msg", "msg", "event"]);
  await alice.locator("[data-issue-back]").click();
  await expect(alice.locator("[data-issues-none]")).toContainText("No open issues");
  await alice.locator("[data-issues-closed]").click();
  await expect(alice).toHaveURL(/state=closed/);
  await expect(alice.locator("[data-issue-row]")).toHaveCount(1);
  await alice.locator("[data-issue-row] a").first().click();
  await view.locator("[data-issue-reopen]").click();
  await expect(view.locator("[data-issue-state]")).toHaveText("Open");
  await expect(view.locator('[data-issue-event="reopened"]')).toBeVisible();
});

test("what's new in its thread is in the inbox, and Done there reads it", async () => {
  test.setTimeout(90_000);
  await alice.locator("[data-issue-back]").click();
  await expect(alice.locator("[data-issue-row]")).toHaveCount(1);
  await platform(bob, issuePath);
  const box = bob.locator("[data-issue] .thread-compose textarea");
  await box.fill("It finds dry run without the hyphen");
  await box.press("Enter");
  await expect(bob.locator("[data-issue] .thread-msg").last()).toContainText("without the hyphen");

  await expect(alice.locator("[data-issue-row] [data-thread-unread]")).toHaveAttribute("data-thread-unread", "1", { timeout: 15_000 });
  await alice.locator("[data-platform-inbox]").click();
  const card = alice.locator('[data-inbox-group="threads"] [data-inbox-thread]');
  await expect(card).toHaveCount(1, { timeout: 15_000 });
  await expect(card).toContainText("Search misses hyphenated words");
  await expect(card).toContainText("1 new in its thread");
  await card.locator("[data-inbox-thread-done]").click();
  await expect(alice.locator('[data-inbox-group="threads"]')).toHaveCount(0);

  // The list agrees, and so does control after a reload. (The inbox is no
  // project's, so the bar's Issues there is Alice's own project's.)
  await platform(alice, issuePath.replace(/\/issues\/.*$/, "/issues"));
  await expect(alice.locator("[data-issue-row]")).toHaveCount(1, { timeout: 15_000 });
  await expect(alice.locator("[data-issue-row] [data-thread-unread], [data-platform-issues] [data-place-unread]")).toHaveCount(0);
  await alice.reload();
  await expect(alice.locator("[data-issue-row]")).toHaveCount(1, { timeout: 15_000 });
  await expect(alice.locator("[data-issue-row] [data-thread-unread], [data-platform-issues] [data-place-unread]")).toHaveCount(0);
});
