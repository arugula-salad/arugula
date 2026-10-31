// Issues on boards, phase 1 (#289-#292), as a person meets them. Alice owns
// team Acme, with chat's and boards' Labs flags on, and three native issues,
// one hers. The bar says Triage (not Issues) and Boards, and Triage lists the
// three. She makes the board Launch, which has the four lanes (Next, Doing,
// Review, Done) and no way to make a zone; Add to board… on the first issue
// leaves it off Triage and draws it as a card in Next. She adds a row
// through the item picker (type, ArrowDown, Enter), edits only its note, and
// makes an issue with New issue…. Bob, a viewer on a phone, sees what's
// still untriaged, no Add to board…, and the board's cards with no sideways
// scroll. A local control
// (team-fixture.ts, `--platform-host app.localhost`), as issues-platform.spec.ts.
// Phase 2 (#302), epics: Alice sets Launch's goal, closes one of its issues
// and sees its row move to Done, not yet proven, and proven once evidence is
// attached (aru-125: lanes follow the work, and Done has no PROOFS fold),
// closes the board with a two-line note (Done
// and the note show, the Boards page folds it under "Done (1)") and reopens
// it; Bob sees the goal, progress and the Done banner and none of the buttons.
// #339: `/boards` lists every project's boards, by project, folded the same.
// Groups (aru-125, decision 6): a board whose author made groups shows each as
// a section with its progress and the four lanes of its rows under it, the
// rows with none last; a board without any looks as it did.

import { expect, test, type Locator, type Page } from "@playwright/test";
import { closeContexts } from "./helpers";
import { TeamControl, home } from "./team-fixture";

const t = new TeamControl("boards");

test.describe.configure({ mode: "serial" });
test.afterAll(async ({ browser }) => {
  await closeContexts({ browser });
  t.stop();
});

let alice: Page;
let bob: Page;
let team = "";
let app = "";
let project = "";
let boardPath = "";

const FIRST = "Fix the installer";
const SECOND = "Tidy the changelog";
const THIRD = "Update the screenshots";
const NEW = "Write the launch note";

async function platform(p: Page, path = "/") {
  await p.goto(`${app}${path}`);
  await expect(p.locator("[data-platform-bar], [data-chat]").first()).toBeVisible({ timeout: 15_000 });
}

const GOAL = "People can launch Acme in one click.\nThen invite their team.";
const NOTE = "A person can install Acme and invite a team.\nThe changelog is current.";

/** The platform's API as whoever `p` is signed in as. */
async function api(p: Page, method: string, path: string, body?: unknown) {
  return p.evaluate(
    async ([m, u, b]) => {
      const r = await fetch(u as string, { method: m as string, credentials: "same-origin", headers: { "Content-Type": "application/json" }, body: b === undefined ? undefined : JSON.stringify(b) });
      return { status: r.status, body: (await r.json().catch(() => null)) as Record<string, any> | null };
    },
    [method, path, body] as const,
  );
}

const zone = (p: Page, title: string): Locator => p.locator("[data-board-zone]").filter({ has: p.getByRole("heading", { name: title, exact: true }) });
const row = (p: Page, title: string): Locator => p.locator("[data-board-row]").filter({ has: p.locator("[data-board-title]", { hasText: title }) });
const triaged = (p: Page, title: string): Locator => p.locator("[data-triage-row]").filter({ hasText: title });

