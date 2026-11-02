// A person moves a row between lanes (#494). Alice owns team Acme, with a
// board of three rows on native issues; two have an open PR attached (so
// they sit in Review), written into the platform's database as the GitHub
// mirror would leave them. She drags the first to Done and confirms: its
// issue is closed. She moves the second to Next from its ⋯ menu: it stays
// there after a reload and says it was set by her. In the drawer's lane
// selector she moves the third to Doing, and the first out of Done, which
// opens its issue again. A local control (team-fixture.ts).

import { spawnSync } from "node:child_process";
import { mkdirSync } from "node:fs";
import { expect, test, type Page } from "@playwright/test";
import { closeContexts } from "./helpers";
import { TeamControl } from "./team-fixture";

const t = new TeamControl("moverows");

test.afterAll(async ({ browser }) => {
  await closeContexts({ browser });
  t.stop();
});

const SHOTS = `${process.env.TMPDIR ?? "/tmp"}/494-shots`;

async function shot(p: Page, name: string) {
  mkdirSync(SHOTS, { recursive: true });
  await p.screenshot({ path: `${SHOTS}/${name}.png`, animations: "disabled", fullPage: true });
}

const sql = (q: string) => {
  const r = spawnSync("sqlite3", ["-cmd", ".timeout 5000", t.db, q], { encoding: "utf8" });
  expect(r.status, r.stderr).toBe(0);
};

async function api(p: Page, method: string, path: string, body?: unknown) {
  return p.evaluate(
    async ([m, u, b]) => {
      const r = await fetch(u as string, { method: m as string, credentials: "same-origin", headers: { "Content-Type": "application/json" }, body: b === undefined ? undefined : JSON.stringify(b) });
      return { status: r.status, body: (await r.json().catch(() => null)) as Record<string, any> | null };
    },
    [method, path, body] as const,
  );
}

