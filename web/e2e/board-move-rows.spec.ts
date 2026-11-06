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
const g = new TeamControl("movegroups");

test.use({ viewport: { width: 1280, height: 900 } });

test.afterAll(async ({ browser }) => {
  await closeContexts({ browser });
  t.stop();
  g.stop();
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
  await alice.evaluate((tm) => window.__arugula.control!.setLabsFlag("chat", true, tm), team);
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
  await alice.getByRole("menuitem", { name: "Move to" }).click();
  await shot(alice, "1280-menu");
  await alice.getByRole("menuitem", { name: "Next", exact: true }).click();
  await expect(lane(alice, titles[1])).toHaveAttribute("data-board-zone", "lane_next");
  await open(alice);
  await expect(lane(alice, titles[1])).toHaveAttribute("data-board-zone", "lane_next");
  await expect(card(alice, titles[1]).locator("[data-board-lane-set]")).toContainText(/^set by /);
  expect(await state(titles[1])).toBe("open");

  // The drawer's selector: Doing for the third; out of Done reopens the first.
  await card(alice, titles[2]).locator(".board-sub").click({ position: { x: 4, y: 4 } });
  const select = alice.locator("[data-board-drawer-lane]");
  await expect(select).toHaveValue("next");
  await select.selectOption("doing");
  await expect(lane(alice, titles[2])).toHaveAttribute("data-board-zone", "lane_doing");
  await expect(alice.locator("[data-board-drawer] [data-board-lane-set]")).toContainText("set by");
  await shot(alice, "1280-drawer");
  await alice.locator("[data-board-drawer-close]").click();
  await card(alice, titles[0]).locator(".board-sub").click({ position: { x: 4, y: 4 } });
  await alice.locator("[data-board-drawer-lane]").selectOption("review");
  await expect(lane(alice, titles[0])).toHaveAttribute("data-board-zone", "lane_review");
  expect(await state(titles[0])).toBe("open");

  // A phone has the menu and the drawer, and no dragging.
  await alice.setViewportSize({ width: 390, height: 844 });
  await open(alice);
  await expect(card(alice, titles[1])).not.toHaveAttribute("draggable", "true");
  await shot(alice, "390-board");
  await card(alice, titles[1]).locator(".board-sub").click({ position: { x: 4, y: 4 } });
  await shot(alice, "390-drawer");
});

