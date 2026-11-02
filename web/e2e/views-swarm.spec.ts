// #503: building a View by hand in the swarm, across machines. Hold a block
// and drop it on the stage; flick one at it; sweep several; drag a leaf off
// the edge and it goes home. A quick drag still pans.

import { expect, test, type Page } from "@playwright/test";
import { FakeFleet } from "./fake-fleet";
import { closeContexts } from "./helpers";

test.afterAll(closeContexts);

let fake: FakeFleet;

test.use({ baseURL: async ({}, use) => use(fake?.machines[0]?.url) });
test.describe.configure({ mode: "serial" });

test.beforeAll(async () => {
  fake = new FakeFleet();
  await fake.machine("workstation");
  await fake.machine("build-01");
  await fake.machine("build-02");
  await fake.populate();
});

test.afterAll(() => fake?.stop());

async function swarm(page: Page) {
  await page.goto("/#swarm");
  await expect
    .poll(() => page.evaluate(() => window.__arugula?.fleet.list.filter((h) => h.state === "connected").length ?? 0), { timeout: 20_000 })
    .toBe(3);
  await expect.poll(() => page.evaluate(() => window.__arugula.fleet.panes.length)).toBeGreaterThanOrEqual(8);
  await expect(page.locator(".swarm")).toBeVisible();
  // The tiles have settled once a pane stops moving.
  await page.waitForTimeout(1200);
}

/** A terminal's tile on a machine, where it is on the page. */
async function tile(page: Page, host: string, skip = 0) {
  return page.evaluate(
    ([host, skip]) => {
      const list = window.__arugula.fleet.panes.filter((p) => p.host === host && p.info.type === "terminal" && !p.stale);
      const p = list[skip as number];
      const s = (window.__arugula as unknown as { swarm: { screenOf(k: string): { x: number; y: number } | null } }).swarm.screenOf(p.key)!;
      return { key: p.key, id: p.id, leaf: `[data-view-leaf="${p.host}.${p.id}"]`, x: s.x, y: s.y };
    },
    [host, skip] as const,
  );
}

const leaves = (page: Page) => page.locator("[data-view-leaf]");
const stageArea = (page: Page) => page.locator("[data-view-stage] .view-area").boundingBox().then((b) => b!);

async function hold(page: Page, at: { x: number; y: number }) {
  await page.mouse.move(at.x, at.y);
  await page.mouse.down();
  // Longer than the hold to grab.
  await page.waitForTimeout(220);
}

test("hold a block, drop it on the stage; a second machine's splits it", async ({ page }) => {
  await swarm(page);
  await expect(page.locator("[data-view-stage]")).toHaveCount(0);

  const a = await tile(page, "workstation");
  await hold(page, a);
  // Picking it up wakes the stage.
  const stage = page.locator("[data-view-stage]");
  await expect(stage).toBeVisible();
  await expect(stage).toHaveClass(/awake/);
  const box = await stageArea(page);
  await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2, { steps: 12 });
  await expect(page.locator("[data-view-zone]")).toBeVisible();
  await page.mouse.up();
  await expect(leaves(page)).toHaveCount(1);
  await expect(stage).not.toHaveClass(/awake/);

  // From the other machine, onto the right half of that leaf.
  const b = await tile(page, "build-01");
  await hold(page, b);
  await page.mouse.move(box.x + box.width - 16, box.y + box.height / 2, { steps: 12 });
  await page.mouse.up();
  await expect(leaves(page)).toHaveCount(2);
  await expect(page.locator(a.leaf).locator(".view-leaf-host")).toHaveText("workstation");
  await expect(page.locator(b.leaf).locator(".view-leaf-host")).toHaveText("build-01");

  // They end up side by side, once the springs settle.
  await expect
    .poll(async () => {
      const [l, r] = [await page.locator(a.leaf).boundingBox(), await page.locator(b.leaf).boundingBox()];
      return !!l && !!r && Math.abs(l.x + l.width - r.x) < 2 && Math.abs(l.y - r.y) < 2;
    })
    .toBe(true);
});

