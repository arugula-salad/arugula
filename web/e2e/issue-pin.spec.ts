// A project's own issue as an issue block (M203): a pin onto the
// platform's issue, drawn by the platform page's issue view. Alice owns
// team Acme (chat on, so issues are) and the machine geek. She opens one of
// Acme's issues as a block in geek's workspace on control's page, where the
// view reads as her: no agent grant needed (the daemon's own read, which
// needs one, says so, and reads it once she gives one). The same issue on a phone (control's page, 390 wide)
// and on the platform page is the same view, and a change in any of the
// three shows in the others: the title from the platform page, a comment
// from the phone, closing it from the block.
// A local control (team-fixture.ts, `--platform-host app.localhost`).

import { expect, test, type Page } from "@playwright/test";
import { closeContexts } from "./helpers";
import { TeamControl, call, home, live, show } from "./team-fixture";

const t = new TeamControl("issuepin");

test.describe.configure({ mode: "serial" });
test.afterAll(async ({ browser }) => {
  await closeContexts({ browser });
  t.stop();
});

let alice: Page;
let phone: Page;
let platform: Page;
let app = "";
let issue = "";
let block = 0;

const TITLE = "Search misses hyphenated words";

/** Acme's project, and an issue in it, through the platform's routes. */
async function newIssue(p: Page): Promise<string> {
  return p.evaluate(async (title) => {
    const projects = (await (await fetch("/api/projects")).json()) as { projects: { id: string; team: unknown }[] };
    const acme = projects.projects.find((x) => x.team)!.id;
    const r = await fetch(`/api/projects/${acme}/issues`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ title, body: "Searching for dry-run finds nothing.", labels: ["bug"] }),
    });
    return ((await r.json()) as { id: string }).id;
  }, TITLE);
}

const pinned = (p: Page) => p.locator(`[data-issue-block="${issue}"]`);

test("an issue opens as a block in geek's workspace, read as Alice", async ({ browser }) => {
  test.setTimeout(240_000);
  await t.start(["--platform-host", "app.localhost"]);
  app = t.base.replace("//127.0.0.1", "//app.localhost");
  alice = await t.person(browser, "alice");
  await alice.evaluate(() => window.__arugula.control!.createTeam("Acme"));
  const team = await alice.evaluate(() => window.__arugula.control!.teams[0].team);
  await alice.evaluate((tm) => window.__arugula.control!.setLabsFlag("chat", true, tm), team);
  await t.addMachine(alice, "geek");
  await home(alice);
  await expect.poll(() => alice.evaluate(() => window.__arugula.chat.on), { timeout: 15_000 }).toBe(true);
  await expect.poll(() => live(alice, ["geek"]), { timeout: 30_000 }).toBe(true);
  await show(alice, "geek");
  issue = await newIssue(alice);
  expect(issue).toMatch(/^iss_[0-9a-f]{32}$/);

  // Beside geek's terminal, as `show` puts one.
  const beside = await alice.evaluate(() => window.__arugula.client.active()!);
  const opened = await call(alice, "POST", "/api/blocks", { type: "forge", config: { issue }, local: true, split: beside });
  expect(opened.status, JSON.stringify(opened.body)).toBe(200);
  block = (opened.body as { block: number }).block;
  const view = pinned(alice);
  await expect(view).toHaveAttribute("data-issue-reader", "me", { timeout: 15_000 });
  // ItemBlock's header and facts say the title, state and labels; the
  // old words (the `control` tag, "Read as you…" foot) are gone, and an
  // open issue nobody is waiting on has no next-step strip.
  await expect(view.locator("[data-item-head] .item-title")).toHaveText(TITLE);
  await expect(view.locator("[data-item-pill]")).toHaveText("Open");
  await expect(view.locator('[data-item-source="arugula"]')).toHaveAttribute("title", /read as you, through Arugula control/);
  await expect(view.locator('[data-item-fact="labels"] .item-label')).toHaveText(["bug"]);
  await expect(view.locator("[data-item-tab]")).toHaveText(["Activity"]);
  await expect(view.locator("[data-issue-body]")).toContainText("dry-run finds nothing");
  await expect(view.locator(".ws-tag")).toHaveCount(0);
  await expect(view.locator("[data-issue-foot]")).toHaveCount(0);
  await expect(view.locator("[data-item-next]")).toHaveCount(0);
  await expect(view).not.toContainText("Read as you");

  await view.screenshot({ path: `/tmp/410-shots/${process.env.SHOT ?? "after"}-open.png` });

  // The daemon reads it as the machine only with a grant, and says how.
  const st = await call(alice, "GET", `/api/blocks/${block}`);
  const state = (st.body as { state: { native: boolean; pin: { global_id: string }; error: string | null } }).state;
  expect(state.native).toBe(true);
  expect(state.pin.global_id).toBe(issue);
  expect(state.error ?? "").toContain("threads.read");
  // Given one over Acme, it reads the issue for its owner.
  const machine = await alice.evaluate(() => window.__arugula.fleet.host("geek")!.id!);
  const granted = await alice.evaluate(
    async ([m, i]) => {
      const issue = (await (await fetch(`/api/issues/${i}`)).json()) as { project: string };
      const r = await fetch("/api/grants", {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ daemon: m, power: "threads.read", scope: { kind: "project", id: issue.project } }),
      });
      return r.status;
    },
    [machine, issue] as const,
  );
  expect(granted).toBe(200);
  const read = await call(alice, "POST", `/api/blocks/${block}/call/refresh`, {});
  const shown = (read.body as { read: boolean; number: number | null; title?: unknown; issue?: unknown });
  // What everyone who sees the pane gets: its number, never its text.
  expect([shown.read, shown.number, shown.title, shown.issue]).toEqual([true, 1, undefined, undefined]);
  // The issue itself is the owner's, by its own call.
  const own = await call(alice, "POST", `/api/blocks/${block}/call/native_issue`, {});
  expect((own.body as { issue: { title: string } | null }).issue?.title).toBe(TITLE);

  // A comment from the block is a message in its thread.
  const box = view.locator(".thread-compose textarea");
  await box.fill("I can reproduce it on geek");
  await box.press("Enter");
  await expect(view.locator(".thread-msg").last()).toContainText("I can reproduce it on geek");
});