// A row is dragged between groups (#664). The group under the card takes the
// drop, outlined whole, and none of its lanes does; the row keeps its lane.
// Within its own group a drag still picks a lane, and no group outlines.
test("a row dragged onto another group moves into it, lane unchanged", async ({ browser }) => {
  test.setTimeout(240_000);
  await g.start(["--platform-host", "app.localhost"]);
  const app = g.base.replace("//127.0.0.1", "//app.localhost");
  const alice = await g.person(browser, "alice");
  await alice.emulateMedia({ colorScheme: "dark" });
  await alice.evaluate(() => window.__arugula.control!.createTeam("Acme"));
  await alice.goto(`${app}/`);
  await alice.locator("section[data-platform-home] [data-project]", { hasText: "Acme" }).click();
  const project = /\/p\/(prj_[0-9a-f]+)/.exec(alice.url())![1];

  const board = (await api(alice, "POST", `/api/projects/${project}/boards`, { title: "Phases" })).body!.board.id as string;
  const entries = `/api/boards/${board}/entries`;
  await api(alice, "PUT", entries, { entry: "zone", title: "Decide" });
  const both = await api(alice, "PUT", entries, { entry: "zone", title: "Build" });
  const [decide] = (both.body!.groups as { id: string }[]).map((x) => x.id);
  let view: any;
  for (const [note, zone] of [["Pick a name", decide], ["Wire it up", undefined], ["Loose end", undefined]] as const) {
    view = (await api(alice, "PUT", entries, { entry: "row", note, zone })).body;
  }
  const rowOf = (note: string) => view.zones.flatMap((z: any) => z.rows).find((r: any) => r.note === note).id as string;
  const card = (note: string) => alice.locator(`[data-board-row="${rowOf(note)}"]`);
  const group = (id: string) => alice.locator(`[data-board-group="${id}"]`);
  const laneOf = async (note: string) => card(note).getAttribute("data-lane");
  const open = async () => {
    await alice.goto(`${app}/p/${project}/boards/${board}`);
    await expect(alice.locator("[data-board-row]")).toHaveCount(3);
  };
  await open();
  const lane = await laneOf("Loose end");

  // Mid-drag over Decide: the group outlines, and none of its lanes.
  await alice.setViewportSize({ width: 1280, height: 1200 });
  await card("Loose end").scrollIntoViewIfNeeded();
  const from = (await card("Loose end").boundingBox())!;
  await alice.mouse.move(from.x + from.width / 2, from.y + from.height / 2);
  await alice.mouse.down();
  await alice.mouse.move(from.x + from.width / 2 + 10, from.y + from.height / 2 + 10, { steps: 3 });
  const head = (await group(decide).locator(".section-head").boundingBox())!;
  await alice.mouse.move(head.x + 60, head.y + head.height / 2, { steps: 8 });
  // Chromium delivers the first dragover only after a few moves inside the target.
  await alice.mouse.move(head.x + 70, head.y + head.height / 2, { steps: 4 });
  await alice.mouse.move(head.x + 80, head.y + head.height / 2, { steps: 4 });
  await expect(group(decide)).toHaveAttribute("data-drop-over", "");
  await expect(group(decide).locator("[data-board-zone][data-drop-over]")).toHaveCount(0);
  // Over one of its lanes the group still takes it, and the lane doesn't.
  const lanebox = (await group(decide).locator('[data-board-zone="lane_doing"]').boundingBox())!;
  await alice.mouse.move(lanebox.x + 40, lanebox.y + 20, { steps: 6 });
  await alice.mouse.move(lanebox.x + 50, lanebox.y + 22, { steps: 4 });
  await expect(group(decide)).toHaveAttribute("data-drop-over", "");
  await expect(group(decide).locator("[data-board-zone][data-drop-over]")).toHaveCount(0);
  await shot(alice, "group-outline");
  await alice.mouse.up();
  await expect(group(decide).locator(`[data-board-row="${rowOf("Loose end")}"]`)).toHaveCount(1);
  await open();
  await expect(group(decide).locator(`[data-board-row="${rowOf("Loose end")}"]`)).toHaveCount(1);
  expect(await laneOf("Loose end")).toBe(lane);
  await expect(alice.locator("[data-drop-over]")).toHaveCount(0);

  // Within its own group a lane is still the target, and the group is not.
  const own = alice.locator(`[data-board-group="${decide}"] [data-board-zone="lane_doing"]`);
  const src = (await card("Loose end").boundingBox())!;
  await alice.mouse.move(src.x + src.width / 2, src.y + src.height / 2);
  await alice.mouse.down();
  await alice.mouse.move(src.x + src.width / 2 + 10, src.y + src.height / 2 + 10, { steps: 3 });
  const dst = (await own.boundingBox())!;
  await alice.mouse.move(dst.x + 40, dst.y + 20, { steps: 8 });
  await alice.mouse.move(dst.x + 50, dst.y + 22, { steps: 4 });
  await alice.mouse.move(dst.x + 60, dst.y + 24, { steps: 4 });
  await expect(own).toHaveAttribute("data-drop-over", "");
  await expect(group(decide)).not.toHaveAttribute("data-drop-over", "");
  await alice.mouse.up();
  await expect(group(decide).locator(`[data-board-zone="lane_doing"] [data-board-row="${rowOf("Loose end")}"]`)).toHaveCount(1);

  // A grouped row onto "No group" lands there, in its lane.
  const doing = await laneOf("Pick a name");
  await card("Pick a name").dragTo(group("none").locator(".section-head"));
  await open();
  await expect(group("none").locator(`[data-board-row="${rowOf("Pick a name")}"]`)).toHaveCount(1);
  expect(await laneOf("Pick a name")).toBe(doing);
});
