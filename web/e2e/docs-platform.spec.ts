// Project Docs on the platform page (aru-118): a project's Docs, one Doc
// drawn by the Doc block's renderer (a chart fence draws a chart), and who
// may write. Alice owns team Acme, with chat's Labs flag on (native Docs
// come with it); Bob is a viewer. Alice makes a Doc from the project's
// Docs and writes a section; Bob reads it on a phone with no form, sees
// Alice's next section arrive without a reload, and finds it on the
// project's home. Then (Arugula #678) Alice's machine joins and copies a
// Doc of its own up: the copy is read-only on the page, which says whose
// it is, and a write to it is refused naming the machine, while the
// machine's own changes still arrive live. A local control
// (team-fixture.ts, `--platform-host app.localhost`), as platform.spec.ts
// runs it.

import { expect, test, type Page } from "@playwright/test";
import { attacker, prototype, prototypeRan } from "./doc-steps";
import { closeContexts } from "./helpers";
import { localToken } from "./local-token";
import { daemonPort } from "./ports";
import { TeamControl, home } from "./team-fixture";

const t = new TeamControl("docs");

test.describe.configure({ mode: "serial" });
test.afterAll(async ({ browser }) => {
  await closeContexts({ browser });
  t.stop();
});

let alice: Page;
let bob: Page;
let app = "";
let docPath = "";
/** Acme's team id, for the machine that joins it. */
let team = "";
/** The team Doc Alice made from /docs, for the compatibility check. */
let roadmapPath = "";

async function platform(p: Page, path = "/") {
  await p.goto(`${app}${path}`);
  await expect(p.locator("[data-platform-bar], [data-chat]").first()).toBeVisible({ timeout: 15_000 });
}

const CHART = '```chart\n{"type": "bar", "x": "week", "y": "shipped", "rows": [{"week": "w1", "shipped": 3}, {"week": "w2", "shipped": 5}]}\n```';

test("an owner makes a Doc from the project's Docs and writes it", async ({ browser }) => {
  test.setTimeout(180_000);
  // A block domain too, for a Doc's prototype (#506): the platform page's
  // CSP has `script-src 'self'`, so its frame is control's proto origin.
  await t.start(["--platform-host", "app.localhost", "--block-domain", "blocks.localhost"]);
  app = t.base.replace("//127.0.0.1", "//app.localhost");
  alice = await t.person(browser, "alice");
  await alice.evaluate(() => window.__arugula.control!.createTeam("Acme"));
  team = await alice.evaluate(() => window.__arugula.control!.teams[0].team);
  await alice.evaluate((tm) => window.__arugula.control!.setLabsFlag("chat", true, tm), team);
  const link = await alice.evaluate((tm) => window.__arugula.control!.invite(tm, "viewer", true), team);
  bob = await t.person(browser, "bob", { viewport: { width: 390, height: 844 }, isMobile: true, hasTouch: true });
  await bob.goto(link);
  await bob.locator("[data-accept-invite]").click();
  await alice.evaluate(() => window.__arugula.control!.refresh());
  await expect(alice.locator("[data-admit-yes]")).toBeVisible({ timeout: 15_000 });
  await alice.locator("[data-admit-yes]").click();
  await expect.poll(() => alice.evaluate(() => window.__arugula.control!.teams[0].roster.members.length), { timeout: 15_000 }).toBe(2);

  await platform(alice);
  await alice.locator("section[data-platform-home] [data-project]", { hasText: "Acme" }).click();
  await expect(alice.locator("[data-project-home]")).toContainText("No Docs yet", { timeout: 15_000 });
  await alice.locator("[data-project-docs-link]").click();
  // Docs is one of the project's places (Arugula #635).
  await expect(alice.locator("[data-platform-bar] [data-platform-docs]")).toHaveAttribute("aria-current", "page");
  await expect(alice).toHaveURL(/\/p\/prj_[0-9a-f]+-acme\/docs$/);
  await expect(alice.locator("[data-docs-none]")).toBeVisible();

  await alice.locator("[data-doc-new-open]").click();
  await alice.locator("[data-doc-new-title]").fill("Release plan");
  await alice.locator("[data-doc-new]").click();
  await expect(alice).toHaveURL(/\/p\/prj_[0-9a-f]+-acme\/docs\/pdc_[0-9a-f]{32}-release-plan$/, { timeout: 15_000 });
  docPath = new URL(alice.url()).pathname;
  const page = alice.locator("[data-doc-page]");
  await expect(page.locator(".doc-title")).toHaveText("Release plan");
  await expect(page).toHaveAttribute("data-writer", "true");
  await expect(page.locator("[data-doc-maker]")).toContainText("made by you");

  // A first section, with a chart.
  await page.locator("[data-doc-heading-input]").fill("Goals");
  await page.locator("[data-doc-markdown-input]").fill(`Ship **0.64** this week.\n\n${CHART}`);
  await page.locator("[data-doc-save]").click();
  const goals = page.locator(".doc-section[data-section=s1]");
  await expect(goals.locator(".doc-heading")).toHaveText("Goals");
  await expect(goals.locator(".doc-body strong")).toHaveText("0.64");
  await expect(goals.locator(".chart-box svg")).toBeVisible();
  await expect(page.locator("[data-doc-progress]")).toHaveText("1 of 1 sections written");
});

