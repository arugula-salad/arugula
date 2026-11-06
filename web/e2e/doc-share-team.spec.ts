// Share with a team (Arugula #418), in a machine's share dialog. Alice's
// machine geek copies a Doc up to her personal project; her team Acme has a
// project. Opened from geek's own page (served by the daemon, with no way to
// call the platform as her) the dialog's section is one line pointing at the
// copy on the platform. With the platform reachable as her (the page's
// calls to /api/projects carried to control with her session, as a page
// served through control makes them) it lists Acme: Share makes a team Doc
// from the copy, and Update copy stays off until the Doc changes. A local
// control (team-fixture.ts), as docs-platform.spec.ts runs it.

import { expect, test, type Page } from "@playwright/test";
import { closeContexts } from "./helpers";
import { localToken } from "./local-token";
import { daemonPort } from "./ports";
import { TeamControl } from "./team-fixture";

const t = new TeamControl("docshareteam");

test.afterAll(async ({ browser }) => {
  await closeContexts({ browser });
  t.stop();
});

/** A picture of the page for the PR, when E2E_SHOTS names a folder. */
const shot = async (p: Page, name: string) => {
  if (process.env.E2E_SHOTS) await p.screenshot({ path: `${process.env.E2E_SHOTS}/${name}` });
};

test("a machine's share dialog shares its Doc's copy with a team, or says where to", async ({ browser }) => {
  test.setTimeout(180_000);
  await t.start(["--platform-host", "app.localhost"]);
  const app = t.base.replace("//127.0.0.1", "//app.localhost");
  const alice = await t.person(browser, "alice");
  await alice.evaluate(() => window.__arugula.control!.createTeam("Acme"));
  await alice.evaluate(() => window.__arugula.control!.setLabsFlag("doc-copies", true));
  const state = await t.addMachine(alice, "geek");
  const base = `http://127.0.0.1:${await daemonPort(state)}`;
  const auth = { "content-type": "application/json", authorization: `Bearer ${localToken}` };
  const post = async (path: string, body: unknown) => {
    const r = await fetch(base + path, { method: "POST", headers: auth, body: JSON.stringify(body) });
    const text = await r.text();
    expect(r.ok, `${path}: ${text}`).toBe(true);
    return JSON.parse(text) as { block?: number };
  };
  const [{ id: term }] = (await (await fetch(`${base}/api/panes`, { headers: auth })).json()) as { id: number }[];
  const made = await post("/api/blocks", { type: "doc", config: { title: "Geek's plan" }, split: term, local: true });
  const doc = made.block!;
  await post(`/api/blocks/${doc}/call/doc_outline`, { title: "Geek's plan", sections: [{ id: "s1", heading: "Goals", intent: "what ships" }] });
  await post(`/api/blocks/${doc}/call/doc_fill`, { section: "s1", markdown: "From **geek**." });
  // The copy is put a moment after the fill; the daemon says where it is.
  await expect
    .poll(async () => ((await (await fetch(`${base}/api/docs`, { headers: auth })).json()) as { title: string; platform?: unknown }[]).find((d) => d.title === "Geek's plan")?.platform, {
      timeout: 60_000,
      intervals: [1000],
    })
    .toBeTruthy();

  // geek's own page, with the Doc beside its terminal.
  const shell = await browser.newPage();
  await shell.goto(base);
  await expect.poll(() => shell.evaluate(() => window.__arugula?.client.connected)).toBe(true);
  const dialog = shell.locator("[data-doc-link-dialog]");
  const open = async () => {
    await shell.locator(`[data-doc-bar="${doc}"] [data-doc-bar-share]`).click();
    await expect(dialog).toBeVisible();
  };
  await expect(shell.locator(`[data-doc-bar="${doc}"]`)).toBeVisible({ timeout: 30_000 });

  // No way to call the platform as her from here: one line, to the copy.
  await open();
  const off = dialog.locator("[data-doc-share-team-off]");
  await expect(off).toContainText("Open it on the platform to share it with a team", { timeout: 15_000 });
  await expect(off.locator("a")).toHaveAttribute("href", /\/p\/prj_[0-9a-f]+[^/]*\/docs\/pdc_[0-9a-f]{32}$/);
  await shot(shell, "machine-dialog-local.png");
  await shell.keyboard.press("Escape");
  await expect(dialog).toHaveCount(0);

  // As a page served through control would call it: carried to control with
  // Alice's session.
  await alice.goto(`${app}/`);
  await shell.route(/\/api\/projects(\/|\?|$)/, async (route) => {
    const q = route.request();
    const u = new URL(q.url());
    const r = await alice.request.fetch(`${app}${u.pathname}${u.search}`, { method: q.method(), data: q.postData() ?? undefined, headers: { "content-type": "application/json", origin: new URL(app).origin } });
    await route.fulfill({ response: r });
  });
  await open();
  const section = dialog.locator("[data-doc-share-team]");
  const acme = section.locator("[data-share-with-project]", { hasText: "Acme" });
  await expect(acme.locator("[data-share-with-go]")).toBeVisible({ timeout: 15_000 });
  await shot(shell, "machine-dialog-section.png");
  await acme.locator("[data-share-with-go]").click();
  await expect(acme.locator("[data-share-with-made]")).toContainText("Made", { timeout: 15_000 });
  await expect(acme.locator("[data-share-with-update]")).toBeDisabled();
  await shot(shell, "machine-dialog-shared.png");

  // The Doc on geek changes, and its copy follows: Update copy turns on, and
  // brings the team's Doc up to date (nobody edited it, so no question).
  await post(`/api/blocks/${doc}/call/doc_fill`, { section: "s1", markdown: "Changed on **geek**." });
  await expect(async () => {
    await shell.keyboard.press("Escape");
    await open();
    await expect(acme.locator("[data-share-with-update]")).toBeEnabled({ timeout: 3_000 });
  }).toPass({ timeout: 60_000, intervals: [2000] });
  await acme.locator("[data-share-with-update]").click();
  await expect(acme.locator("[data-share-with-update]")).toBeDisabled({ timeout: 15_000 });
  const href = (await acme.locator("[data-share-with-open]").getAttribute("href"))!;
  const m = /\/p\/(prj_[0-9a-f]+)[^/]*\/docs\/(pdc_[0-9a-f]{32})/.exec(href)!;
  const team = await alice.evaluate(async ([p, id]) => (await (await fetch(`/api/projects/${p}/docs/${id}`)).json()) as { sections: { body: string }[]; writer: boolean }, [m[1], m[2]]);
  expect(team.writer).toBe(true);
  expect(team.sections[0].body).toBe("Changed on **geek**.");
});