test("with boards on, the bar says Triage and Boards, and Triage lists the untriaged", async ({ browser }) => {
  test.setTimeout(180_000);
  await t.start(["--platform-host", "app.localhost"]);
  app = t.base.replace("//127.0.0.1", "//app.localhost");
  alice = await t.person(browser, "alice");
  await alice.evaluate(() => window.__arugula.control!.createTeam("Acme"));
  team = await alice.evaluate(() => window.__arugula.control!.teams[0].team);
  for (const flag of ["chat", "boards"]) await alice.evaluate(([f, tm]) => window.__arugula.control!.setLabsFlag(f, true, tm), [flag, team] as const);
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
  await expect(alice).toHaveURL(/\/p\/prj_[0-9a-f]+-acme$/);
  project = /\/p\/(prj_[0-9a-f]+)/.exec(alice.url())![1];

  // Three native issues, the first Alice's own.
  const me = (await api(alice, "GET", "/api/projects")).body!.me.account as string;
  for (const [title, assignees] of [[FIRST, [me]], [SECOND, []], [THIRD, []]] as const) {
    const made = await api(alice, "POST", `/api/projects/${project}/issues`, { title, body: "", labels: [], assignees });
    expect(made.status).toBe(200);
  }

  await platform(alice, alice.url().replace(app, ""));
  await expect(alice.locator("[data-platform-triage]")).toHaveText("Triage", { timeout: 15_000 });
  await expect(alice.locator("[data-platform-boards]")).toHaveText("Boards");
  await expect(alice.locator("[data-platform-issues]")).toHaveCount(0);

  await alice.locator("[data-platform-triage]").click();
  await expect(alice).toHaveURL(/\/p\/prj_[0-9a-f]+-acme\/triage$/);
  await expect(alice.locator("[data-triage-row]")).toHaveCount(3, { timeout: 15_000 });
  for (const title of [FIRST, SECOND, THIRD]) await expect(triaged(alice, title)).toBeVisible();
  await expect(triaged(alice, FIRST).locator(".triage-who")).toHaveText("alice");

  await alice.locator("[data-triage-all]").click();
  await expect(alice).toHaveURL(/\/p\/prj_[0-9a-f]+-acme\/issues$/);
  await expect(alice.locator("[data-issue-row]")).toHaveCount(3);
});