test("a viewer reads it on a phone, with no form, and has the next section live", async () => {
  test.setTimeout(90_000);
  await home(bob);
  // Loading a Doc by its address doesn't take the slug off the address and
  // put it back: its title isn't known yet, and that's no reason to change it.
  await bob.addInitScript(() => {
    const seen: string[] = [];
    (window as unknown as { addresses: string[] }).addresses = seen;
    const replace = history.replaceState.bind(history);
    history.replaceState = (state, unused, url) => {
      seen.push(String(url));
      replace(state, unused, url);
    };
  });
  await platform(bob, docPath);
  const page = bob.locator("[data-doc-page]");
  await expect(page.locator(".doc-title")).toHaveText("Release plan", { timeout: 15_000 });
  const addresses = await bob.evaluate(() => (window as unknown as { addresses: string[] }).addresses);
  expect(addresses.filter((a) => !a.endsWith("-release-plan"))).toEqual([]);
  await expect(page.locator(".doc-section[data-section=s1] .chart-box svg")).toBeVisible();
  await expect(page).toHaveAttribute("data-writer", "false");
  await expect(page.locator("[data-doc-writer]")).toHaveCount(0);
  // The phone layout: nothing wider than the screen.
  const wide = await bob.evaluate(() => {
    const main = document.querySelector(".platform-main")!;
    return main.scrollWidth - main.clientWidth;
  });
  expect(wide).toBeLessThanOrEqual(0);

  // Alice adds a section; Bob's page has it without a reload.
  const mine = alice.locator("[data-doc-page]");
  await mine.locator("[data-doc-heading-input]").fill("Risks");
  await mine.locator("[data-doc-markdown-input]").fill("- the canary\n- the weekend");
  await mine.locator("[data-doc-save]").click();
  await expect(mine.locator(".doc-section[data-section=s2] li")).toHaveCount(2);
  await expect(page.locator(".doc-section[data-section=s2] .doc-heading")).toHaveText("Risks", { timeout: 15_000 });
  await expect(page.locator(".doc-section[data-section=s2] li")).toHaveText(["the canary", "the weekend"]);

  // The project's home lists it, and its Docs page has no form for him.
  await platform(bob, docPath.replace(/\/docs\/.*$/, ""));
  await expect(bob.locator("[data-project-docs] [data-doc-link]")).toHaveText(["Release plan"], { timeout: 15_000 });
  await platform(bob, docPath.replace(/\/docs\/.*$/, "/docs"));
  await expect(bob.locator("[data-docs-page] [data-doc-title]")).toHaveText(["Release plan"], { timeout: 15_000 });
  await expect(bob.locator("[data-doc-new-open]")).toHaveCount(0);
});

// #506: a prototype on the platform page runs in a frame on the proto
// origin the hub named in its answers, and gets out of nowhere. Alice writes
// it, Bob reads it on a phone.
test("a prototype runs in a frame on the platform page and gets out of nowhere", async () => {
  test.setTimeout(90_000);
  const a = await attacker();
  try {
    const mine = alice.locator("[data-doc-page]");
    await mine.locator("[data-doc-heading-input]").fill("Prototype");
    await mine.locator("[data-doc-markdown-input]").fill(prototype(a));
    await mine.locator("[data-doc-save]").click();
    const scheme = async (p: Page) => ((await p.evaluate(() => document.documentElement.dataset.scheme)) === "light" ? "light" : "dark");
    await prototypeRan(alice, a, await scheme(alice));
    await expect(alice.locator("[data-prototype-frame]")).toHaveAttribute("src", /^http:\/\/proto\.blocks\.localhost:\d+\/$/);
    await expect(alice.locator("[data-prototype-note]")).toHaveCount(0);

    await platform(bob, docPath);
    await expect(bob.locator("[data-doc-page] .doc-title")).toHaveText("Release plan", { timeout: 15_000 });
    await prototypeRan(bob, a, await scheme(bob));
    expect(a.hits).toEqual([]);
  } finally {
    a.close();
  }
});

