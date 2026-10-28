// Issues on boards, phase 1 (#289-#292), as a person meets them. Alice owns
// team Acme, with chat's and boards' Labs flags on, and three native issues,
// one hers. The bar says Triage (not Issues) and Boards, and Triage lists the
// three. She makes the board Launch with a zone Now; Add to board… on the
// first issue leaves it off Triage and draws it as a card on the board. She
// adds a row through the "points at" picker (type, ArrowDown, Enter), edits
// only its note, and makes an issue from the zone's menu with New issue….
// Bob, a viewer on a phone, sees what's still untriaged, no Add to board…,
// and the board's cards with no sideways scroll. A local control
// (team-fixture.ts, `--platform-host app.localhost`), as issues-platform.spec.ts.

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

test("a board Launch with a zone Now takes the first issue from Triage as a card", async () => {
  test.setTimeout(90_000);
  await alice.locator("[data-platform-boards]").click();
  await expect(alice).toHaveURL(/\/p\/prj_[0-9a-f]+-acme\/boards$/);
  await alice.locator("[data-board-new-title]").fill("Launch");
  await alice.locator("[data-board-new]").click();
  await expect(alice.locator("[data-board-page] [data-board-title]")).toHaveText("Launch", { timeout: 15_000 });
  boardPath = new URL(alice.url()).pathname;

  await alice.locator("[data-board-add-zone]").click();
  await alice.locator(".prompt input").fill("Now");
  await alice.locator(".prompt input").press("Enter");
  await expect(zone(alice, "Now")).toBeVisible();
  // A new board has no Proofs zone; + Add PROOFS makes it, and the button goes.
  await alice.locator("[data-board-add-proofs]").click();
  await expect(alice.locator('[data-board-zone][data-zone-kind="proofs"]')).toBeVisible();
  await expect(alice.locator("[data-board-add-proofs]")).toHaveCount(0);

  await alice.locator("[data-platform-triage]").click();
  await expect(alice.locator("[data-triage-row]")).toHaveCount(3, { timeout: 15_000 });
  await triaged(alice, FIRST).locator("[data-triage-add]").click();
  await alice.getByRole("menuitem", { name: "Launch" }).click();
  await alice.getByRole("menuitem", { name: "Now" }).click();
  await expect(triaged(alice, FIRST)).toHaveCount(0);
  await expect(alice.locator("[data-triage-row]")).toHaveCount(2);

  await alice.locator("[data-platform-boards]").click();
  await alice.locator("[data-board-link]", { hasText: "Launch" }).click();
  const card = row(alice, FIRST);
  await expect(zone(alice, "Now").locator(card)).toHaveCount(1, { timeout: 15_000 });
  await expect(card.locator(".board-card-title .board-title")).toHaveText(FIRST);
  await expect(card.locator("[data-board-ptr]")).toHaveText(/^#\d+$/);
  await expect(card.locator("[data-board-live]")).toHaveAttribute("data-board-live", "open");
  await expect(card.locator("[data-board-who]")).toHaveText("alice");
});

test("a row is added through the picker, and editing its note keeps what it points at", async () => {
  test.setTimeout(90_000);
  const now = zone(alice, "Now");
  await now.locator("[data-board-add-row]").click();
  const form = now.locator("[data-board-form]");
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

test("New issue… in a zone's menu makes a native issue and a card row for it", async () => {
  test.setTimeout(90_000);
  const now = zone(alice, "Now");
  await now.locator("[data-board-zone-menu]").click();
  await alice.getByRole("menuitem", { name: "New issue…" }).click();
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
