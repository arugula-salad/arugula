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
import { attacker, measure, prototype, prototypeRan } from "./doc-steps";
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
/** Alice's machine geek's daemon, once it has joined. */
let geek = "";

/** A POST to geek's daemon, as its owner. */
async function geekPost(path: string, body: unknown) {
  const r = await fetch(geek + path, {
    method: "POST",
    headers: { "content-type": "application/json", authorization: `Bearer ${localToken}` },
    body: JSON.stringify(body),
  });
  const text = await r.text();
  expect(r.ok, `${path}: ${text}`).toBe(true);
  return JSON.parse(text) as { block?: number };
}

/** A Doc on geek's machine with one section filled, so its copy lands in Alice's Mine; the block's id. */
async function machineDoc(title: string) {
  const made = await geekPost("/api/blocks", { type: "doc", config: { title }, local: true });
  await geekPost(`/api/blocks/${made.block}/call/doc_outline`, { title, sections: [{ id: "s1", heading: "Goals", intent: "what ships" }] });
  await geekPost(`/api/blocks/${made.block}/call/doc_fill`, { section: "s1", markdown: `${title} says **hello**.` });
  return made.block!;
}

async function platform(p: Page, path = "/") {
  await p.goto(`${app}${path}`);
  await expect(p.locator("[data-platform-bar], [data-chat]").first()).toBeVisible({ timeout: 15_000 });
}

/** The Doc page's ⋯, then its "Share with…": the dialog with each team's state (the header's own Share goes straight to the one team). */
async function shareWithDialog(p: Page) {
  await p.locator("[data-doc-page] [data-doc-more]").click();
  await p.getByRole("menuitem", { name: "Share with…" }).click();
}

/** A picture of the page for the PR, when E2E_SHOTS names a folder. */
const shot = async (p: Page, name: string) => {
  if (process.env.E2E_SHOTS) await p.screenshot({ path: `${process.env.E2E_SHOTS}/${name}` });
};

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
  await expect(page.locator("[data-doc-maker]")).toHaveText("you");

  // A first section, with a chart.
  await page.locator("[data-doc-heading-input]").fill("Goals");
  await page.locator("[data-doc-markdown-input]").fill(`Ship **0.64** this week.\n\n${CHART}`);
  await page.locator("[data-doc-save]").click();
  const goals = page.locator(".doc-section[data-section=s1]");
  await expect(goals.locator(".doc-heading")).toHaveText("Goals");
  await expect(goals.locator(".doc-body strong")).toHaveText("0.64");
  await expect(goals.locator(".chart-box svg")).toBeVisible();
  await expect(page.locator("[data-doc-progress]")).toHaveCount(0);
});

