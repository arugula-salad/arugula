// Plane epics as boards, in the UI (#484). Alice owns team Acme with boards
// on and three boards she made: Launch and Later stand for epics (the
// responses are given a `source`, as the platform's sync gives one, since no
// Plane runs here) and Notes is her own. The Boards page lists Notes, then a
// "From Plane" group with the started epic's board first and the other
// marked "Not started in Plane". The board's header says "From Plane · ARUGU-10"
// as a link, and how many done children were left out. Settings → Plane has
// an "Epics become boards" switch on the connection, which PATCHes
// `epics_as_boards`, and counts the boards that follow it. Removing a synced
// row says the platform's refusal. A local control (team-fixture.ts).

import { expect, test, type Page } from "@playwright/test";
import { closeContexts } from "./helpers";
import { TeamControl } from "./team-fixture";

const t = new TeamControl("epicboards");

test.afterAll(async ({ browser }) => {
  await closeContexts({ browser });
  t.stop();
});

const SHOTS = process.env.EPIC_SHOTS;
const REFUSAL = "ARUGU-11 is a child of ARUGU-10 in Plane: change its parent there";

const source = (key: string, started: boolean, left_out = 0) => ({ provider: "plane", key, url: `https://app.plane.so/acme/browse/${key}/`, connection: "Acme", left_out, started });

const connection = (epics_as_boards: boolean) => ({
  id: "trk_1",
  provider: "plane",
  workspace_slug: "acme",
  source_project_id: "p1",
  identifier: "ARUGU",
  name: "Acme",
  backfilled_at: 1,
  items: 12,
  purge_after: null,
  waiting_writes: 0,
  has_webhook_secret: false,
  last_delivery_at: null,
  webhook_url: "https://example.test/hook",
  attribution: "none",
  write_policy: { create: "approve", comment: "approve", edit: "approve", state: "approve", fields: "approve" },
  epics_as_boards,
});

async function shot(p: Page, name: string) {
  if (SHOTS) await p.screenshot({ path: `${SHOTS}/${name}.png`, animations: "disabled", fullPage: true });
}