test("a board Launch has the four lanes and takes the first issue from Triage as a card in Next", async () => {
  test.setTimeout(90_000);
  await alice.locator("[data-platform-boards]").click();
  await expect(alice).toHaveURL(/\/p\/prj_[0-9a-f]+-acme\/boards$/);
  await alice.locator("[data-board-new-title]").fill("Launch");
  await alice.locator("[data-board-new]").click();
  await expect(alice.locator("[data-board-page] [data-board-title]")).toHaveText("Launch", { timeout: 15_000 });
  boardPath = new URL(alice.url()).pathname;

  // Always the four lanes, in order, and no way to make, rename or move a zone.
  await expect(alice.locator("[data-board-zone] h2")).toHaveText(["Next", "Doing", "Review", "Done"]);
  await expect(alice.locator("[data-board-add-zone], [data-board-add-proofs], [data-board-zone-menu]")).toHaveCount(0);

  await alice.locator("[data-platform-triage]").click();
  await expect(alice.locator("[data-triage-row]")).toHaveCount(3, { timeout: 15_000 });
  await triaged(alice, FIRST).locator("[data-triage-add]").click();
  await alice.getByRole("menuitem", { name: "Launch" }).click();
  await expect(triaged(alice, FIRST)).toHaveCount(0);
  await expect(alice.locator("[data-triage-row]")).toHaveCount(2);

  await alice.locator("[data-platform-boards]").click();
  await alice.locator("[data-board-link]", { hasText: "Launch" }).click();
  const card = row(alice, FIRST);
  await expect(zone(alice, "Next").locator(card)).toHaveCount(1, { timeout: 15_000 });
  await expect(card.locator(".board-card-title .board-title")).toHaveText(FIRST);
  await expect(card.locator("[data-board-ptr]")).toHaveText(/^#\d+$/);
  await expect(card.locator("[data-board-live]")).toHaveAttribute("data-board-live", "open");
  await expect(card.locator("[data-board-who]")).toHaveText("alice");
});

test("a row is added through the picker, and editing its note keeps what it points at", async () => {
  test.setTimeout(90_000);
  await alice.locator("[data-board-add-row]").click();
  const form = alice.locator("[data-board-form]");
  await form.locator("[data-board-ptr-input]").fill("changelog");
  const pick = form.locator("[data-board-pick]");
  await expect(pick).toHaveCount(1, { timeout: 10_000 });
  await expect(pick).toContainText(SECOND);
  await form.locator("[data-board-ptr-input]").press("ArrowDown");
  await form.locator("[data-board-ptr-input]").press("Enter");
  await expect(form.locator("[data-board-pick-list]")).toBeHidden();
  await expect(form.locator("[data-board-ptr-input]")).toHaveValue(/^#\d+$/);
  await form.locator("[data-board-save]").click();

  const card = row(alice, SECOND);
  await expect(card).toHaveCount(1, { timeout: 15_000 });
  await expect(card.locator("[data-board-ptr]")).toHaveText(/^#\d+$/);
  await expect(card.locator("[data-board-live]")).toHaveAttribute("data-board-live", "open");
  const ptr = await card.locator("[data-board-ptr]").textContent();

  // Only the note changes; the pointer isn't sent, so the row stays on the
  // issue. The row is found by its id: its title is gone while it's edited.
  const id = (await card.getAttribute("data-board-row"))!;
  const edited = alice.locator(`[data-board-row="${id}"]`);
  await card.locator("[data-board-row-menu]").click();
  await alice.getByRole("menuitem", { name: "Edit…" }).click();
  await edited.locator("[data-board-note-input]").fill("needs the 0.40 entry");
  await edited.locator("[data-board-save]").click();
  await expect(edited.locator("[data-board-note]")).toHaveText("needs the 0.40 entry", { timeout: 15_000 });
  await expect(edited.locator("[data-board-title]")).toHaveText(SECOND);
  await expect(edited.locator("[data-board-ptr]")).toHaveText(ptr!);
  await expect(edited.locator("[data-board-live]")).toHaveAttribute("data-board-live", "open");
});

test("New issue… makes a native issue and a card row for it in Next", async () => {
  test.setTimeout(90_000);
  const now = zone(alice, "Next");
  await alice.locator("[data-board-new-issue]").click();
  await alice.locator(".prompt input").fill(NEW);
  await alice.locator(".prompt input").press("Enter");

  // A row with no note: the platform takes it, and the card is its title, number and state.
  const card = row(alice, NEW);
  await expect(now.locator(card)).toHaveCount(1, { timeout: 15_000 });
  await expect(card.locator("[data-board-ptr]")).toHaveText(/^#\d+$/);
  await expect(card.locator("[data-board-live]")).toHaveAttribute("data-board-live", "open");
  await expect(card.locator("[data-board-note]")).toHaveCount(0);

  await platform(alice, `/p/${project}/issues`);
  await expect(alice.locator("[data-issue-row]")).toHaveCount(4, { timeout: 15_000 });
  await expect(alice.locator("[data-issue-row]", { hasText: NEW })).toHaveCount(1);
});

test("a viewer on a phone sees what is untriaged and the board's cards, with no sideways scroll", async () => {
  test.setTimeout(90_000);
  await home(bob);
  await platform(bob, `/p/${project}/triage`);
  await expect(bob.locator("[data-triage-row]")).toHaveCount(1, { timeout: 15_000 });
  await expect(triaged(bob, THIRD)).toBeVisible();
  await expect(bob.locator("[data-triage-add]")).toHaveCount(0);

  await platform(bob, boardPath);
  for (const title of [FIRST, SECOND, NEW]) await expect(row(bob, title)).toHaveCount(1, { timeout: 15_000 });
  const wide = await bob.evaluate(() => document.documentElement.scrollWidth - window.innerWidth);
  expect(wide).toBeLessThanOrEqual(0);
});

test("Alice sets the board's goal, and it shows under the title with the issues' progress", async () => {
  test.setTimeout(90_000);
  await platform(alice, boardPath);
  const page = alice.locator("[data-board-page]");
  await expect(page.locator("[data-board-progress]")).toHaveText("0 of 3 done", { timeout: 15_000 });
  await expect(page.locator("[data-board-goal]")).toHaveCount(0);
  await expect(page.locator("[data-board-progress] [role=progressbar]")).toHaveAttribute("aria-valuemax", "3");
  await page.locator("[data-board-goal-edit]").click();
  await page.locator("[data-board-goal-input]").fill(GOAL);
  await page.locator("[data-board-goal-save]").click();
  await expect(page.locator("[data-board-goal]")).toHaveText(GOAL, { timeout: 15_000 });
  // Plain text keeping its line break.
  await expect(page.locator("[data-board-goal]")).toHaveCSS("white-space", "pre-wrap");

  // Renaming is the title's click.
  await page.locator("[data-board-rename]").click();
  await alice.locator(".prompt input").fill("Launch 1.0");
  await alice.locator(".prompt input").press("Enter");
  await expect(page.locator("h1[data-board-title]")).toHaveText("Launch 1.0", { timeout: 15_000 });
  await page.locator("[data-board-rename]").click();
  await alice.locator(".prompt input").fill("Launch");
  await alice.locator(".prompt input").press("Enter");
  await expect(page.locator("h1[data-board-title]")).toHaveText("Launch", { timeout: 15_000 });
});

test("closing one of the board's issues moves its row to Done, not yet proven, until evidence is attached", async () => {
  test.setTimeout(90_000);
  const id = /brd_[0-9a-f]+/.exec(boardPath)![0];
  const view = (await api(alice, "GET", `/api/boards/${id}`)).body!;
  const first = view.zones.flatMap((z: any) => z.rows).find((r: any) => r.live?.title === FIRST);
  const closed = await api(alice, "PATCH", `/api/issues/${first.pointer.id}`, { state: "closed", reason: "completed" });
  expect(closed.status).toBe(200);
  await platform(alice, boardPath);
  const page = alice.locator("[data-board-page]");
  await expect(page.locator("[data-board-progress]")).toHaveText("1 of 3 done, 0 proven", { timeout: 15_000 });
  await expect(page.locator("[data-board-progress] [role=progressbar]")).toHaveAttribute("aria-valuenow", "1");
  // Nobody moved it: the closed issue is in Done, and the others stay in Next.
  await expect(zone(alice, "Done").locator(row(alice, FIRST))).toHaveCount(1);
  await expect(zone(alice, "Next").locator(row(alice, FIRST))).toHaveCount(0);
  await expect(zone(alice, "Next").locator("[data-board-row]")).toHaveCount(2);
  await expect(row(alice, FIRST).locator("[data-board-proof]")).toHaveAttribute("data-board-proof", "not-proven");
  await expect(row(alice, FIRST).locator("[data-board-proof]")).toHaveText("not proven yet");
  // There is no PROOFS zone to fold, and a row has no menu to move it.
  await expect(alice.locator('[data-zone-kind="proofs"]')).toHaveCount(0);
  await row(alice, FIRST).locator("[data-board-row-menu]").click();
  await expect(alice.getByRole("menuitem", { name: /^Move/ })).toHaveCount(0);
  await alice.keyboard.press("Escape");

  // Evidence attached to the Done row is the mark that proves it.
  const att = await api(alice, "POST", `/api/boards/${id}/attachments`, {
    row: first.id,
    role: "proof",
    pointer: { kind: "link", url: "https://ci.example.com/runs/1" },
  });
  expect(att.status).toBe(200);
  await expect(page.locator("[data-board-progress]")).toHaveText("1 of 3 done, 1 proven", { timeout: 15_000 });
  await expect(row(alice, FIRST).locator("[data-board-proof]")).toHaveAttribute("data-board-proof", "evidence");
  await expect(row(alice, FIRST).locator("[data-board-attachment]")).toContainText("ci.example.com/runs/1");
});

test("Alice closes the board with a note, finds it under Done, and reopens it", async () => {
  test.setTimeout(90_000);
  const page = alice.locator("[data-board-page]");
  await page.locator("[data-board-close]").click();
  const save = page.locator("[data-board-close-save]");
  await expect(save).toBeDisabled();
  await page.locator("[data-board-close-note]").fill(NOTE);
  await expect(save).toBeEnabled();
  await save.click();
  await expect(page.locator("[data-board-done]")).toContainText("Done", { timeout: 15_000 });
  await expect(page.locator("[data-board-done-note]")).toHaveText(NOTE);
  await expect(page.locator("[data-board-close]")).toHaveCount(0);
  // Rows stay editable on a closed board.
  await expect(row(alice, SECOND).locator("[data-board-row-menu]")).toBeEnabled();

  await alice.locator("[data-platform-boards]").click();
  const done = alice.locator("[data-boards-done]");
  await expect(done.locator("summary")).toHaveText("Done (1)", { timeout: 15_000 });
  await expect(done).not.toHaveAttribute("open", "");
  await expect(alice.locator("[data-boards] [data-board-link]")).toHaveCount(0);
  await done.locator("summary").click();
  await expect(done.locator("[data-board-link]")).toContainText("Launch");
  await expect(done).toContainText("A person can install Acme and invite a team.");
  await expect(done).not.toContainText("The changelog is current.");

  await done.locator("[data-board-link]").click();
  await alice.locator("[data-board-reopen]").click();
  await expect(alice.locator("[data-board-done]")).toHaveCount(0, { timeout: 15_000 });
  await expect(alice.locator("[data-board-close]")).toBeVisible();
  await alice.locator("[data-platform-boards]").click();
  await expect(alice.locator("[data-boards] [data-board-link]", { hasText: "Launch" })).toHaveCount(1, { timeout: 15_000 });
  await expect(alice.locator("[data-boards] [data-board-progress]")).toHaveText("1 of 3 done, 1 proven");
  await expect(alice.locator("[data-boards-done]")).toHaveCount(0);
  // The note is kept when the board is open again.
  const id = /brd_[0-9a-f]+/.exec(boardPath)![0];
  const kept = (await api(alice, "GET", `/api/boards/${id}`)).body!;
  expect(kept.board.done_note).toBe(NOTE);
  expect(kept.board.closed_at ?? null).toBeNull();
});

test("a viewer on a phone sees the goal, progress and Done, with no buttons and no sideways scroll", async () => {
  test.setTimeout(90_000);
  const id = /brd_[0-9a-f]+/.exec(boardPath)![0];
  expect((await api(alice, "POST", `/api/boards/${id}/close`, { done_note: NOTE })).status).toBe(200);
  await platform(bob, boardPath);
  const page = bob.locator("[data-board-page]");
  await expect(page.locator("[data-board-goal]")).toHaveText(GOAL, { timeout: 15_000 });
  await expect(page.locator("[data-board-progress]")).toHaveText("1 of 3 done, 1 proven");
  await expect(page.locator("[data-board-done]")).toContainText("Done");
  for (const hook of ["close", "reopen", "goal-edit", "rename"]) await expect(page.locator(`[data-board-${hook}]`)).toHaveCount(0);
  expect(await bob.evaluate(() => document.documentElement.scrollWidth - window.innerWidth)).toBeLessThanOrEqual(0);

  await platform(bob, boardPath.replace(/\/boards\/.*/, "/boards"));
  await expect(bob.locator("[data-boards-done] summary")).toHaveText("Done (1)", { timeout: 15_000 });
  expect(await bob.evaluate(() => document.documentElement.scrollWidth - window.innerWidth)).toBeLessThanOrEqual(0);
});

test("/boards lists every project's boards by project, folded under Done the same", async () => {
  test.setTimeout(60_000);
  for (const p of [alice, bob]) {
    await platform(p, "/boards");
    const page = p.locator("[data-all-boards-page]");
    const acme = page.locator(`[data-all-boards-project="${project}"]`);
    await expect(acme.locator("h2")).toHaveText("Acme", { timeout: 15_000 });
    // Only Acme has boards: Alice's own project isn't listed.
    await expect(page.locator("[data-all-boards-project]")).toHaveCount(1);
    await expect(acme.locator("[data-boards-done] summary")).toHaveText("Done (1)");
    // The counts over the page, and a board a card with its state.
    await expect(page.locator("[data-boards-stats]")).toHaveText("0 open1 done");
    await acme.locator("[data-boards-done] summary").click();
    await expect(acme.locator("[data-board-link]")).toContainText("Launch");
    await expect(acme.locator("[data-boards-done] li.board-card.sw-card.sw-muted")).toContainText("Done");
    expect(await p.evaluate(() => document.documentElement.scrollWidth - window.innerWidth)).toBeLessThanOrEqual(0);
  }
  await alice.locator("[data-all-boards-project-link]").click();
  await expect(alice.locator("[data-boards-page]")).toBeVisible({ timeout: 15_000 });
});

test("a board with a long title and goal wraps in a card on a phone, with no sideways scroll", async () => {
  test.setTimeout(60_000);
  const long = "An extraordinarily long board title " + "withoutbreaks".repeat(6);
  const made = await api(alice, "POST", `/api/projects/${project}/boards`, { title: long });
  expect(made.status).toBe(200);
  const edited = await api(alice, "PATCH", `/api/boards/${made.body!.board.id}`, { goal: `${long} ${long}` });
  expect(edited.status).toBe(200);
  for (const path of ["/boards", `/p/${project}/boards`]) {
    await platform(bob, path);
    const card = bob.locator("li.board-card.sw-card.sw-ok", { hasText: "extraordinarily" });
    await expect(card).toHaveCount(1, { timeout: 15_000 });
    await expect(card).toBeVisible();
    await expect(card).toContainText("Open");
    // The card's entry animation (swarm.css) settles: it isn't left at opacity 0.
    await expect.poll(() => card.evaluate((el) => getComputedStyle(el).opacity), { timeout: 5_000 }).toBe("1");
    await expect(card.locator("[data-board-progress]")).toHaveText("No work yet");
    await expect(bob.locator("[data-boards-stats]")).toHaveText("1 open1 done");
    expect(await bob.evaluate(() => document.documentElement.scrollWidth - window.innerWidth)).toBeLessThanOrEqual(0);
    if (process.env.BOARDS_SHOTS) await bob.screenshot({ path: `${process.env.BOARDS_SHOTS}/${path === "/boards" ? "all-boards" : "project-boards"}.png`, animations: "disabled" });
  }
  if (process.env.BOARDS_SHOTS) {
    await platform(bob, `/p/${project}/issues`);
    await expect(bob.locator("[data-issue-row]").first()).toBeVisible();
    await bob.screenshot({ path: `${process.env.BOARDS_SHOTS}/issues.png`, animations: "disabled" });
  }
});

test("a board with groups shows each as a section with its four lanes, and the rows with none last", async () => {
  test.setTimeout(90_000);
  const made = await api(alice, "POST", `/api/projects/${project}/boards`, { title: "Phased" });
  expect(made.status).toBe(200);
  const id = made.body!.board.id as string;
  const entries = `/api/boards/${id}/entries`;
  await api(alice, "PUT", entries, { entry: "zone", title: "Decide" });
  const both = await api(alice, "PUT", entries, { entry: "zone", title: "Build" });
  const [decide, build] = (both.body!.groups as { id: string }[]).map((g) => g.id);
  for (const [note, zone] of [["Pick a name", decide], ["Wire it up", build], ["Loose end", undefined]] as const) {
    expect((await api(alice, "PUT", entries, { entry: "row", note, zone })).status).toBe(200);
  }
  await platform(alice, `/p/${project}/boards/${id}`);
  await expect(alice.locator("[data-board-group-title]")).toHaveText(["Decide", "Build", "No group"], { timeout: 15_000 });
  const groupOf = (title: string) => alice.locator("[data-board-group]").filter({ has: alice.locator("[data-board-group-title]", { hasText: title }) });
  // Each group has the four lanes, and its rows are in them.
  for (const title of ["Decide", "Build", "No group"]) await expect(groupOf(title).locator("[data-board-zone] h2")).toHaveText(["Next", "Doing", "Review", "Done"]);
  await expect(groupOf("Decide").locator("[data-board-row]")).toHaveCount(1);
  await expect(groupOf("Decide").locator("[data-board-note]")).toHaveText("Pick a name");
  await expect(groupOf("Build").locator("[data-board-note]")).toHaveText("Wire it up");
  await expect(groupOf("No group").locator("[data-board-note]")).toHaveText("Loose end");
  await expect(groupOf("Decide").locator("[data-board-group-progress]")).toHaveText("no work yet");
  // A board without groups is as before: lanes only.
  await platform(alice, boardPath);
  await expect(alice.locator("[data-board-group]")).toHaveCount(0);
  await expect(alice.locator("[data-board-zone] h2")).toHaveText(["Next", "Doing", "Review", "Done"]);
});