test("the Doc's column widens with the page (Arugula #783)", async () => {
  await alice.goto(`${app}${docPath}`);
  const page = alice.locator("[data-doc-page]");
  await expect(page.locator(".doc-title")).toHaveText("Release plan");
  const was = alice.viewportSize()!;
  try {
    await alice.setViewportSize({ width: 900, height: 900 });
    expect(await measure(alice, "[data-doc-page]")).toBe(68);
    await alice.setViewportSize({ width: 1600, height: 900 });
    expect(await measure(alice, "[data-doc-page]")).toBe(88);
  } finally {
    await alice.setViewportSize(was);
  }
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
  await page.locator("[data-confirm-remove]").click();
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
  await make("Private ideas", "Yours, private");

  // Team is what /docs opens on; the private Docs are under Mine (Arugula #418).
  const groups = alice.locator("[data-list-rows] [role=group] .list-group");
  await expect(alice.locator("[data-list-count=team]")).toHaveAttribute("aria-pressed", "true", { timeout: 15_000 });
  await expect(groups).toHaveText([/^Acme\s*1$/]);
  await expect(alice.locator("[data-list-rows] [data-doc]")).toHaveText([/Team roadmap/]);
  await expect(alice.locator("[data-list-count=team] b")).toHaveText("1");
  await expect(alice.locator("[data-list-count=mine] b")).toHaveText("2");
  await expect(alice.locator("[data-list-count=all]")).toHaveCount(0);
  await alice.locator("[data-list-count=mine]").click();
  await expect(groups).toHaveText([/^Mine\s*2$/]);
  await expect(alice.locator("[data-list-rows] [data-doc]")).toHaveText([/Private ideas/, /Private notes/]);
  // Picking the count that's on does nothing; there's no "all".
  await alice.locator("[data-list-count=mine]").click();
  await expect(alice.locator("[data-list-count=mine]")).toHaveAttribute("aria-pressed", "true");
  await expect(alice.locator("[data-list-rows] [data-doc]")).toHaveCount(2);
  // A row is one line: no opening under the title (it was about 52px with one).
  await expect(alice.locator("[data-list-rows] [data-doc]", { hasText: "says hello" })).toHaveCount(0);
  expect((await alice.locator("[data-list-rows] [data-doc]").first().boundingBox())!.height).toBeLessThan(40);
  // The first row is selected, and its whole Doc is on the right.
  const preview = alice.locator("[data-list-preview]");
  await expect(preview.locator(".doc-section[data-section=s1] .chart-box svg")).toBeVisible({ timeout: 15_000 });
  // Sweeping the pointer over the rows leaves the first Doc, and reads none;
  // a click shows the one clicked (Arugula #657).
  const head = preview.locator(".doc-title");
  // The Doc's own head is the preview's one title (Arugula #659).
  await expect(head).toHaveCount(1);
  await expect(head).toHaveText("Private ideas");
  const reads: string[] = [];
  alice.on("request", (r) => {
    if (/\/docs\/pdc_/.test(r.url())) reads.push(r.url());
  });
  for (const title of ["Private notes", "Private ideas", "Private notes"]) {
    const b = (await alice.locator("[data-list-rows] [data-doc]", { hasText: title }).boundingBox())!;
    await alice.mouse.move(b.x + 20, b.y + b.height / 2, { steps: 6 });
    await alice.waitForTimeout(400);
  }
  await expect(head).toHaveText("Private ideas");
  expect(reads).toEqual([]);
  await alice.locator("[data-list-rows] [data-doc]", { hasText: "Private notes" }).click();
  await expect(head).toHaveText("Private notes");
  await expect(preview.locator(".doc-section[data-section=s1] .chart-box svg")).toBeVisible({ timeout: 15_000 });
  // The filter is this browser's: a reload stays in Mine.
  await alice.reload();
  await expect(alice.locator("[data-list-count=mine]")).toHaveAttribute("aria-pressed", "true", { timeout: 15_000 });
  await alice.locator("[data-list-count=team]").click();
  await expect(alice.locator("[data-list-rows] [data-doc]")).toHaveText([/Team roadmap/]);
  await alice.locator("[data-list-rows] [data-doc]", { hasText: "Team roadmap" }).click();
  await expect(head).toHaveText("Team roadmap");
  await preview.locator("[data-open-doc]").click();
  await expect(alice).toHaveURL(/\/p\/prj_[0-9a-f]+-acme\/docs\/pdc_[0-9a-f]{32}-team-roadmap$/);
  roadmapPath = new URL(alice.url()).pathname;

  // Bob reads the team's, and has no trace of hers.
  await platform(bob, "/docs");
  await expect(bob.locator("[data-list-rows] [data-doc]")).toHaveText([/Team roadmap/], { timeout: 15_000 });
  await expect(bob.getByText("Private notes")).toHaveCount(0);
  await expect(bob.locator("[data-list-rows] .list-group")).toHaveText([/^Acme\s*1$/]);
  await expect(bob.locator("[data-list-count=mine] b")).toHaveText("0");
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
  geek = base;
  const post = geekPost;
  const made = await post("/api/blocks", { type: "doc", config: { title: "Geek's plan" }, local: true });
  await post(`/api/blocks/${made.block}/call/doc_outline`, { title: "Geek's plan", sections: [{ id: "s1", heading: "Goals", intent: "what ships" }] });
  await post(`/api/blocks/${made.block}/call/doc_fill`, { section: "s1", markdown: "From **geek**." });

  // The copy is put a moment after the fill; the list is read on load.
  const row = () => alice.locator("[data-list-rows] [data-doc]", { hasText: "Geek's plan" });
  await platform(alice, "/docs");
  // Mine, where copies are; the reloads below keep it.
  await alice.locator("[data-list-count=mine]").click();
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
  // A copy is Mine, and its row says which machine it came from.
  await expect(row()).toHaveAttribute("data-private", "true");
  await expect(row().locator("[data-doc-machine]")).toHaveText("On geek");
  await row().click();
  const preview = alice.locator("[data-list-preview]");
  await expect(preview.locator("[data-doc-copy-fact]")).toHaveText("a copy from geek");
  await preview.locator("[data-open-doc]").click();

  const page = alice.locator("[data-doc-page]");
  await expect(page.locator(".doc-title")).toHaveText("Geek's plan", { timeout: 15_000 });
  await expect(page).toHaveAttribute("data-writer", "false");
  await expect(page.locator("[data-doc-writer]")).toHaveCount(0);
  await expect(page.locator("[data-doc-copy]")).toContainText("Copy from geek");
  // The copy line says it; the byline doesn't repeat it.
  await expect(page.locator("[data-doc-maker]")).toHaveCount(0);
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

  // Share with… (Arugula #418): a team Doc made from the copy, a snapshot
  // Acme owns. The copy stays read-only and geek's.
  const copyUrl = alice.url();
  await shareWithDialog(alice);
  const dialog = alice.locator("[data-share-with-dialog]");
  const acme = dialog.locator("[data-share-with-project]", { hasText: "Acme" });
  await expect(acme.locator("[data-share-with-go]")).toBeVisible({ timeout: 15_000 });
  await shot(alice, "share-with-modal.png");
  await acme.locator("[data-share-with-go]").click();
  await expect(acme.locator("[data-share-with-made]")).toContainText("Made", { timeout: 15_000 });
  await expect(acme.locator("[data-share-with-update]")).toBeDisabled();
  await shot(alice, "share-with-modal-shared.png");
  await acme.locator("[data-share-with-open]").click();
  await expect(page).toHaveAttribute("data-writer", "true", { timeout: 15_000 });
  await expect(page.locator("[data-doc-made-from]")).toHaveCount(0);
  await expect(page.locator("[data-doc-copy]")).toHaveCount(0);
  await expect(page.locator(".doc-section[data-section=s1] .doc-body")).toContainText("again");
  const teamDoc = /\/p\/(prj_[0-9a-f]+)[^/]*\/docs\/(pdc_[0-9a-f]{32})/.exec(new URL(alice.url()).pathname)!;
  expect(teamDoc[2]).not.toBe(m[2]);
  // The team edits its Doc; geek changes the copy. Update copy asks first.
  const edit = await alice.evaluate(async ([project, id]) => {
    const r = await fetch(`/api/projects/${project}/docs/${id}`, {
      method: "PUT",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ section: "s1", markdown: "Ours." }),
    });
    return r.status;
  }, [teamDoc[1], teamDoc[2]]);
  expect(edit).toBe(200);
  await post(`/api/blocks/${made.block}/call/doc_fill`, { section: "s1", markdown: "Changed on **geek** once more." });
  await alice.goto(copyUrl);
  await expect(async () => {
    await alice.reload();
    await shareWithDialog(alice);
    await expect(acme.locator("[data-share-with-update]")).toBeEnabled({ timeout: 3_000 });
  }).toPass({ timeout: 60_000, intervals: [2000] });
  await acme.locator("[data-share-with-update]").click();
  const sure = alice.locator("[data-share-with-confirm]");
  await expect(sure).toContainText("Acme edited its copy since; Update copy replaces their edits.");
  await sure.locator("[data-share-with-replace]").click();
  await expect(sure).toHaveCount(0);
  await alice.goto(`${app}/p/${teamDoc[1]}/docs/${teamDoc[2]}`);
  await expect(page.locator(".doc-section[data-section=s2] .doc-body")).toContainText("once more", { timeout: 15_000 });
  await alice.goto(copyUrl);

  // The copy is the project's to delete (native #673), and the page says
  // geek's next change brings a new one, which it does, under a new id.
  await page.locator("[data-doc-delete]").click();
  await expect(page.locator("[data-doc-confirm]")).toContainText("geek is your machine");
  await page.locator("[data-confirm-remove]").click();
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

