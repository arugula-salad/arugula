// Labels and owners on a board's cards (#547). Alice owns team Acme with
// boards on. A board of four rows (the response is replaced, as
// board-find.spec.ts does) carries labels and assignees: a card shows three
// label chips and a +N, and two initials; the Label menu narrows the
// board and puts itself in the URL, and a reload keeps it; `label:` in the
// find box says the same. A local control (team-fixture.ts).

import { mkdirSync } from "node:fs";
import { expect, test, type Page } from "@playwright/test";
import { closeContexts } from "./helpers";
import { TeamControl } from "./team-fixture";

const t = new TeamControl("boardlabels");

test.afterAll(async ({ browser }) => {
  await closeContexts({ browser });
  t.stop();
});

const SHOTS = process.env.LABEL_SHOTS;
if (SHOTS) mkdirSync(SHOTS, { recursive: true });

async function shot(p: Page, name: string) {
  if (SHOTS) await p.screenshot({ path: `${SHOTS}/${name}.png`, animations: "disabled", fullPage: true });
}

const ROWS: { title: string; lane: string; labels: string[]; assignees: string[] }[] = [
  { title: "Search the board", lane: "next", labels: ["boards", "ui", "docs", "ci", "runs"], assignees: ["Ana Diaz", "bob", "carol"] },
  { title: "Fold the lanes", lane: "doing", labels: ["boards"], assignees: ["bob"] },
  { title: "Write the guide", lane: "review", labels: ["docs"], assignees: [] },
  { title: "Nothing on it", lane: "done", labels: [], assignees: [] },
];
const row = (n: number) => ({
  id: `row_${(n + 1).toString(16).padStart(8, "0")}`,
  rev: 1,
  pointer: { kind: "issue", id: `iss_${n}` },
  note: "",
  status: "none",
  lane: ROWS[n].lane,
  attachments: [],
  live: { state: ROWS[n].lane === "done" ? "closed" : "open", kind: "native", number: n + 1, title: ROWS[n].title, assignees: ROWS[n].assignees, labels: ROWS[n].labels },
  created_at: 1,
  updated_at: 1,
});