test("an owner deletes it", async () => {
  test.setTimeout(60_000);
  const page = alice.locator("[data-doc-page]");
  await page.locator("[data-doc-delete]").click();
  await page.locator("[data-doc-confirm-delete]").click();
  await expect(alice).toHaveURL(/\/docs$/, { timeout: 15_000 });
  await expect(alice.locator("[data-docs-none]")).toBeVisible({ timeout: 15_000 });
});

// #418: /docs lists every Doc the reader can read, their own private ones
// first; the preview draws the whole Doc. Alice makes a team Doc and a
// private one from the list; Bob sees the team's and never hers.
test("/docs puts a person's private Docs first, and keeps them from the team", async () => {
  test.setTimeout(120_000);
  await platform(alice, "/docs");
  const make = async (title: string, where: string) => {
    await alice.locator("[data-doc-new-open]").click();
    await alice.locator("[data-doc-new-title]").fill(title);
    await alice.locator("[data-doc-new-project]").selectOption({ label: where });
    await alice.locator("[data-doc-new]").click();
    const page = alice.locator("[data-doc-page]");
    await expect(page.locator(".doc-title")).toHaveText(title, { timeout: 15_000 });
    await page.locator("[data-doc-heading-input]").fill("Notes");
    await page.locator("[data-doc-markdown-input]").fill(`${title} says **hello**.\n\n${CHART}`);
    await page.locator("[data-doc-save]").click();
    await expect(page.locator(".doc-section[data-section=s1] .chart-box svg")).toBeVisible();
    await platform(alice, "/docs");
  };
  await make("Team roadmap", "Acme");
  await make("Private notes", "Yours, private");

  const groups = alice.locator("[data-list-rows] [role=group] .list-group");
  await expect(groups).toHaveText([/^Yours, private\s*1$/, /^Acme\s*1$/], { timeout: 15_000 });
  await expect(alice.locator("[data-list-rows] [data-doc]")).toHaveText([/Private notes/, /Team roadmap/]);
  await expect(alice.locator("[data-list-count=private] b")).toHaveText("1");
  await expect(alice.locator("[data-list-count=shared] b")).toHaveText("1");
  // The first row is selected, and its whole Doc is on the right.
  const preview = alice.locator("[data-list-preview]");
  await expect(preview.locator(".doc-section[data-section=s1] .chart-box svg")).toBeVisible({ timeout: 15_000 });
  await alice.locator("[data-list-count=shared]").click();
  await expect(alice.locator("[data-list-rows] [data-doc]")).toHaveText([/Team roadmap/]);
  await preview.locator("[data-open-doc]").click();
  await expect(alice).toHaveURL(/\/p\/prj_[0-9a-f]+-acme\/docs\/pdc_[0-9a-f]{32}-team-roadmap$/);
  roadmapPath = new URL(alice.url()).pathname;

  // Bob reads the team's, and has no trace of hers.
  await platform(bob, "/docs");
  await expect(bob.locator("[data-list-rows] [data-doc]")).toHaveText([/Team roadmap/], { timeout: 15_000 });
  await expect(bob.getByText("Private notes")).toHaveCount(0);
  await expect(bob.locator("[data-list-rows] .list-group")).toHaveText([/^Acme\s*1$/]);
  const wide = await bob.evaluate(() => {
    const main = document.querySelector(".platform-main")!;
    return main.scrollWidth - main.clientWidth;
  });
  expect(wide).toBeLessThanOrEqual(0);
});