// Arugula #818: three of Mine's Docs are put in Acme in one step. Two are
// copies of geek's Docs, one of which Acme already has; the third was
// written here. Each says what became of it, and Team grows by the two that
// were made. Shift-click ranges follow the rows as shown, and leaving Mine
// forgets what was ticked.
test("Mine's Docs are shared into the team together, each with its outcome (Arugula #818)", async () => {
  test.setTimeout(180_000);
  const copy = async (title: string) => {
    const made = await geekPost("/api/blocks", { type: "doc", config: { title }, local: true });
    await geekPost(`/api/blocks/${made.block}/call/doc_outline`, { title, sections: [{ id: "s1", heading: "Goals", intent: "what ships" }] });
    await geekPost(`/api/blocks/${made.block}/call/doc_fill`, { section: "s1", markdown: `${title} says **hello**.` });
  };
  await copy("Bulk one");
  await copy("Bulk two");
  const row = (title: string) => alice.locator("[data-list-rows] [data-doc]", { hasText: title });
  const mine = alice.locator("[data-list-count=mine]");
  await platform(alice, "/docs");
  await mine.click();
  await expect
    .poll(
      async () => {
        await alice.reload();
        await expect(alice.locator("[data-docs-loading]")).toHaveCount(0, { timeout: 15_000 });
        return (await row("Bulk one").count()) + (await row("Bulk two").count());
      },
      { timeout: 60_000, intervals: [2000] },
    )
    .toBe(2);

  // "Bulk two" is in Acme already: shared from its page first.
  await row("Bulk two").click();
  await alice.locator("[data-list-preview] [data-open-doc]").click();
  await shareWithDialog(alice);
  const acme = alice.locator("[data-share-with-dialog] [data-share-with-project]", { hasText: "Acme" });
  await acme.locator("[data-share-with-go]").click();
  await expect(acme.locator("[data-share-with-made]")).toContainText("Made", { timeout: 15_000 });
  await platform(alice, "/docs");
  await expect(mine).toHaveAttribute("aria-pressed", "true");
  await expect(row("Bulk two")).toHaveCount(1, { timeout: 15_000 });
  const team = alice.locator("[data-list-count=team] b");
  const teamWas = Number(await team.textContent());

  // Ticking: a box on hover, shift-click for the range as it's shown (the
  // search narrows it), a group's box for all of it, Space and Esc.
  const boxes = alice.locator("[data-list-rows] [data-doc] [data-list-check]");
  const all = await alice.locator("[data-list-rows] [data-doc]").count();
  expect(all).toBeGreaterThanOrEqual(4);
  await expect(alice.locator("[data-docs-share-bar]")).toHaveCount(0);
  await alice.mouse.move(2, 2);
  // (The row the keys are on shows its box; the others wait for the pointer.)
  await expect(boxes.nth(1)).toHaveCSS("opacity", "0");
  // Shift-click ticks everything between, the rows in between included.
  await boxes.nth(0).click();
  await boxes.nth(2).click({ modifiers: ["Shift"] });
  await expect(alice.locator("[data-docs-selected]")).toHaveText("3 selected");
  await expect(alice.locator("[data-list-rows] [data-list-check]:checked")).toHaveCount(3);
  await alice.locator("[data-docs-clear]").click();
  await alice.locator("[data-list-search]").fill("Bulk");
  await expect(alice.locator("[data-list-rows] [data-doc]")).toHaveCount(2);
  await boxes.nth(0).click();
  await boxes.nth(1).click({ modifiers: ["Shift"] });
  await expect(alice.locator("[data-docs-selected]")).toHaveText("2 selected");
  // A box that's ticked shows with the pointer elsewhere.
  await alice.mouse.move(2, 2);
  await expect(boxes.nth(1)).toHaveCSS("opacity", "1");
  // A search that hides a ticked row drops it: it can't be acted on unseen.
  await alice.locator("[data-list-search]").fill("Bulk one");
  await expect(alice.locator("[data-docs-selected]")).toHaveText("1 selected");
  await alice.locator("[data-list-search]").fill("");
  await expect(alice.locator("[data-docs-selected]")).toHaveText("1 selected");
  await alice.locator("[data-docs-clear]").click();
  await expect(alice.locator("[data-docs-share-bar]")).toHaveCount(0);
  // The group's box ticks every row in Mine; the keys clear it.
  await alice.locator("[data-list-group-check]").click();
  await expect(alice.locator("[data-docs-selected]")).toHaveText(`${all} selected`);
  await alice.locator("[data-list-rows]").focus();
  await alice.keyboard.press("Escape");
  await expect(alice.locator("[data-docs-share-bar]")).toHaveCount(0);
  // Space ticks the row the keys are on.
  await alice.keyboard.press("Space");
  await expect(alice.locator("[data-docs-selected]")).toHaveText("1 selected");
  await alice.keyboard.press("Escape");

  // Switching to Team forgets what was ticked, so it can't be acted on unseen.
  await boxes.first().click();
  await expect(alice.locator("[data-docs-selected]")).toHaveText("1 selected");
  await alice.locator("[data-list-count=team]").click();
  await expect(alice.locator("[data-list-rows] [data-list-check]")).toHaveCount(0);
  await expect(alice.locator("[data-docs-share-bar]")).toHaveCount(0);
  await mine.click();
  await expect(alice.locator("[data-docs-share-bar]")).toHaveCount(0);

  // Three: both copies, and Private notes (written here).
  for (const title of ["Bulk one", "Bulk two", "Private notes"]) await row(title).locator("[data-list-check]").check();
  await expect(alice.locator("[data-docs-selected]")).toHaveText("3 selected");
  await shot(alice, "docs-share-bar.png");
  // One team to write in: the button names it.
  await expect(alice.locator("[data-docs-share]")).toHaveText("Share with Acme");
  await alice.locator("[data-docs-share]").click();
  const results = alice.locator("[data-docs-share-results]");
  await expect(results).toBeVisible({ timeout: 30_000 });
  await expect(results.locator("[data-move-result]")).toHaveCount(3);
  await expect(results.locator("[data-move-result]", { hasText: "Bulk one" })).toHaveAttribute("data-move-result", "made");
  await expect(results.locator("[data-move-result]", { hasText: "Bulk two" })).toHaveAttribute("data-move-result", "there");
  await expect(results.locator("[data-move-result]", { hasText: "Bulk two" }).locator("[data-move-link]")).toHaveText("Already in Acme");
  await expect(results.locator("[data-move-result]", { hasText: "Private notes" })).toHaveAttribute("data-move-result", "made");
  await expect(results.locator("[data-move-result]", { hasText: "Private notes" })).not.toContainText("pictures stay");
  await shot(alice, "docs-share-results.png");
  // The selection cleared, the originals stay in Mine, and Team grew by two.
  await expect(alice.locator("[data-docs-selected]")).toHaveCount(0);
  await expect(alice.locator("[data-list-rows] [data-list-check]:checked")).toHaveCount(0);
  await expect(row("Private notes")).toHaveCount(1);
  await expect(team).toHaveText(String(teamWas + 2), { timeout: 15_000 });
  // A made Doc's link opens the team's Doc.
  await results.locator("[data-move-result]", { hasText: "Bulk one" }).locator("[data-move-link]").click();
  await expect(alice).toHaveURL(/\/p\/prj_[0-9a-f]+-acme\/docs\/pdc_[0-9a-f]{32}/);
  await expect(alice.locator("[data-doc-page] .doc-title")).toHaveText("Bulk one", { timeout: 15_000 });
  await expect(alice.locator("[data-doc-page]")).toHaveAttribute("data-writer", "true");
});

