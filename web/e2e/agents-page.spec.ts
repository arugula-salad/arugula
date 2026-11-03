// The Agents page (#403): a machine's agent recipes and its team's agents.
// One daemon with the `agent-delegation` flag; Claude Code's adapter is the fake ACP
// agent (playwright.config.ts). A project is added; a recipe is made in it
// from the form, edited (a key the form doesn't know is kept), offered (the
// Team tab lists it), run here (an agent block wearing it), and deleted.
// Both tabs are list pages (Arugula #590): a recipe's buttons are in the
// preview of the row that's selected, and the form takes the preview's place.
// A task waiting for you is answered without selecting anything. What the
// Team tab shows of other machines and people is served to the page here; the
// real ones, on a local control, are agents-delegate.spec.ts's.

import { spawn, type ChildProcess } from "node:child_process";
import { existsSync, mkdtempSync, readFileSync, realpathSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { expect, test, type Page } from "@playwright/test";
import { closeContexts, open } from "./helpers";
import { ANY, daemonPort } from "./ports";

let base = "";
let state = "";
let project = "";
let daemon: ChildProcess | undefined;

test.use({ baseURL: async ({}, use) => use(base) });
test.describe.configure({ mode: "serial" });
test.afterAll(closeContexts);

test.beforeAll(async () => {
  state = mkdtempSync(join(tmpdir(), "arugula-e2e-agents-page-"));
  writeFileSync(join(state, "flags.json"), JSON.stringify({ flags: { "agent-delegation": true } }));
  // The daemon names a project by its real path; on macOS tmpdir() is a link.
  project = realpathSync(mkdtempSync(join(tmpdir(), "arugula-e2e-agents-proj-")));
  daemon = spawn(
    "../target/debug/arugulad",
    [
      ...["--listen", ANY, "--state-dir", state, "--shell", "bash --norc --noprofile", "--no-manager-env"],
      ...["--owner", "me@example.com", "--tailscale-socket", "/nonexistent/tailscaled.sock"],
    ],
    // Its own home: the recipes on the page are the ones made here.
    { stdio: "ignore", env: { ...process.env, HOME: state } },
  );
  base = `http://127.0.0.1:${await daemonPort(state, daemon)}`;
});

test.afterAll(() => {
  daemon?.kill("SIGKILL");
  for (const d of [state, project]) if (d) rmSync(d, { recursive: true, force: true });
});

/** The row selected, and its preview: where a recipe's or an agent's buttons are. */
const preview = async (page: Page, hook: string, name: string) => {
  await page.locator(`[${hook}="${name}"]`).click();
  const shown = page.locator("[data-list-preview]");
  await expect(shown.locator("h2")).toHaveText(name);
  return shown;
};

test("make a recipe in a project from the form, edit it, offer it, run it, delete it", async ({ page }) => {
  test.setTimeout(120_000);
  await open(page);
  // The page is a place in the bar, beside Panes and Swarm.
  await page.locator("[data-machine-recipes]").click();
  const p = page.locator("[data-agents-page=recipes]");
  await expect(p).toBeVisible();
  await expect(page.locator("[data-machine-recipes]")).toHaveAttribute("aria-current", "page");

  // A project of ours.
  await p.locator("[data-add-project]").click();
  await page.locator(".prompt input").fill(project);
  await page.locator(".prompt input").press("Enter");
  const section = p.locator(`[data-agents-scope="${project}"]`);
  await expect(section).toContainText("No recipes");

  // New, in that project.
  await p.locator("[data-new-recipe]").click();
  const form = p.locator("[data-recipe-form=new]");
  await form.locator("select[name=dir]").selectOption(project);
  await form.locator("input[name=name]").fill("fixer");
  await form.locator("input[name=description]").fill("Fixes failing tests");
  await form.locator("input[name=model]").fill("haiku");
  await form.locator("input[name=tools]").fill("Read, Edit");
  await form.locator("textarea[name=prompt]").fill("You fix tests.");
  await form.locator("[data-save-recipe]").click();
  await expect(section.locator("[data-recipe=fixer]")).toContainText("Fixes failing tests");
  const file = join(project, ".claude/agents/fixer.md");
  expect(readFileSync(file, "utf8")).toContain("tools: Read, Edit");

  // Someone's own key, then an edit from the page: kept.
  writeFileSync(file, readFileSync(file, "utf8").replace("name: fixer", "name: fixer\ncolor: blue"));
  const again = await preview(page, "data-recipe", "fixer");
  await expect(again).toContainText("Read, Edit");
  await expect(again.locator("[data-recipe-prompt]")).toHaveText("You fix tests.");
  await again.locator("[data-edit=fixer]").click();
  const edit = p.locator("[data-recipe-form=edit]");
  await edit.locator("input[name=description]").fill("Fixes the failing tests, minimally");
  await edit.locator("[data-save-recipe]").click();
  await expect(section.locator("[data-recipe=fixer]")).toContainText("minimally");
  const text = readFileSync(file, "utf8");
  expect(text).toContain("color: blue");
  expect(text).toContain("You fix tests.");

  // Offered: the Team tab lists it, on this machine, even when it was
  // looked at before the offer.
  await p.locator("[data-agents-tab=team]").click();
  await expect(page.locator("[data-agents-page=team] [data-agents-empty]")).toBeVisible();
  await page.locator("[data-agents-tab=recipes]").click();
  await (await preview(page, "data-recipe", "fixer")).locator("[data-offer=fixer]").click();
  await expect(section.locator("[data-recipe=fixer][data-offered=true]")).toBeVisible();
  await p.locator("[data-agents-tab=team]").click();
  const team = page.locator("[data-agents-page=team]");
  await expect(team.locator("[data-agent=fixer]")).toContainText("Fixes the failing tests");
  // The line to give an agent is in its preview.
  await expect((await preview(page, "data-agent", "fixer")).locator("[data-delegate=fixer]")).toHaveText("delegate {agent: fixer}");
  await page.locator("[data-agents-tab=recipes]").click();

  // Run here: an agent block wearing it, in the project.
  const before = await page.evaluate(() => window.__arugula.client.state!.panes.filter((x) => x.type === "agent").length);
  await (await preview(page, "data-recipe", "fixer")).locator("[data-run-here=fixer]").click();
  await page.locator(".prompt input").press("Enter");
  await expect.poll(() => page.evaluate(() => window.__arugula.client.state!.panes.filter((x) => x.type === "agent").length)).toBe(before + 1);
  await expect(page.locator("[data-agents-page]")).toHaveCount(0);

  // Delete, after asking: the file and its offer go.
  await page.evaluate(() => (location.hash = "agents"));
  await (await preview(page, "data-recipe", "fixer")).locator("[data-recipe-more]").click();
  await page.getByRole("menuitem", { name: "Delete…" }).click();
  await page.locator("[data-confirm-dialog]").getByRole("button", { name: "Delete" }).click();
  await expect(page.locator(`[data-agents-scope="${project}"]`)).toContainText("No recipes");
  expect(existsSync(file)).toBe(false);
  const offers = await page.evaluate(async () => (await window.__arugula.client.request("GET", "/api/a2a/offers")).json());
  expect(offers).toEqual([]);
});

test("the form takes the preview's place, stays while the pointer crosses rows, and shows what it saved", async ({ page }) => {
  await open(page);
  await page.evaluate(() => (location.hash = "agents"));
  const p = page.locator("[data-agents-page=recipes]");
  for (const name of ["alpha", "beta"]) {
    await p.locator("[data-new-recipe]").click();
    const form = p.locator("[data-list-preview] [data-recipe-form=new]");
    // A new one is typed straight away.
    await expect(form.locator("input[name=name]")).toBeFocused();
    await form.locator("select[name=dir]").selectOption(project);
    await form.locator("input[name=name]").fill(name);
    await form.locator("input[name=description]").fill(`The ${name} recipe`);
    await form.locator("[data-save-recipe]").click();
    await expect(form).toHaveCount(0);
    await expect(p.locator("[data-list-preview] h2")).toHaveText(name);
    await expect(p.locator(`[data-recipe=${name}]`)).toHaveAttribute("aria-selected", "true");
  }
  await (await preview(page, "data-recipe", "alpha")).locator("[data-edit=alpha]").click();
  const form = p.locator("[data-recipe-form=edit]");
  await p.locator("[data-recipe=beta]").hover();
  await page.waitForTimeout(300);
  await expect(form).toBeVisible();
  await expect(form.locator("input[name=name]")).toHaveValue("alpha");
  await form.locator("[data-cancel-recipe]").click();
  await expect(form).toHaveCount(0);

  // Search and the offered count narrow it; the keys move through it.
  await page.keyboard.press("/");
  await page.keyboard.type("beta");
  await expect(p.locator("[data-recipe]")).toHaveCount(1);
  await page.keyboard.press("Escape");
  await expect(p.locator("[data-recipe]")).toHaveCount(2);
  await p.locator('[data-list-count="offered"]').click();
  await expect(p.locator("[data-agents-none]")).toHaveText("No recipe is offered to your team.");
  await p.locator('[data-list-count="offered"]').click();
  await p.locator("[data-recipe=alpha]").click();
  await page.keyboard.press("j");
  await expect(p.locator("[data-list-preview] h2")).toHaveText("beta");

  // A project added by hand comes off from the ⋯; its files stay.
  await p.locator("[data-recipe-more]").click();
  await page.getByRole("menuitem", { name: /^Stop listing/ }).click();
  await expect(p.locator("[data-recipe]")).toHaveCount(0);
  expect(existsSync(join(project, ".claude/agents/beta.md"))).toBe(true);
});

/** The Team tab with shelves, a task and a grant from a team. */
const team = async (page: Page) => {
  const posts: { path: string; body: unknown }[] = [];
  let waiting = [{ id: "t1", history: [{ parts: [{ text: "Please check why the nightly build fails" }] }], metadata: { arugula: { agent: "reviewer", caller: "Dana" } } }];
  await page.route(/\/api\/a2a\/catalog/, (r) =>
    r.fulfill({
      json: {
        machines: [
          { machine: "m1", name: "buildbox", owner: "", here: true, online: true, note: null, fetched_ms: null, agents: [{ name: "reviewer", description: "Reviews diffs" }] },
          { machine: "m2", name: "attic", owner: "", here: false, online: false, note: "offline", fetched_ms: Date.now() - 3_600_000, agents: [{ name: "archivist", description: "Files things away" }] },
          { machine: "m3", name: "danas-laptop", owner: "Dana", here: false, online: true, note: null, fetched_ms: null, agents: [{ name: "fixer", description: "Fixes tests" }] },
          { machine: "m4", name: "spare", owner: "", here: false, online: true, note: null, fetched_ms: null, agents: [] },
        ],
      },
    }),
  );
  await page.route(/\/api\/a2a\/waiting$/, (r) => r.fulfill({ json: { waiting } }));
  await page.route(/\/api\/a2a\/grants$/, (r) => {
    if (r.request().method() === "POST") posts.push({ path: "grants", body: r.request().postDataJSON() });
    return r.fulfill({ json: r.request().method() === "POST" ? {} : [{ account: "a9", name: "Sam", agent: "reviewer", until_ms: Date.now() + 3_600_000 }] });
  });
  await page.route(/\/api\/a2a\/tasks\/.*\/consent$/, (r) => {
    posts.push({ path: "consent", body: r.request().postDataJSON() });
    waiting = [];
    return r.fulfill({ json: {} });
  });
  await open(page);
  await page.evaluate(() => (location.hash = "agents=team"));
  await expect(page.locator("[data-agents-page=team] [data-agent=reviewer]")).toBeVisible();
  return posts;
};

test("Team: machines are groups, offline is said, and a task waits in view until it's answered", async ({ page }) => {
  const posts = await team(page);
  const p = page.locator("[data-agents-page=team]");
  await expect(p.locator('[data-shelf="attic"] .list-group')).toContainText("offline, as seen");
  await expect(p.locator('[data-shelf="danas-laptop"] .list-group')).toContainText("Dana's");
  await expect(p.locator("[data-quiet=spare]")).toContainText("offers no agents");

  // Only your own, on a machine that's up, has Run.
  await expect((await preview(page, "data-agent", "reviewer")).locator("[data-run=reviewer]")).toHaveText("Run on buildbox…");
  await expect((await preview(page, "data-agent", "archivist")).locator("[data-run]")).toHaveCount(0);
  const theirs = await preview(page, "data-agent", "fixer");
  await expect(theirs.locator("[data-run]")).toHaveCount(0);
  await expect(theirs.locator("[data-delegate=fixer]")).toHaveText("delegate {agent: fixer, machine: danas-laptop}");

  // The buttons are outside the listbox, and on the page with nothing selected for it.
  const ask = p.locator("[data-waiting=t1]");
  await expect(ask).toContainText("Dana asks reviewer");
  await expect(ask).toContainText("nightly build");
  await expect(p.locator("[data-list-rows] [data-waiting]")).toHaveCount(0);
  for (const b of ["data-allow-once", "data-allow-hour", "data-deny"]) await expect(ask.locator(`[${b}]`)).toBeVisible();
  await ask.locator("[data-allow-hour]").click();
  await expect(p.locator("[data-agents-waiting]")).toHaveCount(0);
  expect(posts).toContainEqual({ path: "consent", body: { answer: "hour" } });

  await expect(p.locator('[data-grant="Sam/reviewer"]')).toContainText("may use reviewer");
  await p.locator('[data-grant="Sam/reviewer"] [data-revoke]').click();
  await expect.poll(() => posts.some((x) => x.path === "grants")).toBe(true);

  // A filter narrows the list, and the sections beside it step aside.
  await p.locator('[data-list-count="yours"]').click();
  await expect(p.locator("[data-agent]")).toHaveCount(2);
  await expect(p.locator("[data-agents-quiet]")).toHaveCount(0);
});

test("Team: a card with no description still lists, and search doesn't stop on it", async ({ page }) => {
  // A row here is the card another machine answered with, as it sent it.
  await team(page);
  await page.route(/\/api\/a2a\/catalog/, (r) =>
    r.fulfill({
      json: {
        machines: [
          { machine: "m1", name: "buildbox", owner: "", here: true, online: true, note: null, fetched_ms: null, agents: [{ name: "reviewer" }] },
          { machine: "m3", name: "danas-laptop", owner: "Dana", here: false, online: true, note: null, fetched_ms: null, agents: [{ name: "fixer" }] },
        ],
      },
    }),
  );
  await page.reload();
  const p = page.locator("[data-agents-page=team]");
  await expect(p.locator("[data-agent=reviewer]")).toBeVisible();
  await p.locator("[data-list-search]").fill("fixer");
  await expect(p.locator("[data-agent]")).toHaveCount(1);
  await expect(p.locator("[data-agent=fixer]")).toBeVisible();
  await expect((await preview(page, "data-agent", "fixer")).locator("[data-delegate=fixer]")).toBeVisible();
});

for (const [width, both] of [[1440, true], [966, true], [700, false], [400, false]] as const) {
  test(`at ${width} px nothing overlaps or scrolls sideways`, async ({ page }) => {
    await team(page);
    await page.setViewportSize({ width, height: 800 });
    const p = page.locator("[data-agents-page=team]");
    const wide = () => page.evaluate(() => document.documentElement.scrollWidth);
    expect(await wide()).toBeLessThanOrEqual(width);
    const ask = p.locator("[data-waiting=t1]");
    await expect(ask.locator("[data-deny]")).toBeInViewport();
    const shown = p.locator("[data-list-preview]");
    if (both) await expect(shown).toBeVisible();
    else {
      await expect(shown).toBeHidden();
      await p.locator("[data-agent=reviewer]").click();
      await expect(p.locator("[data-list-rows]")).toBeHidden();
      await expect(shown.locator("[data-run=reviewer]")).toBeVisible();
      expect(await wide()).toBeLessThanOrEqual(width);
      await p.locator("[data-list-back]").click();
    }
    await expect(p.locator("[data-list-rows]")).toBeVisible();
  });
}