test("a flick at the stage lands with no aiming", async ({ page }) => {
  await swarm(page);
  await page.locator("[data-view-stage-toggle]").click();
  const box = await stageArea(page);
  const before = await leaves(page).count();
  const c = await tile(page, "workstation", 1);
  await hold(page, c);
  const steps = 6;
  // Fast, and stopping short of the stage.
  const toX = box.x - 40;
  for (let i = 1; i <= steps; i++) {
    await page.mouse.move(c.x + ((toX - c.x) * i) / steps, c.y + ((box.y + box.height / 2 - c.y) * i) / steps);
    await page.waitForTimeout(8);
  }
  await page.mouse.up();
  await expect(leaves(page)).toHaveCount(before + 1);
});

test("a sweep carries several; off the edge a leaf goes home, and a quick drag still pans", async ({ page }) => {
  await swarm(page);
  await page.locator("[data-view-stage-toggle]").click();
  const box = await stageArea(page);

  // Something to tear out.
  await hold(page, await tile(page, "workstation", 1));
  await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2, { steps: 10 });
  await page.mouse.up();
  await expect(leaves(page)).toHaveCount(1);

  // Off the edge, past the tear distance: home, and still running.
  const count = await leaves(page).count();
  const head = await leaves(page).first().locator(".view-leaf-head").boundingBox();
  await page.mouse.move(head!.x + 40, head!.y + 10);
  await page.mouse.down();
  await page.mouse.move(box.x - 260, box.y + 200, { steps: 10 });
  await page.mouse.up();
  await expect(leaves(page)).toHaveCount(count - 1);
  const alive = await page.evaluate(() => window.__arugula.fleet.panes.length);
  expect(alive).toBeGreaterThanOrEqual(8);

  // Escape puts a carried block back.
  await hold(page, await tile(page, "build-02"));
  await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2, { steps: 6 });
  await page.keyboard.press("Escape");
  await page.mouse.up();
  await expect(leaves(page)).toHaveCount(count - 1);
  await expect(page.locator(".swarm")).toBeVisible();

  // Shift-drag sweeps; holding one carries them all.
  const [p, q] = [await tile(page, "build-01", 1), await tile(page, "build-01", 2)];
  // From just inside one block to past the other: starting on a block still
  // sweeps (a Shift-click alone would toggle it).
  const [dx, dy] = [q.x >= p.x ? 1 : -1, q.y >= p.y ? 1 : -1];
  await page.keyboard.down("Shift");
  await page.mouse.move(p.x - dx * 4, p.y - dy * 3);
  await page.mouse.down();
  await page.mouse.move(q.x + dx * 12, q.y + dy * 12, { steps: 6 });
  await page.mouse.up();
  await page.keyboard.up("Shift");
  const swept = await page.evaluate(() => (window.__arugula as unknown as { swarm: { swept: string[] } }).swarm.swept);
  expect(swept).toEqual(expect.arrayContaining([p.key, q.key]));
  const n = await leaves(page).count();
  await hold(page, p);
  await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2, { steps: 10 });
  await page.mouse.up();
  await expect(leaves(page)).toHaveCount(n + swept.length);

  // A quick drag on a tile pans the field, as it always did.
  const t = await tile(page, "workstation");
  await page.mouse.move(t.x, t.y);
  await page.mouse.down();
  await page.mouse.move(t.x + 60, t.y + 30, { steps: 3 });
  await page.mouse.up();
  await expect.poll(async () => (await tile(page, "workstation")).x).toBeGreaterThan(t.x + 30);
  await expect(leaves(page)).toHaveCount(n + swept.length);

  // Open fills the window with it, and what was on the stage is there.
  await page.locator("[data-view-open]").click();
  await expect(page).toHaveURL(/#view=[a-z0-9]+$/);
  await expect(page.locator("[data-view-page]")).toBeVisible();
  await expect(leaves(page)).toHaveCount(n + swept.length);
});
