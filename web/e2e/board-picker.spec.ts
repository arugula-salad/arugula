// #545: Open beside → Board… with dozens of boards. Alice has a team Acme with
// a terminal; the project's boards response is given 59 more boards (people's,
// then started and not started epics, and a closed one), as a Plane sync would
// make. The item reads "Board…"; the list scrolls inside the screen, typing
// narrows it (a title, or an epic's key), the arrow keys and Enter pick, and
// the last board, reached by scrolling, opens a block. A local control
// (team-fixture.ts).

import { expect, test, type Page } from "@playwright/test";
import { closeContexts, menuItem } from "./helpers";
import { TeamControl, call, home, live, show } from "./team-fixture";

const t = new TeamControl("boardpicker");

test.afterAll(async ({ browser }) => {
  await closeContexts({ browser });
  t.stop();
});

const SHOTS = process.env.PICKER_SHOTS;
const hex = (n: number) => n.toString(16).padStart(32, "0");

async function shot(p: Page, name: string) {
  if (SHOTS) await p.screenshot({ path: `${SHOTS}/${name}.png`, animations: "disabled" });
}

test("Board…: a list of 60 scrolls inside the screen, filters as you type and opens a block", async ({ browser }) => {
  test.setTimeout(240_000);
  await t.start(["--platform-host", "app.localhost"]);
  const alice = await t.person(browser, "alice");
  await alice.evaluate(() => window.__arugula.control!.createTeam("Acme"));
  const team = await alice.evaluate(() => window.__arugula.control!.teams[0].team);
  await home(alice);
  await t.addMachine(alice, "alices", team);
  await expect.poll(() => live(alice, ["alices"]), { timeout: 30_000 }).toBe(true);
  await show(alice, "alices");

  const api = (method: string, path: string, body?: unknown) =>
    alice.evaluate(
      async ([m, u, b]) => {
        const r = await fetch(u as string, { method: m as string, credentials: "same-origin", headers: { "Content-Type": "application/json" }, body: b === undefined ? undefined : JSON.stringify(b) });
        return { status: r.status, body: (await r.json().catch(() => null)) as Record<string, any> | null };
      },
      [method, path, body] as const,
    );
  const project = ((await api("GET", "/api/projects")).body!.projects as { id: string; name: string }[]).find((p) => p.name === "Acme")!.id;
  const made = await api("POST", `/api/projects/${project}/boards`, { title: "Zeta last" });
  expect(made.status).toBe(200);
  const real = made.body!.board.id as string;

  // The real board is a not started epic's: the last in the list. Before it,
  // 40 of people's, 18 of started epics, and a closed one the list leaves out.
  const source = (key: string, started: boolean) => ({ provider: "plane", key, url: `https://app.plane.so/acme/browse/${key}/`, connection: "Acme", left_out: 0, started });
  await alice.route(new RegExp(`/api/projects/${project}/boards$`), async (route) => {
    if (route.request().method() !== "GET") return route.fallback();
    const res = await route.fetch();
    const v = await res.json();
    const base = v.boards[0];
    const fake = (n: number, title: string, extra: Record<string, unknown> = {}) => ({ ...base, id: `brd_${hex(n)}`, title, source: undefined, closed_at: undefined, ...extra });
    const boards = [
      // Epics first in the response: the list orders them after people's.
      ...Array.from({ length: 18 }, (_, i) => fake(100 + i, `Epic ${i}`, { source: source(`ARUGU-${100 + i}`, true) })),
      fake(900, "Closed one", { closed_at: 1 }),
      ...Array.from({ length: 40 }, (_, i) => fake(200 + i, `Plan ${i}`)),
      { ...base, id: real, source: source("ARUGU-7", false) },
    ];
    await route.fulfill({ response: res, json: { ...v, boards } });
  });

  const run = await call(alice, "POST", "/api/run", { command: "sleep 600" });
  expect(run.status, JSON.stringify(run.body)).toBe(200);
  const pane = (run.body as { pane: number }).pane;
  await alice.bringToFront();
  await alice.evaluate((p) => window.__arugula.client.setActive(p), pane);

  const openPicker = async () => {
    await alice.locator(`[data-pane="${pane}"]`).click({ button: "right", position: { x: 60, y: 60 } });
    await expect(alice.getByRole("menuitem", { name: /Pin a board/ })).toHaveCount(0);
    await (await menuItem(alice, "Board…", { exact: true })).click();
    await expect(alice.getByRole("dialog", { name: "Board" })).toBeVisible();
    await expect(alice.locator(".prompt-choices button").first()).toBeVisible({ timeout: 15_000 });
  };
  const choices = alice.locator(".prompt-choices button");
  const inside = async () => {
    const list = (await alice.locator(".prompt-choices").boundingBox())!;
    const vh = alice.viewportSize()!.height;
    expect(list.y + list.height).toBeLessThanOrEqual(vh);
    expect(await alice.locator(".prompt-choices").evaluate((el) => el.scrollHeight > el.clientHeight)).toBe(true);
  };

  for (const [w, h] of [[1280, 900], [390, 844]] as const) {
    await alice.setViewportSize({ width: w, height: h });
    await alice.emulateMedia({ colorScheme: "dark" });
    await openPicker();
    // 40 + 18 + the real one; the closed board is left out. People's first.
    await expect(choices).toHaveCount(59);
    await expect(choices.first()).toHaveText("Acme · Plan 0");
    await expect(choices.nth(40)).toHaveText("Acme · ARUGU-100 · Epic 0");
    await expect(choices.last()).toHaveText("Acme · ARUGU-7 · Zeta last");
    await expect(alice.locator(".prompt-choices", { hasText: "Closed one" })).toHaveCount(0);
    await inside();
    await shot(alice, `picker-${w}-dark`);

    // Scrolled to the end, the last board is reachable.
    await choices.last().scrollIntoViewIfNeeded();
    await expect(choices.last()).toBeInViewport();
    await shot(alice, `picker-end-${w}-dark`);

    // Typing narrows: a title, or an epic's key, in any case.
    await alice.getByRole("dialog", { name: "Board" }).locator("input").fill("plan 3");
    await expect(choices).toHaveText(["Acme · Plan 3", "Acme · Plan 30", "Acme · Plan 31", "Acme · Plan 32", "Acme · Plan 33", "Acme · Plan 34", "Acme · Plan 35", "Acme · Plan 36", "Acme · Plan 37", "Acme · Plan 38", "Acme · Plan 39"]);
    await alice.getByRole("dialog", { name: "Board" }).locator("input").fill("arugu-10");
    await expect(choices).toHaveCount(10);
    await shot(alice, `picker-filtered-${w}-dark`);
    await alice.keyboard.press("Escape");
    await expect(alice.getByRole("dialog", { name: "Board" })).toHaveCount(0);
  }

  // Typing the key, then the arrows and Enter, pick the last board (on a
  // wide page: a phone shows one pane at a time).
  await alice.setViewportSize({ width: 1280, height: 900 });
  await openPicker();
  await alice.getByRole("dialog", { name: "Board" }).locator("input").fill("ARUGU-7");
  await expect(choices).toHaveText(["Acme · ARUGU-7 · Zeta last"]);
  await alice.keyboard.press("ArrowDown");
  await expect(choices.first()).toHaveClass(/active/);
  await alice.keyboard.press("Enter");
  await expect(alice.locator(`[data-board-block="${real}"]`)).toBeVisible({ timeout: 30_000 });

  // By a click, after scrolling to it: a second block of it.
  await openPicker();
  await choices.last().scrollIntoViewIfNeeded();
  await choices.last().click();
  await expect(alice.locator(`[data-board-block="${real}"]`)).toHaveCount(2, { timeout: 30_000 });
});