// Arugula #820: a Doc offers the same three actions wherever it's shown, none
// needing it opened first. Share here is "with Acme", not the machine's "with
// a link". Two of geek's Docs go into Acme, one from its row's ⋯ and one from
// the preview; each says what became of it, a second Share says it's already
// there, and Team grows by one each time.
test("a Mine Doc is shared from its row's menu and from its preview, without opening it (Arugula #820)", async () => {
  test.setTimeout(180_000);
  for (const title of ["Row share", "Preview share"]) await machineDoc(title);
  const row = (title: string) => alice.locator("[data-list-rows] [data-doc]", { hasText: title });
  const toast = alice.locator("[data-platform-toast]");
  await platform(alice, "/docs");
  await alice.locator("[data-list-count=mine]").click();
  await expect
    .poll(
      async () => {
        await alice.reload();
        await expect(alice.locator("[data-docs-loading]")).toHaveCount(0, { timeout: 15_000 });
        return (await row("Row share").count()) + (await row("Preview share").count());
      },
      { timeout: 60_000, intervals: [2000] },
    )
    .toBe(2);
  const team = alice.locator("[data-list-count=team] b");
  const teamWas = Number(await team.textContent());

  // The row's ⋯ has the actions, and pressing it is its own: the row isn't selected or opened.
  const more = row("Row share").locator("[data-doc-more]");
  await more.click();
  await expect(alice.getByRole("menuitem")).toHaveText(["Copy link", "Share with Acme", "Share with…", "Open in browser"]);
  await expect(row("Row share")).toHaveAttribute("aria-selected", "false");
  await expect(alice).toHaveURL(/\/docs$/);
  await alice.getByRole("menuitem", { name: "Share with Acme" }).click();
  await expect(toast).toHaveText("Row share: Made", { timeout: 30_000 });
  await expect(team).toHaveText(String(teamWas + 1), { timeout: 15_000 });
  await expect(alice).toHaveURL(/\/docs$/);
  // Again: it's there already, and Team stays.
  await more.click();
  await alice.getByRole("menuitem", { name: "Share with Acme" }).click();
  await expect(toast).toHaveText("Row share: Already in Acme", { timeout: 30_000 });
  await expect(team).toHaveText(String(teamWas + 1));
  // The keys work: the ContextMenu key on the selected row opens its menu (the ⋯ isn't in the tab order, a row being one option), and Escape shuts it.
  await row("Row share").click();
  await alice.keyboard.press("Shift+F10");
  await expect(alice.getByRole("menuitem", { name: "Copy link" })).toBeVisible();
  await alice.keyboard.press("Escape");
  await expect(alice.getByRole("menu")).toHaveCount(0);
  // And so does a right click on the row.
  await row("Row share").click({ button: "right" });
  await expect(alice.getByRole("menuitem", { name: "Copy link" })).toBeVisible();
  await alice.keyboard.press("Escape");
  await expect(alice.getByRole("menu")).toHaveCount(0);
  await expect(alice).toHaveURL(/\/docs$/);

  // The preview's three, at its right: Copy link, Share with Acme, Open in browser.
  await row("Preview share").click();
  const preview = alice.locator("[data-list-preview]");
  await expect(preview.locator("[data-doc-actions] button")).toHaveCount(3);
  await expect(preview.locator("[data-doc-copy-link]")).toHaveAttribute("aria-label", "Copy link");
  await expect(preview.locator("[data-doc-open-browser]")).toHaveAttribute("aria-label", "Open in browser");
  const share = preview.locator("[data-doc-share]");
  await expect(share).toHaveAttribute("aria-label", "Share with Acme");
  await expect(share).not.toHaveAttribute("data-older", /.*/);
  await share.click();
  await expect(toast).toHaveText("Preview share: Made", { timeout: 30_000 });
  await expect(team).toHaveText(String(teamWas + 2), { timeout: 15_000 });
  await share.click();
  await expect(toast).toHaveText("Preview share: Already in Acme", { timeout: 30_000 });
  await expect(team).toHaveText(String(teamWas + 2));
  await shot(alice, "docs-doc-actions-preview.png");
  // Open in browser is a new tab on the Doc's page, and this one stays.
  const [tab] = await Promise.all([alice.context().waitForEvent("page"), preview.locator("[data-doc-open-browser]").click()]);
  await expect(tab).toHaveURL(/\/p\/prj_[0-9a-f]+[^/]*\/docs\/pdc_[0-9a-f]{32}/);
  await tab.close();
  await expect(alice).toHaveURL(/\/docs$/);
});

