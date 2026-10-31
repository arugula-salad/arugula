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
let managed = "";

const FIRST = "Fix the installer";
const SECOND = "Tidy the changelog";
const THIRD = "Update the screenshots";
const NEW = "Write the launch note";
const OPEN = "Cut the release";
const SHIPPED = "Publish the notes";
const BODY = "The release is cut from main.";

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
    // A board's title in a list is its link.
    await expect(acme.locator("a[data-board-link]")).toHaveAttribute("href", /\/boards\/brd_[0-9a-f]+/);
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

test("a board's cards carry their lane's tone, its groups are managed on the page, and a row moves to another group", async () => {
  test.setTimeout(120_000);
  const made = await api(alice, "POST", `/api/projects/${project}/boards`, { title: "Managed" });
  expect(made.status).toBe(200);
  const id = made.body!.board.id as string;
  managed = `/p/${project}/boards/${id}`;
  const entries = `/api/boards/${id}/entries`;
  const me = (await api(alice, "GET", "/api/projects")).body!.me.account as string;
  // Two groups, a row in the first with an open issue (Next), one with a closed issue (Done) and a note row.
  const first = await api(alice, "PUT", entries, { entry: "zone", title: "Wave one" });
  const wave = (first.body!.groups as { id: string }[])[0].id;
  const issue = async (title: string, state?: string) => {
    const i = await api(alice, "POST", `/api/projects/${project}/issues`, { title, body: title === OPEN ? BODY : "", labels: [], assignees: [] });
    expect(i.status).toBe(200);
    if (state) expect((await api(alice, "PATCH", `/api/issues/${i.body!.id}`, { state, reason: "completed" })).status).toBe(200);
    const r = await api(alice, "PUT", entries, { entry: "row", zone: wave, pointer: { kind: "issue", id: i.body!.id } });
    expect(r.status).toBe(200);
    return r.body!.zones.flatMap((z: any) => z.rows).find((x: any) => x.pointer.id === i.body!.id);
  };
  const open = await issue(OPEN);
  const shipped = await issue(SHIPPED, "closed");
  // An ask to Alice on the open one, and evidence attached to the shipped one.
  expect((await api(alice, "POST", `/api/boards/${id}/rows/${open.id}/ask`, { who: me, question: "Ship it this week?", options: ["Yes", "Next week"], recommend: "Yes" })).status).toBe(200);
  expect((await api(alice, "POST", `/api/boards/${id}/attachments`, { row: shipped.id, role: "proof", pointer: { kind: "link", url: "https://example.com/branch" } })).status).toBe(200);

  await platform(alice, managed);
  const lanes = alice.locator("[data-board-group] .board-lanes");
  await expect(lanes).toHaveCount(1, { timeout: 15_000 });
  await expect(alice.locator("[data-board-zone] ul.sw-cards")).toHaveCount(4);
  // A group's title is a heading above the lanes, and a row's title is a smaller, plainer one.
  const px = (l: Locator) => l.evaluate((el) => parseFloat(getComputedStyle(el).fontSize));
  expect(await px(alice.locator("[data-board-group-title]").first())).toBeGreaterThanOrEqual(22);
  expect(await px(alice.locator("[data-board-zone] h2").first())).toBeLessThan(18);
  expect(await px(alice.locator("[data-board-row] [data-board-title]").first())).toBeLessThanOrEqual(15);
  // The bar is the lane's tone; a row that waits on Alice takes the attention tone, and the header counts it.
  const card = row(alice, SHIPPED);
  await expect(zone(alice, "Done").locator(card)).toHaveClass(/\bsw-card\b.*\bsw-ok\b/);
  await expect(card.locator(".chip")).toContainText(/closed/i);
  // Its title is the issue's page, and its `#N` is dim text, not a link.
  await expect(card.locator("[data-board-title]")).toHaveAttribute("href", /\/issues\/iss_[0-9a-f]+$/);
  await expect(card.locator("[data-board-ptr]")).toHaveText(/^#\d+$/);
  await expect(card.locator("a[data-board-ptr]")).toHaveCount(0);
  // A click on the title follows the link and doesn't open the drawer.
  await card.locator("[data-board-title]").click();
  await expect(alice).toHaveURL(/\/issues\/iss_/);
  await platform(alice, managed);
  await expect(card).toBeVisible({ timeout: 15_000 });
  await expect(alice.locator("[data-board-drawer]")).toHaveCount(0);
  await expect(row(alice, OPEN)).toHaveClass(/\bsw-accent\b/);
  await expect(card.locator(".sw-chip")).toContainText("example.com/branch");
  await expect(alice.locator("[data-board-stats]")).toContainText("1 done");
  await expect(alice.locator("[data-board-needs-you]")).toContainText("1 Needs you");
  await row(alice, OPEN).locator("[data-board-ask-option='Yes']").click();
  await expect(row(alice, OPEN)).toHaveClass(/\bsw-muted\b/, { timeout: 15_000 });
  await expect(alice.locator("[data-board-needs-you]")).toHaveCount(0);
  // A second question, left open: it is in the header's count, and its card takes the attention tone.
  const price = await issue("Decide the price");
  expect((await api(alice, "POST", `/api/boards/${id}/rows/${price.id}/ask`, { who: me, question: "Free or paid?", options: ["Free", "Paid"] })).status).toBe(200);
  await platform(alice, managed);
  await expect(alice.locator("[data-board-needs-you]")).toContainText("1 Needs you", { timeout: 15_000 });
  await expect(row(alice, "Decide the price")).toHaveClass(/\bsw-accent\b/);

  // Add a group, rename it, move it up, and delete it again.
  const titles = alice.locator("[data-board-group-title]");
  await alice.locator("[data-board-add-group]").click();
  await alice.locator(".prompt input").fill("Wave two");
  await alice.locator(".prompt input").press("Enter");
  await expect(titles).toHaveText(["Wave one", "Wave two"], { timeout: 15_000 });
  const second = alice.locator("[data-board-group]").filter({ has: alice.locator("[data-board-group-title]", { hasText: "Wave two" }) });
  await second.locator("[data-board-group-menu]").click();
  await alice.getByRole("menuitem", { name: "Rename…" }).click();
  await alice.locator(".prompt input").fill("Wave 2");
  await alice.locator(".prompt input").press("Enter");
  await expect(titles).toHaveText(["Wave one", "Wave 2"], { timeout: 15_000 });
  await alice.locator("[data-board-group-menu]").nth(1).click();
  await alice.getByRole("menuitem", { name: "Move up" }).click();
  await expect(titles).toHaveText(["Wave 2", "Wave one"], { timeout: 15_000 });
  await alice.locator("[data-board-group-menu]").nth(0).click();
  await expect(alice.getByRole("menuitem", { name: "Move up" })).toBeDisabled();
  await alice.keyboard.press("Escape");

  // Move the open issue's row to Wave 2 from its menu.
  await row(alice, OPEN).locator("[data-board-row-menu]").click();
  await alice.getByRole("menuitem", { name: "Move to group…" }).click();
  // The group the row is in is checked, so its item is a radio; the others are plain items.
  await alice.locator('[role^="menuitem"]', { hasText: "Wave 2" }).click();
  const wave2 = alice.locator("[data-board-group]").filter({ has: alice.locator("[data-board-group-title]", { hasText: "Wave 2" }) });
  await expect(wave2.locator(row(alice, OPEN))).toHaveCount(1, { timeout: 15_000 });
  await expect(alice.locator("[data-board-group]").filter({ hasText: "Wave one" }).locator(row(alice, OPEN))).toHaveCount(0);

  if (process.env.BOARDS_SHOTS) {
    await alice.setViewportSize({ width: 1400, height: 900 });
    await alice.screenshot({ path: `${process.env.BOARDS_SHOTS}/board-desktop.png`, animations: "disabled" });
    await platform(alice, `/p/${project}/issues`);
    await expect(alice.locator("[data-issue-row]").first()).toBeVisible();
    await alice.screenshot({ path: `${process.env.BOARDS_SHOTS}/issues-desktop.png`, animations: "disabled" });
  }
  // A group with nothing in it goes after a confirm; the others stay.
  expect((await api(alice, "PUT", entries, { entry: "zone", title: "Scratch" })).status).toBe(200);
  await platform(alice, managed);
  await expect(titles).toHaveText(["Wave 2", "Wave one", "Scratch"], { timeout: 15_000 });
  await alice.locator("[data-board-group-menu]").nth(2).click();
  await alice.getByRole("menuitem", { name: "Delete the group…" }).click();
  await alice.locator("[data-confirm-remove]").click();
  await expect(titles).toHaveText(["Wave 2", "Wave one"], { timeout: 15_000 });
});

// #426: a board is often a pane, so its lanes and cards follow the board's width, not the window's.
test("in a board 760px wide the lanes are two per row and every card title keeps 14ch, at any viewport", async () => {
  test.setTimeout(120_000);
  const made = await api(alice, "POST", `/api/projects/${project}/boards`, { title: "Narrow" });
  expect(made.status).toBe(200);
  const id = made.body!.board.id as string;
  const entries = `/api/boards/${id}/entries`;
  const first = await api(alice, "PUT", entries, { entry: "zone", title: "Wave" });
  const wave = (first.body!.groups as { id: string }[])[0].id;
  const add = async (title: string, state?: string) => {
    const i = await api(alice, "POST", `/api/projects/${project}/issues`, { title, body: "", labels: [], assignees: [] });
    expect(i.status).toBe(200);
    if (state) expect((await api(alice, "PATCH", `/api/issues/${i.body!.id}`, { state, reason: "completed" })).status).toBe(200);
    const r = await api(alice, "PUT", entries, { entry: "row", zone: wave, pointer: { kind: "issue", id: i.body!.id } });
    expect(r.status).toBe(200);
    return r.body!.zones.flatMap((z: any) => z.rows).find((x: any) => x.pointer.id === i.body!.id);
  };
  await add("Next up: write the migration guide for the new settings");
  // Done cards with a closed chip and a proof tag, the case that squeezed the title.
  for (const title of ["Plane settings: show the webhook URL and its secret", "Publish the release notes and the changelog for everyone"]) {
    const done = await add(title, "closed");
    expect((await api(alice, "POST", `/api/boards/${id}/attachments`, { row: done.id, role: "proof", pointer: { kind: "link", url: "https://example.com/branch" } })).status).toBe(200);
  }

  await alice.setViewportSize({ width: 1900, height: 1000 });
  await platform(alice, `/p/${project}/boards/${id}`);
  await expect(alice.locator("[data-board-row]")).toHaveCount(3, { timeout: 15_000 });
  const shots = process.env.SHOTS_426; // "<dir>/<before|after>-", for a look by eye
  const columns = (w: number) =>
    alice.evaluate((px) => {
      const board = document.querySelector<HTMLElement>(".board")!;
      board.style.maxWidth = "none";
      board.style.width = `${px}px`;
      return getComputedStyle(document.querySelector(".board-lanes")!).gridTemplateColumns.split(" ").length;
    }, w);
  // The window stays wide throughout; only the board's own width changes.
  for (const [width, lanes] of [[1700, 4], [1000, 4], [760, 2], [480, 1]] as const) {
    const across = await columns(width);
    if (shots) await alice.locator(".board").screenshot({ path: `${shots}${width}.png` });
    expect(across).toBe(lanes);
    const m = await alice.evaluate(() => {
      const probe = document.createElement("span");
      const first = document.querySelector<HTMLElement>(".board-card [data-board-title]")!;
      probe.style.cssText = `position:absolute;visibility:hidden;width:14ch;font:${getComputedStyle(first).font}`;
      document.body.append(probe);
      const ch = probe.getBoundingClientRect().width;
      probe.remove();
      const lanes = document.querySelector<HTMLElement>(".board-lanes")!;
      return {
        ch,
        titles: [...document.querySelectorAll(".board-card [data-board-title]")].map((t) => t.getBoundingClientRect().width),
        scroll: lanes.scrollWidth - lanes.clientWidth,
        // The menu stays at its head's top-right, however the title and chip wrap.
        menus: [...document.querySelectorAll(".board-card-head")].map((h) => {
          const b = h.querySelector("[data-board-row-menu]")!.getBoundingClientRect();
          const r = h.getBoundingClientRect();
          return { right: r.right - b.right, top: b.top - r.top };
        }),
      };
    });
    for (const w of m.titles) expect(w).toBeGreaterThanOrEqual(Math.min(m.ch, width - 80) - 0.5);
    expect(m.scroll).toBeLessThanOrEqual(0);
    for (const b of m.menus) (expect(b.right).toBeLessThanOrEqual(8), expect(b.top).toBeLessThanOrEqual(8));
  }
  // The proof tag is on the sub line, not the head.
  await expect(alice.locator(".board-card-head [data-board-proof]")).toHaveCount(0);
  await expect(alice.locator(".board-sub [data-board-proof]")).toHaveCount(2);
});

test("a card's title opens the drawer with the item's body, its comments and the PR's checks", async () => {
  test.setTimeout(90_000);
  const checks = { item: "fi_0123456789abcdef0123456789abcdef", head_sha: "abc1234", checks: [
    { kind: "check_run", name: "binaries/quick", status: "completed", conclusion: "success", url: "https://github.com/acme/app/runs/1", updated_at: 1 },
    { kind: "check_run", name: "lint", status: "completed", conclusion: "failure", url: null, updated_at: 1 },
  ] };
  // The GitHub side has no mirror here: the board is read with a PR in place of the first row, and the item routes answer as the platform does.
  await alice.route(/\/api\/boards\/brd_[0-9a-f]+$/, async (route) => {
    const r = await route.fetch();
    const j = await r.json();
    const pr = j.zones.flatMap((z: any) => z.rows).find((x: any) => x.live?.title === SHIPPED);
    if (pr) {
      pr.pointer = { kind: "forge_item", id: checks.item };
      pr.live = { state: "checks_failed", kind: "pr", repo: "acme/app", number: 7, title: SHIPPED, assignees: [] };
      pr.lane = "review";
      // Two agents on it: one working with a title, one idle with none. Neither is named by its pane number.
      pr.attachments = [
        { id: "att_0000000000000000000000000000aa01", role: "work", pointer: { kind: "block", machine: "mch_0123", pane: "640" }, block: { state: "working", title: "Lexer lane", branch: "feat/lexer", head: "abc1234", pushed: false, uncommitted: 2 } },
        { id: "att_0000000000000000000000000000aa02", role: "work", pointer: { kind: "block", machine: "mch_0123", pane: "641" }, block: { state: "idle", branch: "feat/other", head: "def5678", pushed: true, uncommitted: 0 } },
      ];
    }
    await route.fulfill({ response: r, json: j });
  });
  await alice.route(/\/api\/forge\/items\/fi_[0-9a-f]+\/checks$/, (route) => route.fulfill({ json: checks }));
  await alice.route(/\/api\/forge\/items\/fi_[0-9a-f]+\/content$/, (route) =>
    route.fulfill({
      json: {
        item: checks.item,
        from: "github",
        body: { kind: "body", id: 1, author: "alice", body: "The PR body, from GitHub.", path: null, line: null, reply_to: null, created_at: 1, updated_at: 1 },
        comments: [1, 2, 3, 4].map((n) => ({ kind: "comment", id: n + 1, author: "bob", body: `Comment ${n}`, path: null, line: null, reply_to: null, created_at: n, updated_at: n })),
      },
    }),
  );
  await platform(alice, managed);
  const card = row(alice, SHIPPED);
  await expect(card).toHaveClass(/\bsw-warn\b/, { timeout: 15_000 });
  // One chip per agent: its title and its state, once. Its branch and the rest come only when it has stopped, dim beside the chip.
  const working = card.locator('[data-board-block="working"]');
  await expect(working).toHaveText(/^Lexer lane\s*working$/);
  await expect(card.getByText("working", { exact: true })).toHaveCount(1);
  await expect(card).not.toContainText("640");
  await expect(card).not.toContainText("641");
  await expect(card.locator('[data-board-block="idle"]')).toHaveText(/^agent\s*idle$/);
  await expect(card.locator("[data-board-block-facts]")).toHaveText("feat/other@def5678 · pushed");
  await expect(card).not.toContainText("feat/lexer");
  // The chip opens the agent's pane, on control's page (the platform page has no panes of its own).
  await expect(working.locator("[data-board-block-open]")).toHaveAttribute("href", /\/#pane=mch_0123\.640$/);
  // The title is the item's link (here a PR on GitHub, in a new tab), and `#N` is plain text: clicking the title leaves the board alone.
  const title = card.locator("[data-board-title]");
  await expect(title).toHaveAttribute("href", "https://github.com/acme/app/pull/7");
  await expect(card.locator("[data-board-ptr]")).toHaveText("acme/app#7");
  await expect(card.locator("a[data-board-ptr]")).toHaveCount(0);
  const [tab] = await Promise.all([alice.context().waitForEvent("page"), title.click()]);
  await tab.close();
  await expect(alice.locator("[data-board-drawer]")).toHaveCount(0);
  // Details in the row's menu opens the drawer; so does the card's empty space (its corner, here), and Enter on the focused card.
  await card.locator("[data-board-row-menu]").click();
  await alice.getByRole("menuitem", { name: "Details" }).click();
  await expect(alice.locator("[data-board-drawer]")).toHaveCount(1);
  await expect(alice.locator("[data-board-drawer]")).toBeFocused();
  await alice.keyboard.press("Escape");
  await expect(alice.locator("[data-board-drawer]")).toHaveCount(0);
  await card.focus();
  await alice.keyboard.press("Enter");
  await expect(alice.locator("[data-board-drawer]")).toHaveCount(1);
  await expect(alice.locator("[data-board-drawer]")).toBeFocused();
  await alice.keyboard.press("Escape");
  await expect(alice.locator("[data-board-drawer]")).toHaveCount(0);
  // A click on a chip is the chip's, not the card's.
  const [pane] = await Promise.all([alice.context().waitForEvent("page"), card.locator("[data-board-block-open]").first().click()]);
  await pane.close();
  await expect(alice.locator("[data-board-drawer]")).toHaveCount(0);
  await card.click({ position: { x: 6, y: 6 } });
  const drawer = alice.locator("[data-board-drawer]");
  await expect(drawer.locator("[data-board-drawer-title]")).toHaveText(SHIPPED);
  await expect(drawer.locator("[data-board-drawer-body]")).toHaveText("The PR body, from GitHub.");
  // The latest three comments, not the first.
  await expect(drawer.locator("[data-board-drawer-comments] .board-drawer-comment")).toHaveCount(3);
  await expect(drawer.locator("[data-board-drawer-comments]")).toContainText("Comment 4");
  await expect(drawer.locator("[data-board-drawer-comments]")).not.toContainText("Comment 1");
  await expect(drawer.locator('[data-board-check="binaries/quick"]')).toContainText("success");
  await expect(drawer.locator('[data-board-check="lint"]')).toContainText("failure");
  await expect(drawer.locator("[data-board-drawer-link]")).toHaveAttribute("href", "https://github.com/acme/app/pull/7");
  await drawer.locator("[data-board-drawer-close]").click();
  await expect(drawer).toHaveCount(0);
  // An item the reader may not read here has no body to show: the drawer says to open it on GitHub.
  await alice.unroute(/\/api\/forge\/items\/fi_[0-9a-f]+\/content$/);
  await alice.route(/\/api\/forge\/items\/fi_[0-9a-f]+\/content$/, (route) => route.fulfill({ status: 404, json: { error: "not found" } }));
  await card.click({ position: { x: 6, y: 6 } });
  await expect(drawer.locator("[data-board-drawer-body] a")).toHaveText("Open on GitHub");
  await expect(drawer.locator("[data-board-drawer-comments]")).toHaveCount(0);
  await expect(drawer.locator('[data-board-check="lint"]')).toContainText("failure");
  await drawer.locator("[data-board-drawer-close]").click();
  if (process.env.BOARDS_SHOTS) {
    await alice.setViewportSize({ width: 1400, height: 900 });
    await alice.screenshot({ path: `${process.env.BOARDS_SHOTS}/board-agents-desktop.png`, animations: "disabled" });
    await alice.setViewportSize({ width: 390, height: 844 });
    await alice.screenshot({ path: `${process.env.BOARDS_SHOTS}/board-agents-phone.png`, animations: "disabled", fullPage: true });
  }
  await alice.unroute(/\/api\/boards\/brd_[0-9a-f]+$/);
});

test("a native issue's drawer shows its body, and a viewer on a phone has the drawer as a sheet and no sideways scroll", async () => {
  test.setTimeout(90_000);
  await platform(bob, managed);
  // Bob can't manage anything.
  await expect(row(bob, OPEN)).toHaveCount(1, { timeout: 15_000 });
  for (const hook of ["add-group", "group-menu"]) await expect(bob.locator(`[data-board-${hook}]`)).toHaveCount(0);
  await row(bob, OPEN).locator("[data-board-row-menu]").click();
  await expect(bob.getByRole("menuitem", { name: "Move to group…" })).toHaveCount(0);
  await bob.keyboard.press("Escape");
  expect(await bob.evaluate(() => document.documentElement.scrollWidth - window.innerWidth)).toBeLessThanOrEqual(0);
  await row(bob, OPEN).click({ position: { x: 6, y: 6 } });
  const drawer = bob.locator("[data-board-drawer]");
  await expect(drawer).toBeVisible();
  await expect(drawer.locator("[data-board-drawer-body]")).toHaveText(BODY, { timeout: 15_000 });
  const box = (await drawer.boundingBox())!;
  expect(box.x).toBeGreaterThanOrEqual(0);
  expect(box.x + box.width).toBeLessThanOrEqual(390);
  // A sheet at the foot of the screen, not a side panel.
  expect(box.width).toBeGreaterThan(380);
  expect(box.y).toBeGreaterThan(100);
  expect(await bob.evaluate(() => document.documentElement.scrollWidth - window.innerWidth)).toBeLessThanOrEqual(0);
  await bob.keyboard.press("Escape");
  await expect(drawer).toHaveCount(0);
  if (process.env.BOARDS_SHOTS) await bob.screenshot({ path: `${process.env.BOARDS_SHOTS}/board-phone.png`, animations: "disabled" });
});

test("connecting a Plane project: each field is labelled, says what goes in it, and is wide enough to read", async () => {
  test.setTimeout(60_000);
  await platform(alice, `/p/${project}/settings`);
  const form = alice.locator("[data-plane-form]");
  await expect(form).toBeVisible({ timeout: 15_000 });
  for (const [hook, label] of [["workspace", "Workspace"], ["project", "Project"], ["key", "API key"]] as const) {
    const input = form.locator(`[data-plane-${hook}]`);
    // The label is the field's own, so a screen reader and a click both find it.
    await expect(form.getByLabel(label, { exact: false }).first()).toBeVisible();
    expect((await input.boundingBox())!.width, `${hook} is wide enough to read`).toBeGreaterThan(240);
  }
  // The secret comes after connecting, in the connection's row.
  await expect(form.locator("[data-plane-hook]")).toHaveCount(0);
  if (process.env.BOARDS_SHOTS) await form.screenshot({ path: `${process.env.BOARDS_SHOTS}/plane-connect.png`, animations: "disabled" });
});