test("cards show labels and owners, the Label menu narrows the board and the URL keeps it", async ({ browser }) => {
  test.setTimeout(180_000);
  await t.start(["--platform-host", "app.localhost"]);
  const app = t.base.replace("//127.0.0.1", "//app.localhost");
  const alice = await t.person(browser, "alice");
  await alice.evaluate(() => window.__arugula.control!.createTeam("Acme"));
  const team = await alice.evaluate(() => window.__arugula.control!.teams[0].team);
  await alice.evaluate((tm) => window.__arugula.control!.setLabsFlag("chat", true, tm), team);
  await alice.goto(`${app}/`);
  await alice.locator("section[data-platform-home] [data-project]", { hasText: "Acme" }).click();
  const project = /\/p\/(prj_[0-9a-f]+)/.exec(alice.url())![1];
  const board = await alice.evaluate(async (p) => {
    const r = await fetch(`/api/projects/${p}/boards`, { method: "POST", credentials: "same-origin", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ title: "Labels" }) });
    return ((await r.json()) as { board: { id: string } }).board.id;
  }, project);
  await alice.route(new RegExp(`/api/boards/${board}$`), async (route) => {
    if (route.request().method() !== "GET") return route.fallback();
    const res = await route.fetch();
    const v = await res.json();
    for (const z of v.zones) z.rows = ROWS.map((_, i) => row(i)).filter((r) => `lane_${r.lane}` === z.id);
    await route.fulfill({ response: res, json: v });
  });

  for (const [w, h, name] of [[1280, 900, "wide"], [390, 844, "narrow"]] as const) {
    await alice.setViewportSize({ width: w, height: h });
    await alice.emulateMedia({ colorScheme: "dark" });
    await alice.goto(`${app}/p/${project}/boards/${board}`);
    await expect(alice.locator("[data-board-row]")).toHaveCount(4, { timeout: 15_000 });

    // Three chips and a +N; two initials and a +N. A phone's card is narrow: the +N of all five.
    const first = alice.locator("[data-board-row]", { hasText: "Search the board" });
    if (w > 700) {
      await expect(first.locator("[data-board-label]")).toHaveText(["boards", "ui", "docs"]);
      await expect(first.locator("[data-board-label-more]")).toHaveText("+2");
      await expect(first.locator("[data-board-label]").first()).toBeVisible();
      await expect(first.locator("[data-board-labels-all]")).toBeHidden();
    } else {
      await expect(first.locator("[data-board-labels-all]")).toHaveText("+5");
      await expect(first.locator("[data-board-labels-all]")).toBeVisible();
      await expect(first.locator("[data-board-label]").first()).toBeHidden();
    }
    await expect(first.locator("[data-board-avatar]")).toHaveText(["AD", "B"]);
    await expect(first.locator("[data-board-avatar-more]")).toHaveText("+1");
    await expect(alice.locator("[data-board-row]", { hasText: "Nothing on it" }).locator("[data-board-labels], [data-board-who]")).toHaveCount(0);
    await shot(alice, `cards-${name}`);

    // The Label menu: any of its values; the hash holds it; a reload keeps it.
    await alice.locator('[data-board-facet-button="label"]').click();
    await alice.locator('[data-board-facet-value="docs"]').check();
    await expect(alice.locator("[data-board-row]")).toHaveCount(2);
    await expect(alice.locator("[data-board-find-count]")).toHaveText("2 of 4 rows");
    await alice.locator('[data-board-facet-value="ui"]').check();
    await expect(alice.locator("[data-board-row]")).toHaveCount(2, { timeout: 5_000 });
    await alice.locator('[data-board-facet-value="ui"]').uncheck();
    await alice.locator('[data-board-facet-value="boards"]').check();
    await expect(alice.locator("[data-board-row]")).toHaveCount(3, { message: "any label of the menu: docs or boards" });
    await alice.locator('[data-board-facet-button="label"]').click();
    await alice.locator('[data-board-facet-button="person"]').click();
    await alice.locator('[data-board-facet-value="bob"]').check();
    await expect(alice.locator("[data-board-row]")).toHaveCount(2, { message: "across menus every one holds: (docs or boards) and bob" });
    await expect.poll(() => new URL(alice.url()).hash).toContain("label=docs&label=boards&person=bob");
    await shot(alice, `menu-${name}`);

    await alice.reload();
    await expect(alice.locator("[data-board-row]")).toHaveCount(2, { timeout: 15_000 });
    await expect(alice.locator("[data-board-find-count]")).toHaveText("2 of 4 rows");
    await expect(alice.locator('[data-board-facet-button="label"]')).toContainText("2");

    // `label:` in the find box is the same filter, and Clear clears the lot, hash included.
    await alice.locator("[data-board-find-clear]").click();
    await expect(alice.locator("[data-board-row]")).toHaveCount(4);
    await expect.poll(() => new URL(alice.url()).hash).toBe("");
    await alice.locator("[data-board-find-input]").fill("label:docs @carol");
    await expect(alice.locator("[data-board-row] [data-board-title]")).toHaveText(["Search the board"]);
    await alice.locator("[data-board-row]").click({ position: { x: 5, y: 5 } });
    await expect(alice.locator("[data-board-drawer-tags] [data-board-label]")).toHaveCount(5);
    await shot(alice, `drawer-${name}`);
    // Close the drawer with its own button (as the other board specs do)
    // and wait for it to go: it covers the find bar.
    await alice.locator("[data-board-drawer-close]").click();
    await expect(alice.locator("[data-board-drawer]")).toHaveCount(0);
    await alice.locator("[data-board-find-clear]").click();
  }
});