// A team Doc made from a copy is older once the copy changes: Share reads
// Update, with a dot, and brings the team's Doc up to date from the preview.
test("Update shows for an older team copy and runs Update copy (Arugula #820)", async () => {
  test.setTimeout(180_000);
  const block = await machineDoc("Update share");
  const row = alice.locator("[data-list-rows] [data-doc]", { hasText: "Update share" });
  const toast = alice.locator("[data-platform-toast]");
  /** /docs under Mine, with the Doc selected: its preview. */
  const preview = async () => {
    await platform(alice, "/docs");
    await alice.locator("[data-list-count=mine]").click();
    await row.click();
    return alice.locator("[data-list-preview]");
  };
  await expect
    .poll(
      async () => {
        await alice.reload();
        await expect(alice.locator("[data-docs-loading]")).toHaveCount(0, { timeout: 15_000 });
        return row.count();
      },
      { timeout: 60_000, intervals: [2000] },
    )
    .toBe(1);
  let share = (await preview()).locator("[data-doc-share]");
  await expect(share).toHaveAttribute("aria-label", "Share with Acme");
  await share.click();
  await expect(toast).toHaveText("Update share: Made", { timeout: 30_000 });
  await expect(share).not.toHaveAttribute("data-older", /.*/);

  // The machine changes the Doc: the team's copy is older than it now.
  await geekPost(`/api/blocks/${block}/call/doc_fill`, { section: "s1", markdown: "Changed on **geek**." });
  await expect(async () => {
    share = (await preview()).locator("[data-doc-share]");
    await expect(share).toHaveAttribute("data-older", "", { timeout: 3_000 });
  }).toPass({ timeout: 60_000, intervals: [2000] });
  await expect(share).toHaveAttribute("aria-label", "Update Acme's copy: it's older than this Doc");
  await shot(alice, "docs-doc-actions-update.png");
  // The row's menu says it too.
  await row.locator("[data-doc-more]").click();
  await expect(alice.getByRole("menuitem")).toHaveText(["Copy link", "Update", "Share with…", "Open in browser"]);
  await alice.keyboard.press("Escape");
  await share.click();
  await expect(toast).toHaveText("Acme's copy now matches", { timeout: 30_000 });
  await expect(share).not.toHaveAttribute("data-older", /.*/, { timeout: 15_000 });
  await expect(share).toHaveAttribute("aria-label", "Share with Acme");
});

