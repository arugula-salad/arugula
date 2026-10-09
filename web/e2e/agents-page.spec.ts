// The Agents page (#403): a machine's agent recipes and its team's agents.
// One daemon with the `agents` flag; Claude Code's adapter is the fake ACP
// agent (playwright.config.ts). A project is added; a recipe is made in it
// from the form, edited (a key the form doesn't know is kept), offered (the
// Team tab lists it), run here (an agent block wearing it), and deleted.

import { spawn, type ChildProcess } from "node:child_process";
import { existsSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { expect, test } from "@playwright/test";
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
  writeFileSync(join(state, "flags.json"), JSON.stringify({ flags: { agents: true } }));
  project = mkdtempSync(join(tmpdir(), "arugula-e2e-agents-proj-"));
  daemon = spawn(
    "../target/debug/arugulad",
    [
      ...["--listen", ANY, "--state-dir", state, "--shell", "bash --norc --noprofile", "--no-manager-env"],
      ...["--owner", "me@example.com", "--tailscale-socket", "/nonexistent/tailscaled.sock"],
    ],
    { stdio: "ignore" },
  );
  base = `http://127.0.0.1:${await daemonPort(state, daemon)}`;
});

test.afterAll(() => {
  daemon?.kill("SIGKILL");
  for (const d of [state, project]) if (d) rmSync(d, { recursive: true, force: true });
});

test("make a recipe in a project from the form, edit it, offer it, run it, delete it", async ({ page }) => {
  test.setTimeout(120_000);
  await open(page);
  // The session menu offers the page.
  await page.locator(".session-button").click();
  await page.getByRole("menuitem", { name: "Agents…" }).click();
  const p = page.locator("[data-agents-page=recipes]");
  await expect(p).toBeVisible();

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
  await section.locator("[data-edit=fixer]").click();
  const edit = p.locator("[data-recipe-form=edit]");
  await edit.locator("input[name=description]").fill("Fixes the failing tests, minimally");
  await edit.locator("[data-save-recipe]").click();
  await expect(section.locator("[data-recipe=fixer]")).toContainText("minimally");
  const text = readFileSync(file, "utf8");
  expect(text).toContain("color: blue");
  expect(text).toContain("You fix tests.");

  // Offered: the Team tab lists it, on this machine.
  await section.locator("[data-offer=fixer]").click();
  await expect(section.locator("[data-recipe=fixer][data-offered=true]")).toBeVisible();
  await p.locator("[data-agents-tab=team]").click();
  const team = page.locator("[data-agents-page=team]");
  await expect(team.locator("[data-agent=fixer]")).toContainText("Fixes the failing tests");
  await page.locator("[data-agents-tab=recipes]").click();

  // Run here: an agent block wearing it, in the project.
  const before = await page.evaluate(() => window.__arugula.client.state!.panes.filter((x) => x.type === "agent").length);
  await section.locator("[data-run-here=fixer]").click();
  await page.locator(".prompt input").press("Enter");
  await expect.poll(() => page.evaluate(() => window.__arugula.client.state!.panes.filter((x) => x.type === "agent").length)).toBe(before + 1);
  await expect(page.locator("[data-agents-page]")).toHaveCount(0);

  // Delete, after asking: the file and its offer go.
  await page.evaluate(() => (location.hash = "agents"));
  await page.locator(`[data-agents-scope="${project}"] [data-delete=fixer]`).click();
  await page.locator("[data-confirm-dialog]").getByRole("button", { name: "Delete" }).click();
  await expect(page.locator(`[data-agents-scope="${project}"]`)).toContainText("No recipes");
  expect(existsSync(file)).toBe(false);
  const offers = await page.evaluate(async () => (await window.__arugula.client.request("GET", "/api/a2a/offers")).json());
  expect(offers).toEqual([]);
});