test("a row is moved by dragging, from its menu and from the drawer", async ({ browser }) => {
  test.setTimeout(240_000);
  await t.start(["--platform-host", "app.localhost"]);
  const app = t.base.replace("//127.0.0.1", "//app.localhost");
  const alice = await t.person(browser, "alice");
  await alice.emulateMedia({ colorScheme: "dark" });
  await alice.evaluate(() => window.__arugula.control!.createTeam("Acme"));
  const team = await alice.evaluate(() => window.__arugula.control!.teams[0].team);
  for (const flag of ["chat", "boards"]) await alice.evaluate(([f, tm]) => window.__arugula.control!.setLabsFlag(f, true, tm), [flag, team] as const);
  await alice.goto(`${app}/`);
  await alice.locator("section[data-platform-home] [data-project]", { hasText: "Acme" }).click();
  const project = /\/p\/(prj_[0-9a-f]+)/.exec(alice.url())![1];

  const board = (await api(alice, "POST", `/api/projects/${project}/boards`, { title: "Launch" })).body!.board.id as string;
  const titles = ["Fix the installer", "Tidy the changelog", "Update the screenshots"];
  const issue: Record<string, string> = {};
  const rowOf: Record<string, string> = {};
  for (const title of titles) {
    const i = await api(alice, "POST", `/api/projects/${project}/issues`, { title });
    issue[title] = i.body!.id;
    const v = await api(alice, "PUT", `/api/boards/${board}/entries`, { entry: "row", pointer: { kind: "issue", id: i.body!.id } });
    expect(v.status).toBe(200);
    rowOf[title] = v.body!.zones.flatMap((z: any) => z.rows).find((r: any) => r.pointer.id === i.body!.id).id;
  }
  // Two of them have an open PR: Review.
  sql(`INSERT INTO linked_repos (repo_id, full_name, project_id, installation_id, linked_at) VALUES (7, 'acme/app', '${project}', 1, 1);
    INSERT INTO forge_items (id, repo_id, number, kind, state, title, created_at, updated_at, synced_at) VALUES
      ('fi_0000000000000000000000000000a001', 7, 1, 'pr', 'open', 'Installer', 1, 1, 1),
      ('fi_0000000000000000000000000000a002', 7, 2, 'pr', 'open', 'Changelog', 1, 1, 1);
    INSERT INTO board_attachments (id, board_id, row_id, role, target_kind, target, created_at) VALUES
      ('att_0000000000000000000000000000a001', '${board}', '${rowOf[titles[0]]}', 'work', 'forge_item', 'fi_0000000000000000000000000000a001', 1),
      ('att_0000000000000000000000000000a002', '${board}', '${rowOf[titles[1]]}', 'work', 'forge_item', 'fi_0000000000000000000000000000a002', 1);`);

  const open = async (p: Page) => {
    await p.goto(`${app}/p/${project}/boards/${board}`);
    await expect(p.locator("[data-board-row]")).toHaveCount(3);
  };
  const lane = (p: Page, title: string) => p.locator(`[data-board-zone] [data-board-row="${rowOf[title]}"]`).locator("xpath=ancestor::*[@data-board-zone][1]");
  const card = (p: Page, title: string) => p.locator(`[data-board-row="${rowOf[title]}"]`);
  const state = async (title: string) => (await api(alice, "GET", `/api/issues/${issue[title]}`)).body!.state;
  await open(alice);
  await expect(lane(alice, titles[0])).toHaveAttribute("data-board-zone", "lane_review");
  await expect(lane(alice, titles[2])).toHaveAttribute("data-board-zone", "lane_next");
  await shot(alice, "1280-before");

  // Dragging the first to Done asks, and closing it moves the row.
  await card(alice, titles[0]).dragTo(alice.locator('[data-board-zone="lane_done"]'));
  await expect(alice.locator("[data-confirm-dialog]")).toContainText("Close #");
  await shot(alice, "1280-close-dialog");
  await alice.locator("[data-confirm-remove]").click();
  await expect(lane(alice, titles[0])).toHaveAttribute("data-board-zone", "lane_done");
  expect(await state(titles[0])).toBe("closed");

  // ⋯ → Move to → Next on the second: it stays through a reload, marked.
  await card(alice, titles[1]).locator("[data-board-row-menu]").click();
  await alice.getByRole("menuitem", { name: "Move to" }).hover();
  await shot(alice, "1280-menu");
  await alice.getByRole("menuitem", { name: "Next", exact: true }).click();
  await expect(lane(alice, titles[1])).toHaveAttribute("data-board-zone", "lane_next");
  await open(alice);
  await expect(lane(alice, titles[1])).toHaveAttribute("data-board-zone", "lane_next");
  await expect(card(alice, titles[1]).locator("[data-board-lane-set]")).toContainText(/^set by /);
  expect(await state(titles[1])).toBe("open");

  // The drawer's selector: Doing for the third; out of Done reopens the first.
  await card(alice, titles[2]).locator("[data-board-title]").click();
  const select = alice.locator("[data-board-drawer-lane]");
  await expect(select).toHaveValue("next");
  await select.selectOption("doing");
  await expect(lane(alice, titles[2])).toHaveAttribute("data-board-zone", "lane_doing");
  await expect(alice.locator("[data-board-drawer] [data-board-lane-set]")).toContainText("set by");
  await shot(alice, "1280-drawer");
  await alice.locator("[data-board-drawer-close]").click();
  await card(alice, titles[0]).locator("[data-board-title]").click();
  await alice.locator("[data-board-drawer-lane]").selectOption("review");
  await expect(lane(alice, titles[0])).toHaveAttribute("data-board-zone", "lane_review");
  expect(await state(titles[0])).toBe("open");

  // A phone has the menu and the drawer, and no dragging.
  await alice.setViewportSize({ width: 390, height: 844 });
  await open(alice);
  await expect(card(alice, titles[1])).not.toHaveAttribute("draggable", "true");
  await shot(alice, "390-board");
  await card(alice, titles[1]).locator("[data-board-title]").click();
  await shot(alice, "390-drawer");
});