// A team's Doc has no Share: it's already the team's. Its row, its preview
// and its page still have Copy link and Open in browser. A Mine Doc's page has
// all three at the header's right, and Share says whom.
test("a Team Doc has no Share, and a Doc's page has the three actions (Arugula #820)", async () => {
  test.setTimeout(120_000);
  const row = (title: string) => alice.locator("[data-list-rows] [data-doc]", { hasText: title });
  await platform(alice, "/docs");
  await alice.locator("[data-list-count=team]").click();
  await expect(row("Row share")).toHaveCount(1, { timeout: 15_000 });
  await row("Row share").locator("[data-doc-more]").click();
  await expect(alice.getByRole("menuitem")).toHaveText(["Copy link", "Open in browser"]);
  await alice.keyboard.press("Escape");
  await row("Row share").click();
  const preview = alice.locator("[data-list-preview]");
  await expect(preview.locator("[data-doc-actions] button")).toHaveCount(2);
  await expect(preview.locator("[data-doc-copy-link]")).toBeVisible();
  await expect(preview.locator("[data-doc-open-browser]")).toBeVisible();
  await expect(preview.locator("[data-doc-share]")).toHaveCount(0);
  await preview.locator("[data-open-doc]").click();
  const page = alice.locator("[data-doc-page]");
  await expect(page.locator(".doc-title")).toHaveText("Row share", { timeout: 15_000 });
  await expect(page.locator("[data-doc-actions] [data-doc-copy-link]")).toBeVisible();
  await expect(page.locator("[data-doc-actions] [data-doc-open-browser]")).toBeVisible();
  await expect(page.locator("[data-doc-share]")).toHaveCount(0);

  await platform(alice, "/docs");
  await alice.locator("[data-list-count=mine]").click();
  await row("Row share").click();
  await alice.locator("[data-list-preview] [data-open-doc]").click();
  await expect(page.locator(".doc-title")).toHaveText("Row share", { timeout: 15_000 });
  const actions = page.locator("[data-doc-actions]");
  await expect(actions.locator("button:not([data-doc-more])")).toHaveCount(3);
  await expect(actions.locator("[data-doc-copy-link]")).toHaveAttribute("aria-label", "Copy link");
  await expect(actions.locator("[data-doc-share]")).toHaveAttribute("aria-label", "Share with Acme");
  await expect(actions.locator("[data-doc-open-browser]")).toHaveAttribute("aria-label", "Open in browser");
  await expect(page.locator("[data-doc-share-with]")).toHaveCount(0);
  await shot(alice, "docs-doc-page-actions.png");
  const [tab] = await Promise.all([alice.context().waitForEvent("page"), actions.locator("[data-doc-open-browser]").click()]);
  await expect(tab).toHaveURL(alice.url());
  await tab.close();
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
    await expect(page.locator("[data-doc-maker]")).toHaveText("you");
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
  geek = base;
  const post = geekPost;
  const made = await post("/api/blocks", { type: "doc", config: { title: "Carol's notes" }, local: true });
  await post(`/api/blocks/${made.block}/call/doc_outline`, { title: "Carol's notes", sections: [{ id: "s1", heading: "One", intent: "" }] });
  await post(`/api/blocks/${made.block}/call/doc_fill`, { section: "s1", markdown: "From **carolbox**." });

  await platform(carol, "/docs");
  // Her copies are Mine, and the counts aren't drawn until a Doc is: the
  // browser's remembered filter is what the reloads below open on.
  await carol.evaluate(() => localStorage.setItem("platform-docs-filter", "mine"));
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
