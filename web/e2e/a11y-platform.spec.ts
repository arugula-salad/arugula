// Accessibility (#401) on the platform page: projects, a project's home,
// issues, an issue, triage, boards and a board, Docs and a Doc, a chat
// room, the inbox, every project's boards, the project's settings and the
// account's, each at 1440 and 400 px, light and dark. Alice owns team
// Acme, with chat's and boards' Labs flags on, three issues (one on the
// board Launch), a Doc and #general with a message in it. A local control
// (team-fixture.ts, `--platform-host app.localhost`), as
// boards-platform.spec.ts runs it. What's checked and the known list:
// a11y.ts.

import { expect, test, type Locator, type Page } from "@playwright/test";
import { everyWay, prepare } from "./a11y";
import { closeContexts } from "./helpers";
import { TeamControl, home } from "./team-fixture";

// Four loads and scans a test.
test.describe.configure({ timeout: 90_000 });
const t = new TeamControl("a11y");

let alice: Page;
let app = "";
/** The project's path, `/p/prj_…-acme`. */
let project = "";
let issuePath = "";
let boardPath = "";
let docPath = "";
let room = "";

async function api(p: Page, method: string, path: string, body?: unknown) {
  return p.evaluate(
    async ([m, u, b]) => {
      const r = await fetch(u as string, { method: m as string, credentials: "same-origin", headers: { "Content-Type": "application/json" }, body: b === undefined ? undefined : JSON.stringify(b) });
      return { status: r.status, body: (await r.json().catch(() => null)) as Record<string, any> | null };
    },
    [method, path, body] as const,
  );
}

async function platform(path: string) {
  await alice.goto(`${app}${path}`);
  await expect(alice.locator("[data-platform-bar], [data-chat]").first()).toBeVisible({ timeout: 15_000 });
}

test.beforeAll(async ({ browser }) => {
  test.setTimeout(180_000);
  await t.start(["--platform-host", "app.localhost"]);
  app = t.base.replace("//127.0.0.1", "//app.localhost");
  alice = await t.person(browser, "alice");
  await alice.evaluate(() => window.__arugula.control!.createTeam("Acme"));
  const team = await alice.evaluate(() => window.__arugula.control!.teams[0].team);
  for (const flag of ["chat", "boards"]) await alice.evaluate(([f, tm]) => window.__arugula.control!.setLabsFlag(f, true, tm), [flag, team] as const);

  // #general, with a line in it.
  await home(alice);
  await expect.poll(() => alice.evaluate(() => window.__arugula.chat.on), { timeout: 15_000 }).toBe(true);
  room = await alice.evaluate((tm) => window.__arugula.chat.newChannel(tm, "general").then((r) => r.id), team);
  await alice.evaluate((r) => window.__arugula.chat.post(r, "The release is cut from main."), room);

  await platform("/");
  await alice.locator("section[data-platform-home] [data-project]", { hasText: "Acme" }).click();
  await expect(alice).toHaveURL(/\/p\/prj_[0-9a-f]+-acme$/);
  project = new URL(alice.url()).pathname;
  const id = /\/p\/(prj_[0-9a-f]+)/.exec(project)![1];

  // Three issues, the first Alice's.
  const me = (await api(alice, "GET", "/api/projects")).body!.me.account as string;
  for (const [title, assignees] of [["Fix the installer", [me]], ["Tidy the changelog", []], ["Update the screenshots", []]] as const)
    expect((await api(alice, "POST", `/api/projects/${id}/issues`, { title, body: "The release is cut from main.", labels: [], assignees })).status).toBe(200);
  await platform(`${project}/issues`);
  await alice.locator("[data-issue-row] .issue-row-title", { hasText: "Fix the installer" }).click();
  await expect(alice).toHaveURL(/\/issues\/iss_[0-9a-f]{32}-/, { timeout: 15_000 });
  issuePath = new URL(alice.url()).pathname;

  // The board Launch, with the first issue in Next.
  await platform(`${project}/boards`);
  await alice.locator("[data-board-new-title]").fill("Launch");
  await alice.locator("[data-board-new]").click();
  await expect(alice.locator("[data-board-page] [data-board-title]")).toHaveText("Launch", { timeout: 15_000 });
  boardPath = new URL(alice.url()).pathname;
  await platform(`${project}/triage`);
  await alice.locator("[data-triage-row]", { hasText: "Fix the installer" }).locator("[data-triage-add]").click();
  await alice.getByRole("menuitem", { name: "Launch" }).click();
  await expect(alice.locator("[data-triage-row]")).toHaveCount(2);

  // A Doc with a section.
  await platform(`${project}/docs`);
  await alice.locator("[data-doc-new-title]").fill("Release plan");
  await alice.locator("[data-doc-new]").click();
  await expect(alice).toHaveURL(/\/docs\/pdc_[0-9a-f]{32}-release-plan$/, { timeout: 15_000 });
  docPath = new URL(alice.url()).pathname;
  const doc = alice.locator("[data-doc-page]");
  await doc.locator("[data-doc-heading-input]").fill("Goals");
  await doc.locator("[data-doc-markdown-input]").fill("Ship **0.64** this week.");
  await doc.locator("[data-doc-save]").click();
  await expect(doc.locator(".doc-section[data-section=s1] .doc-heading")).toHaveText("Goals");

  await prepare(alice);
});

test.afterAll(async ({ browser }) => {
  await closeContexts({ browser });
  t.stop();
});

/** Scan the platform page at `path` once `ready` shows. */
const place = (name: string, path: () => string, ready: (p: Page) => Locator) =>
  test(name, async () => {
    await everyWay(alice, name, async () => {
      await platform(path());
      await expect(ready(alice).first()).toBeVisible({ timeout: 15_000 });
    });
  });

place("platform-home", () => "/", (p) => p.locator("section[data-platform-home] [data-project]"));
place("platform-inbox", () => "/inbox", (p) => p.locator("[data-platform-inbox]"));
place("platform-settings", () => "/settings", (p) => p.locator("[data-platform-settings]"));
place("project", () => project, (p) => p.locator("[data-project-home]"));
place("project-settings", () => `${project}/settings`, (p) => p.locator("[data-project-settings]"));
place("issues", () => `${project}/issues`, (p) => p.locator("[data-issue-row]"));
place("issue", () => issuePath, (p) => p.locator("[data-issue-title]", { hasText: "Fix the installer" }));
place("triage", () => `${project}/triage`, (p) => p.locator("[data-triage-row]"));
place("boards", () => `${project}/boards`, (p) => p.locator("[data-board-link]"));
place("board", () => boardPath, (p) => p.locator("[data-board-row]"));
place("all-boards", () => "/boards", (p) => p.locator("[data-all-boards-page] [data-board-link]"));
place("project-docs", () => `${project}/docs`, (p) => p.locator("[data-doc-link]"));
place("project-doc", () => docPath, (p) => p.locator("[data-doc-page] .doc-section[data-section=s1]"));
place("chat", () => `/chat/${room}`, (p) => p.locator("[data-room-view]").getByText("The release is cut from main."));
