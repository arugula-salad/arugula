// A board's progress bar shows Done, Review and Doing (#541), on the Boards
// page and in the board's header. The responses are given the progress, as
// the platform's lanes would count it. A local control (team-fixture.ts).

import { expect, test, type Page } from "@playwright/test";
import { closeContexts } from "./helpers";
import { TeamControl } from "./team-fixture";

const t = new TeamControl("progresslanes");

test.afterAll(async ({ browser }) => {
  await closeContexts({ browser });
  t.stop();
});

const SHOTS = process.env.EPIC_SHOTS;

async function shot(p: Page, name: string) {
  if (SHOTS) await p.screenshot({ path: `${SHOTS}/${name}.png`, animations: "disabled", fullPage: true });
}

test("progress lanes: the bar's segments and words on the list and in the header", async ({ browser }) => {
  test.setTimeout(180_000);
  await t.start(["--platform-host", "app.localhost"]);
  const app = t.base.replace("//127.0.0.1", "//app.localhost");
  const alice = await t.person(browser, "alice");
  await alice.evaluate(() => window.__arugula.control!.createTeam("Acme"));
  const team = await alice.evaluate(() => window.__arugula.control!.teams[0].team);
  for (const flag of ["chat", "boards"]) await alice.evaluate(([f, tm]) => window.__arugula.control!.setLabsFlag(f, true, tm), [flag, team] as const);
  await alice.goto(`${app}/`);
  await alice.locator("section[data-platform-home] [data-project]", { hasText: "Acme" }).click();
  const project = /\/p\/(prj_[0-9a-f]+)/.exec(alice.url())![1];

  const ids: Record<string, string> = {};
  for (const title of ["Launch", "Only done"]) {
    ids[title] = await alice.evaluate(
      async ([u, ti]) => {
        const r = await fetch(u, { method: "POST", credentials: "same-origin", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ title: ti }) });
        return ((await r.json()) as { board: { id: string } }).board.id;
      },
      [`/api/projects/${project}/boards`, title] as const,
    );
  }
  const progress: Record<string, object> = {
    [ids.Launch]: { done: 3, total: 10, proven: 2, review: 2, doing: 1 },
    [ids["Only done"]]: { done: 2, total: 4, proven: 0, review: 0, doing: 0 },
  };
  await alice.route(new RegExp(`/api/projects/${project}/boards$`), async (route) => {
    const res = await route.fetch();
    const v = await res.json();
    for (const b of v.boards) if (progress[b.id]) b.progress = progress[b.id];
    await route.fulfill({ response: res, json: v });
  });
  await alice.route(new RegExp(`/api/boards/brd_[0-9a-f]+$`), async (route) => {
    if (route.request().method() !== "GET") return route.fallback();
    const res = await route.fetch();
    const v = await res.json();
    if (progress[v.board.id]) v.board.progress = progress[v.board.id];
    await route.fulfill({ response: res, json: v });
  });

  for (const [w, h, scheme] of [[1280, 900, "dark"], [1280, 900, "light"], [390, 844, "dark"]] as const) {
    await alice.setViewportSize({ width: w, height: h });
    await alice.emulateMedia({ colorScheme: scheme });
    await alice.goto(`${app}/p/${project}/boards`);
    const card = alice.locator("[data-board-progress]", { has: alice.locator("[data-lane]") }).first();
    await expect(alice.locator("[data-board-progress]").filter({ hasText: "3 of 10 done, 2 proven · 2 in review · 1 doing" })).toBeVisible({ timeout: 15_000 });
    await expect(alice.locator("[data-board-progress]").filter({ hasText: /^2 of 4 done, 0 proven$/ })).toBeVisible();
    await expect(card).toBeVisible();
    const bar = alice.locator("[data-board-progress]").filter({ hasText: "in review" }).getByRole("progressbar");
    await expect(bar.locator("i")).toHaveCount(3);
    expect(await bar.locator("i").evaluateAll((is) => is.map((i) => [i.getAttribute("data-lane"), (i as HTMLElement).style.width]))).toEqual([["done", "30%"], ["review", "20%"], ["doing", "10%"]]);
    await expect(bar).toHaveAttribute("aria-valuenow", "3");
    await shot(alice, `lanes-list-${w}-${scheme}`);

    await alice.goto(`${app}/p/${project}/boards/${ids.Launch}`);
    const head = alice.locator(".board-progress");
    await expect(head).toContainText("3 of 10 done, 2 proven · 2 in review · 1 doing", { timeout: 15_000 });
    await expect(head.locator("i")).toHaveCount(3);
    await shot(alice, `lanes-header-${w}-${scheme}`);
  }
});