test("the phone and the platform page show the same view, and all three update live", async () => {
  test.setTimeout(180_000);
  // The phone: control's page, 390 wide, the block shown.
  phone = await alice.context().newPage();
  await phone.setViewportSize({ width: 390, height: 844 });
  await home(phone);
  await show(phone, "geek");
  await phone.evaluate((b) => window.__arugula.client.setActive(b), block);
  const onPhone = pinned(phone);
  await expect(onPhone.locator(".item-title")).toHaveText(TITLE, { timeout: 15_000 });
  await expect(onPhone.locator(".thread-msg").last()).toContainText("I can reproduce it on geek");
  expect(await phone.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(true);

  // The platform page: the issue by its id.
  platform = await alice.context().newPage();
  await platform.goto(`${app}/issues/${issue}`);
  const there = platform.locator(`[data-issue="${issue}"]`);
  await expect(there.locator("[data-issue-title]")).toHaveText(TITLE, { timeout: 15_000 });
  await expect(there.locator(".thread-msg").last()).toContainText("I can reproduce it on geek");

  // The title, changed on the platform page: the block and the phone have it.
  await there.locator("[data-issue-edit-title]").click();
  await there.locator("[data-issue-title-edit]").fill("Search misses words with hyphens");
  await there.locator("[data-issue-title-edit]").press("Enter");
  await expect(there.locator("[data-issue-title]")).toHaveText("Search misses words with hyphens");
  await expect(pinned(alice).locator(".item-title")).toHaveText("Search misses words with hyphens", { timeout: 10_000 });
  await expect(onPhone.locator(".item-title")).toHaveText("Search misses words with hyphens", { timeout: 10_000 });

  // A comment from the phone: the platform page and the block have it.
  const box = onPhone.locator(".thread-compose textarea");
  await box.fill("still there on my phone");
  await box.press("Enter");
  await expect(there.locator(".thread-msg").last()).toContainText("still there on my phone", { timeout: 10_000 });
  await expect(pinned(alice).locator(".thread-msg").last()).toContainText("still there on my phone", { timeout: 10_000 });

  // Closed from the block: closed everywhere, with the event in the timeline.
  await pinned(alice).locator("[data-issue-close]").click();
  for (const v of [pinned(alice), onPhone]) {
    await expect(v.locator("[data-item-pill]")).toHaveText("Closed", { timeout: 10_000 });
    await expect(v.locator("[data-item-next]")).toHaveCount(0);
  }
  await expect(there.locator("[data-issue-state]")).toHaveText("Closed", { timeout: 10_000 });
  await pinned(alice).screenshot({ path: `/tmp/410-shots/${process.env.SHOT ?? "after"}-closed.png` });
  await expect(there.locator('[data-issue-event="closed"]')).toBeVisible();
  await expect(onPhone.locator('[data-issue-event="closed"]')).toBeVisible({ timeout: 10_000 });
});
