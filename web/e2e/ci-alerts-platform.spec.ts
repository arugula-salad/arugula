// CI alerts as native issues (native #774) on the platform page. Alice owns
// team Acme with chat on, and turns CI alerts on in its project's settings:
// the URL and the key show, the key once. The test signs a fake chant
// `ci.red` event with that key as chant's tick would (HMAC-SHA256 over
// `<timestamp>.<body>`, `x-chant-signature: sha256=<hex>`) and posts it to
// the URL: an issue opens in the project, written by chant CI, and the
// settings list it. A signed `ci.green` comments and closes it. The same
// event with a wrong key is refused and changes nothing. A local control
// (team-fixture.ts) with a key to seal with.

import { createHmac, randomBytes } from "node:crypto";
import { expect, test } from "@playwright/test";
import { closeContexts } from "./helpers";
import { TeamControl } from "./team-fixture";

const t = new TeamControl("cialerts");

test.afterAll(async ({ browser }) => {
  await closeContexts({ browser });
  t.stop();
});

const RED = "a1".repeat(20);
const GREEN = "c3".repeat(20);

function event(id: string, kind: string, ci: Record<string, unknown>) {
  return {
    schema: "chant.notify/v1",
    id,
    event: kind,
    at: new Date().toISOString(),
    source: "chant",
    project: "acme/app",
    summary: `main ${kind}`,
    targets: [],
    ci,
    run: { forge: "github", repository: "acme/app", commit: ci.commit },
  };
}

/** Post `e` to `url` signed with `key`, as chant's `webhookRequest` does. */
async function send(url: string, key: string, e: ReturnType<typeof event>) {
  const body = JSON.stringify(e);
  const ts = String(Math.floor(Date.now() / 1000));
  const sig = `sha256=${createHmac("sha256", key).update(`${ts}.${body}`, "utf8").digest("hex")}`;
  const r = await fetch(url, {
    method: "POST",
    headers: { "content-type": "application/json", "x-chant-event": e.event, "x-chant-delivery": e.id, "x-chant-timestamp": ts, "x-chant-signature": sig },
    body,
  });
  return { status: r.status, body: (await r.json().catch(() => ({}))) as Record<string, unknown> };
}

test("a signed red opens an issue by chant CI, and a signed green closes it", async ({ browser }) => {
  test.setTimeout(180_000);
  await t.start(["--platform-host", "app.localhost", "--cloud-secret-key", randomBytes(32).toString("hex")]);
  const app = t.base.replace("//127.0.0.1", "//app.localhost");
  const alice = await t.person(browser, "alice");
  await alice.evaluate(() => window.__arugula.control!.createTeam("Acme"));
  const team = await alice.evaluate(() => window.__arugula.control!.teams[0].team);
  await alice.evaluate((tm) => window.__arugula.control!.setLabsFlag("chat", true, tm), team);
  await alice.goto(`${app}/`);
  await alice.locator("section[data-platform-home] [data-project]", { hasText: "Acme" }).click();
  const project = /\/p\/(prj_[0-9a-f]+)/.exec(alice.url())![1];

  await alice.goto(`${app}/p/${project}/settings`);
  const section = alice.locator("[data-ci-alerts]");
  await expect(section).toHaveAttribute("data-ci-alerts", "off", { timeout: 15_000 });
  await section.locator("[data-ci-alerts-on]").click();
  await expect(section).toHaveAttribute("data-ci-alerts", "on");
  const shownUrl = (await section.locator("[data-ci-alerts-url]").textContent())!;
  const key = (await section.locator("[data-ci-alerts-key]").textContent())!;
  expect(shownUrl).toMatch(new RegExp(`/ci/notify/${project}$`));
  expect(key).toMatch(/^[0-9a-f]{64}$/);
  // Control's own address, whatever its public URL says.
  const url = `${t.base}${new URL(shownUrl).pathname}`;
  // Alice is mentioned on a new red issue.
  await section.locator("[data-ci-alerts-person]").first().check();
  await expect(section.locator("[data-ci-alerts-person]").first()).toBeChecked();

  // A wrong key: refused, nothing opened.
  const red = event("red-1", "ci.red", { branch: "main", commit: RED, phase: "test", failedRun: { name: "test-2", conclusion: "failure" } });
  expect((await send(url, "0".repeat(64), red)).status).toBe(401);

  // Signed: an issue opens, chant CI's.
  const opened = await send(url, key, red);
  expect(opened.status).toBe(200);
  expect(opened.body.did).toBe("opened");
  const issue = opened.body.issue as string;
  // The key isn't shown again once the page is read afresh.
  await alice.reload();
  await expect(section.locator("[data-ci-alerts-key]")).toHaveCount(0, { timeout: 15_000 });
  await expect(section.locator("[data-ci-alerts-key-made]")).toBeVisible();
  await expect(section.locator('[data-ci-alerts-open-branch="main"]')).toContainText("#1");
  await section.locator('[data-ci-alerts-open-branch="main"] a').click();
  await expect(alice).toHaveURL(new RegExp(`/issues/${issue}`), { timeout: 15_000 });
  const view = alice.locator("[data-issue]");
  await expect(view.locator("[data-issue-title]")).toHaveText(`main is red at ${RED.slice(0, 12)}`);
  await expect(view.locator("[data-issue-state]")).toHaveText("Open");
  await expect(view.locator("[data-issue-ci]")).toHaveText("CI");
  await expect(view.locator(".issue-who").first()).toHaveText("chant CI");
  await expect(view.locator("[data-issue-body]")).toContainText("Phase test: test-2 (failure)");

  // Green: a comment by chant CI, and closed.
  const green = await send(url, key, event("green-1", "ci.green", { branch: "main", commit: GREEN, red: RED }));
  expect(green.body.did).toBe("closed");
  await expect(view.locator("[data-issue-state]")).toHaveText("Closed", { timeout: 15_000 });
  const said = view.locator(".thread-msg").last();
  await expect(said).toContainText(`is green again at ${GREEN.slice(0, 12)}`);
  await expect(said.locator(".thread-name")).toHaveText("chant CI");
  await expect(said.locator('[data-via="ci"]')).toHaveText("CI");
  // The same event again does nothing.
  expect((await send(url, key, event("green-1", "ci.green", { branch: "main", commit: GREEN, red: RED }))).body.duplicate).toBe(true);
});
