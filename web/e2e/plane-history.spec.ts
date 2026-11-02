// Settings → Plane shows what Arugula changed in Plane (#441). Alice owns
// team Acme with a stand-in Plane connection (no Plane runs here: the
// connection list, its history and the retry are answered by the test, as
// epic-boards.spec.ts answers the boards). A move to Done that failed for good
// shows as an error with its reason and a Retry, under "ARUGU: recent
// changes", and the section's header says "1 failed". Retry POSTs the
// connection's retry route for that job, and the list follows the answer: the
// error goes, the header with it, and what Arugula did stays below. A local
// control (team-fixture.ts).

import { expect, test, type Page } from "@playwright/test";
import { closeContexts } from "./helpers";
import { TeamControl } from "./team-fixture";

const t = new TeamControl("planehistory");

test.afterAll(async ({ browser }) => {
  await closeContexts({ browser });
  t.stop();
});

const SHOTS = process.env.PLANE_SHOTS;
const JOB = "planedone:fi_1:trk_1:12";
const HOUR = 3_600_000;

const connection = (failed_moves: number) => ({
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
  failed_moves,
  has_webhook_secret: false,
  last_delivery_at: null,
  webhook_url: "https://example.test/hook",
  attribution: "none",
  write_policy: { create: "approve", comment: "approve", edit: "approve", state: "approve", fields: "approve" },
  epics_as_boards: true,
});

async function shot(p: Page, name: string) {
  if (SHOTS) await p.screenshot({ path: `${SHOTS}/${name}.png`, animations: "disabled", fullPage: true });
}

test("a failed move shows with its error, Retry queues it again and the list follows", async ({ browser }) => {
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

  const now = Date.now();
  const moved = { at: now - 2 * HOUR, kind: "moved", line: "moved ARUGU-7 to Done: merged acme/app#41", by: "Alice", error: null, job: null };
  const sent = { at: now - 5 * HOUR, kind: "sent", line: "Comment ARUGU-3, approved and sent", by: "Alice", error: null, job: null };
  const failed = {
    at: now - 60_000,
    kind: "move_failed",
    line: "couldn't move ARUGU-12 to Done: merged acme/app#42",
    by: null,
    error: "Plane refused the key",
    job: JOB,
  };
  let entries = [failed, moved, sent];
  const retries: string[] = [];
  await alice.route(new RegExp(`/api/projects/${project}/trackers$`), (route) => route.fulfill({ json: { project, connections: [connection(entries.includes(failed) ? 1 : 0)] } }));
  await alice.route(new RegExp(`/api/projects/${project}/trackers/trk_1/history$`), (route) => route.fulfill({ json: { entries } }));
  await alice.route(new RegExp(`/api/projects/${project}/trackers/trk_1/moves/.+/retry$`), (route) => {
    retries.push(`${route.request().method()} ${new URL(route.request().url()).pathname}`);
    entries = [moved, sent];
    return route.fulfill({ json: { entries } });
  });

  for (const [w, h] of [[1280, 900], [390, 844]] as const) {
    await alice.setViewportSize({ width: w, height: h });
    await alice.emulateMedia({ colorScheme: "dark" });
    entries = [failed, moved, sent];
    await alice.goto(`${app}/p/${project}/settings`);
    const block = alice.locator('[data-plane-history="trk_1"]');
    await expect(block.getByRole("heading", { name: "ARUGU: recent changes" })).toBeVisible({ timeout: 15_000 });
    await expect(alice.locator("[data-plane-failed]")).toContainText("1 failed");
    const bad = block.locator(`[data-plane-failed-move="${JOB}"]`);
    await expect(bad).toContainText("couldn't move ARUGU-12 to Done: merged acme/app#42");
    await expect(bad).toContainText("Plane refused the key");
    await expect(block.locator("[data-plane-event]")).toHaveText(["moved ARUGU-7 to Done: merged acme/app#41 · 2h ago by Alice", "Comment ARUGU-3, approved and sent · 5h ago by Alice"]);
    await block.scrollIntoViewIfNeeded();
    await shot(alice, `failed-${w}-dark`);

    await bad.getByRole("button", { name: "Retry" }).click();
    await expect(block.locator("[data-plane-failed-move]")).toHaveCount(0);
    await expect(alice.locator("[data-plane-failed]")).toHaveCount(0);
    await expect(block.locator("[data-plane-event]")).toHaveCount(2);
    await shot(alice, `retried-${w}-dark`);
  }
  expect(retries).toEqual([`POST /api/projects/${project}/trackers/trk_1/moves/${JOB}/retry`, `POST /api/projects/${project}/trackers/trk_1/moves/${JOB}/retry`]);

  // Nothing to show.
  entries = [];
  await alice.goto(`${app}/p/${project}/settings`);
  await expect(alice.locator('[data-plane-history="trk_1"] [data-plane-history-none]')).toHaveText("Nothing yet", { timeout: 15_000 });
});