// Arugula #678: the machine's library Doc is the truth, and its copy on the
// platform is read-only. Alice's machine geek joins her account alone (a
// team's machine doesn't copy its Docs) and she turns Your Docs on the
// platform on, her own word for copies (native #673); geek makes a Doc, its
// copy lands in her private Docs, where the row, the preview
// and the page say it's a copy from geek, the page has no form, a write
// over the route is refused naming geek, and geek's next change arrives
// on the open page.
test("a machine's copy is read-only, and the page says whose it is", async () => {
  test.setTimeout(180_000);
  await alice.goto(t.base);
  await expect.poll(() => alice.evaluate(() => !!window.__arugula?.control), { timeout: 15_000 }).toBe(true);
  await alice.evaluate(() => window.__arugula.control!.setLabsFlag("doc-copies", true));
  const state = await t.addMachine(alice, "geek");
  const base = `http://127.0.0.1:${await daemonPort(state)}`;
  const post = async (path: string, body: unknown) => {
    const r = await fetch(base + path, {
      method: "POST",
      headers: { "content-type": "application/json", authorization: `Bearer ${localToken}` },
      body: JSON.stringify(body),
    });
    const text = await r.text();
    expect(r.ok, `${path}: ${text}`).toBe(true);
    return JSON.parse(text) as { block?: number };
  };
  const made = await post("/api/blocks", { type: "doc", config: { title: "Geek's plan" }, local: true });
  await post(`/api/blocks/${made.block}/call/doc_outline`, { title: "Geek's plan", sections: [{ id: "s1", heading: "Goals", intent: "what ships" }] });
  await post(`/api/blocks/${made.block}/call/doc_fill`, { section: "s1", markdown: "From **geek**." });

  // The copy is put a moment after the fill; the list is read on load.
  const row = () => alice.locator("[data-list-rows] [data-doc]", { hasText: "Geek's plan" });
  await platform(alice, "/docs");
  await expect
    .poll(
      async () => {
        await alice.reload();
        await expect(alice.locator("[data-docs-page]")).toBeVisible();
        await expect(alice.locator("[data-docs-loading]")).toHaveCount(0, { timeout: 15_000 });
        return row().count();
      },
      { timeout: 60_000, intervals: [2000] },
    )
    .toBe(1);
  await expect(row()).toHaveAttribute("data-private", "true");
  await row().click();
  const preview = alice.locator("[data-list-preview]");
  await expect(preview.locator("[data-doc-copy-fact]")).toHaveText("a copy from geek");
  await preview.locator("[data-open-doc]").click();

  const page = alice.locator("[data-doc-page]");
  await expect(page.locator(".doc-title")).toHaveText("Geek's plan", { timeout: 15_000 });
  await expect(page).toHaveAttribute("data-writer", "false");
  await expect(page.locator("[data-doc-writer]")).toHaveCount(0);
  await expect(page.locator("[data-doc-copy]")).toContainText("A copy of a Doc in geek's library");
  await expect(page.locator("[data-doc-copy]")).toContainText("read-only");
  await expect(page.locator("[data-doc-maker]")).toHaveText("a copy from geek");
  await expect(page.locator(".doc-section[data-section=s1] strong")).toHaveText("geek");

  // Over the route, as an agent or a script would: refused, naming geek.
  const m = /\/p\/(prj_[0-9a-f]+)[^/]*\/docs\/(pdc_[0-9a-f]{32})/.exec(new URL(alice.url()).pathname)!;
  const refused = await alice.evaluate(async ([project, id]) => {
    const r = await fetch(`/api/projects/${project}/docs/${id}`, {
      method: "PUT",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ section: "s1", markdown: "edited on the web" }),
    });
    return { status: r.status, body: await r.text() };
  }, [m[1], m[2]]);
  expect(refused.status).toBe(403);
  expect(refused.body).toContain("copy");
  expect(refused.body).toContain("geek");

  // The machine's own change still lands, and the open page has it.
  await post(`/api/blocks/${made.block}/call/doc_fill`, { section: "s1", markdown: "Changed on **geek** again." });
  await expect(page.locator(".doc-section[data-section=s1] .doc-body")).toContainText("again", { timeout: 30_000 });
  await expect(page.locator(".doc-section[data-section=s1] strong")).toHaveText("geek");
  await expect(page).toHaveAttribute("data-writer", "false");

  // The copy is the project's to delete (native #673), and the page says
  // geek's next change brings a new one, which it does, under a new id.
  await page.locator("[data-doc-delete]").click();
  await expect(page.locator("[data-doc-confirm]")).toContainText("geek is your machine");
  await page.locator("[data-doc-confirm-delete]").click();
  await expect(alice).toHaveURL(/\/docs$/, { timeout: 15_000 });
  const copies = async () => (await alice.evaluate(async (p) => (await fetch(`/api/projects/${p}/docs`)).json(), m[1])).docs.filter((d: { title: string }) => d.title === "Geek's plan") as { id: string }[];
  expect(await copies()).toHaveLength(0);
  await post(`/api/blocks/${made.block}/call/doc_fill`, { section: "s1", markdown: "Back after the delete." });
  await expect.poll(async () => (await copies()).length, { timeout: 60_000, intervals: [2000] }).toBe(1);
  expect((await copies())[0].id).not.toBe(m[2]);

  // Deleted on geek, the copy goes with it.
  const rows = (await (await fetch(`${base}/api/docs`, { headers: { authorization: `Bearer ${localToken}` } })).json()) as { id: string; title: string }[];
  const lib = rows.find((r) => r.title === "Geek's plan")!.id;
  const gone = await fetch(`${base}/api/docs/${lib}`, { method: "DELETE", headers: { authorization: `Bearer ${localToken}` } });
  expect(gone.ok, await gone.text()).toBe(true);
  await expect.poll(async () => (await copies()).length, { timeout: 60_000, intervals: [2000] }).toBe(0);
});