test("epic boards: the header, the Boards page group, the Settings switch and a refused removal", async ({ browser }) => {
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

  const api = (method: string, path: string, body?: unknown) =>
    alice.evaluate(
      async ([m, u, b]) => {
        const r = await fetch(u as string, { method: m as string, credentials: "same-origin", headers: { "Content-Type": "application/json" }, body: b === undefined ? undefined : JSON.stringify(b) });
        return { status: r.status, body: (await r.json().catch(() => null)) as Record<string, any> | null };
      },
      [method, path, body] as const,
    );
  const ids: Record<string, string> = {};
  for (const title of ["Notes", "Launch", "Later"]) ids[title] = (await api("POST", `/api/projects/${project}/boards`, { title })).body!.board.id;
  const made = await api("PUT", `/api/boards/${ids.Later}/entries`, { id: null, rev: null, entry: "row", zone: null, title: null, kind: null, pointer: null, note: "ARUGU-11", status: null });
  expect(made.status).toBe(200);
  const row = made.body!.zones.flatMap((z: any) => z.rows)[0].id as string;

  // What the sync would have given the two epics' boards.
  const sources: Record<string, ReturnType<typeof source>> = { [ids.Launch]: source("ARUGU-10", true, 3), [ids.Later]: source("ARUGU-20", false) };
  await alice.route(new RegExp(`/api/projects/${project}/boards$`), async (route) => {
    const res = await route.fetch();
    const v = await res.json();
    for (const b of v.boards) if (sources[b.id]) b.source = sources[b.id];
    await route.fulfill({ response: res, json: v });
  });
  await alice.route(new RegExp(`/api/boards/brd_[0-9a-f]+$`), async (route) => {
    if (route.request().method() !== "GET") return route.fallback();
    const res = await route.fetch();
    const v = await res.json();
    if (sources[v.board.id]) v.board.source = sources[v.board.id];
    await route.fulfill({ response: res, json: v });
  });
  await alice.route(new RegExp(`/api/boards/${ids.Later}/entries/${row}$`), (route) => route.fulfill({ status: 409, json: { error: REFUSAL } }));

  // The Boards page: Notes, then From Plane with the started epic first.
  for (const [w, h, scheme] of [[1280, 900, "light"], [1280, 900, "dark"], [390, 844, "light"], [390, 844, "dark"]] as const) {
    await alice.setViewportSize({ width: w, height: h });
    await alice.emulateMedia({ colorScheme: scheme });
    await alice.goto(`${app}/p/${project}/boards`);
    const group = alice.locator("[data-boards-plane]");
    await expect(group).toBeVisible({ timeout: 15_000 });
    await expect(group.getByRole("heading", { name: "From Plane" })).toBeVisible();
    await expect(group.locator("[data-board-link]")).toHaveText(["Launch", "Later"]);
    await expect(group.locator("[data-board-not-started]")).toHaveCount(1);
    await expect(alice.locator("[data-boards]  [data-board-link]")).toHaveText(["Notes"]);
    await shot(alice, `boards-${w}-${scheme}`);

    // The header: a link to the epic, and the done children left out.
    await group.locator(`[data-board-link="${ids.Launch}"]`).click();
    const src = alice.locator("[data-board-source]");
    await expect(src).toContainText("From Plane · ARUGU-10", { timeout: 15_000 });
    await expect(src.getByRole("link", { name: "From Plane · ARUGU-10" })).toHaveAttribute("href", "https://app.plane.so/acme/browse/ARUGU-10/");
    await expect(src.getByRole("link")).toHaveAttribute("target", "_blank");
    await expect(src.locator("[data-board-left-out]")).toHaveText(/3 done children left out/);
    await shot(alice, `header-${w}-${scheme}`);
  }

  // A person's board has no such line.
  await alice.goto(`${app}/p/${project}/boards/${ids.Notes}`);
  await expect(alice.locator("[data-board-title]")).toBeVisible({ timeout: 15_000 });
  await expect(alice.locator("[data-board-source]")).toHaveCount(0);

  // Removing a synced row shows the platform's refusal.
  await alice.goto(`${app}/p/${project}/boards/${ids.Later}`);
  await alice.locator("[data-board-row-menu]").click();
  await alice.getByRole("menuitem", { name: "Delete the row" }).click();
  await expect(alice.locator("[data-platform-toast]")).toContainText(REFUSAL);

  // Settings → Plane: the switch, PATCHing the connection, and the count.
  let on = true;
  const patches: unknown[] = [];
  await alice.route(new RegExp(`/api/projects/${project}/trackers$`), (route) => route.fulfill({ json: { connections: [connection(on)] } }));
  await alice.route(new RegExp(`/api/projects/${project}/trackers/trk_1$`), (route) => {
    const body = route.request().postDataJSON();
    patches.push(body);
    on = body.epics_as_boards;
    return route.fulfill({ json: connection(on) });
  });
  for (const [w, h, scheme] of [[1280, 900, "light"], [1280, 900, "dark"], [390, 844, "light"], [390, 844, "dark"]] as const) {
    await alice.setViewportSize({ width: w, height: h });
    await alice.emulateMedia({ colorScheme: scheme });
    await alice.goto(`${app}/p/${project}/settings`);
    const cell = alice.locator('[data-plane-epics="trk_1"]');
    await expect(cell).toContainText("Epics become boards", { timeout: 15_000 });
    await expect(cell.locator("[data-plane-epics-switch]")).toBeChecked();
    await expect(cell.locator("[data-plane-epics-count]")).toHaveText("Following 2 epics");
    await cell.scrollIntoViewIfNeeded();
    await shot(alice, `settings-${w}-${scheme}`);
  }
  await alice.locator("[data-plane-epics-switch]").click();
  await expect.poll(() => patches).toEqual([{ epics_as_boards: false }]);
  await expect(alice.locator("[data-plane-epics-switch]")).not.toBeChecked();
});