// Control serves its newest page to older platforms and daemons: a Doc
// view from before #678 has no `machine_name` (and one from before #418 no
// `machine`). The page fills them in where it loads the view, so a Doc
// made here keeps its form, and a copy stays read-only by the view's own
// `writer`.
test("a page newer than the platform still writes a Doc made here", async () => {
  test.setTimeout(60_000);
  const strip = async (route: import("@playwright/test").Route) => {
    const res = await route.fetch();
    const v = (await res.json()) as { doc: Record<string, unknown> };
    const { machine: _machine, machine_name: _name, ...doc } = v.doc;
    await route.fulfill({ response: res, json: { ...v, doc } });
  };
  await alice.route("**/api/projects/*/docs/pdc_*", strip);
  try {
    await platform(alice, roadmapPath);
    const page = alice.locator("[data-doc-page]");
    await expect(page.locator(".doc-title")).toHaveText("Team roadmap", { timeout: 15_000 });
    await expect(page).toHaveAttribute("data-writer", "true");
    await expect(page.locator("[data-doc-writer]")).toBeVisible();
    await expect(page.locator("[data-doc-copy]")).toHaveCount(0);
    await expect(page.locator("[data-doc-maker]")).toHaveText("made by you");
  } finally {
    // Waits for a `strip` still running: this is the spec's last test, and its
    // context closes next, which would dispose the response it's reading.
    await alice.unrouteAll({ behavior: "wait" });
  }
});

// Native #673: someone in no team has chat off, and still has their Docs on
// the platform once they turn the word on: Carol's machine copies a Doc up,
// /docs lists it, and its page opens it (its pictures: the platform's test).
test("someone in no team sees their machine's copies on /docs", async ({ browser }) => {
  test.setTimeout(180_000);
  const carol = await t.person(browser, "carol");
  await expect.poll(() => carol.evaluate(() => !!window.__arugula?.control), { timeout: 15_000 }).toBe(true);
  await carol.evaluate(() => window.__arugula.control!.setLabsFlag("doc-copies", true));
  const state = await t.addMachine(carol, "carolbox");
  const base = `http://127.0.0.1:${await daemonPort(state)}`;
  const post = async (path: string, body: unknown) => {
    const r = await fetch(base + path, {
      method: "POST",
      headers: { "content-type": "application/json", authorization: `Bearer ${localToken}` },
      body: JSON.stringify(body),
    });
    const text = await r.text();
    expect(r.ok, `${path}: ${text}`).toBe(true);
    return JSON.parse(text) as { block?: number };
  };
  const made = await post("/api/blocks", { type: "doc", config: { title: "Carol's notes" }, local: true });
  await post(`/api/blocks/${made.block}/call/doc_outline`, { title: "Carol's notes", sections: [{ id: "s1", heading: "One", intent: "" }] });
  await post(`/api/blocks/${made.block}/call/doc_fill`, { section: "s1", markdown: "From **carolbox**." });

  await platform(carol, "/docs");
  const row = carol.locator("[data-list-rows] [data-doc]", { hasText: "Carol's notes" });
  await expect
    .poll(
      async () => {
        await carol.reload();
        await expect(carol.locator("[data-docs-loading]")).toHaveCount(0, { timeout: 15_000 });
        return row.count();
      },
      { timeout: 60_000, intervals: [2000] },
    )
    .toBe(1);
  await row.click();
  await carol.locator("[data-list-preview] [data-open-doc]").click();
  const page = carol.locator("[data-doc-page]");
  await expect(page.locator(".doc-section[data-section=s1] strong")).toHaveText("carolbox", { timeout: 15_000 });
  await expect(page.locator("[data-doc-copy]")).toContainText("carolbox");
});
