// Settings (Arugula #1104): one page, four scopes (You, This machine,
// Project, Team and billing), each a list of its sections with what's set,
// and the chosen one's controls beside it. The section is the address, so a
// link opens it; the old addresses (a project's settings, Personal's, a
// team's Settings tab, `#section`) land in their scope. On a phone, the
// list, then a section full width with a way back. On #971's fixture page
// (move-to-project-page.tsx): the platform's answers are this spec's.
import { devices, expect, test, type Page, type Route } from "@playwright/test";
import { fixturePage } from "./fixture-page";

const ME = "prj_aaaa0001";
const SITE = "prj_aaaa0002";

const projects = [
  { id: ME, name: "Personal", owner: { kind: "personal" }, team: null },
  {
    id: SITE,
    name: "Site",
    owner: { kind: "team", id: "tm_1", name: "Acme" },
    team: { id: "tm_1", name: "Acme", role: "owner", members: [{ account: "acct_a", name: "Sam", role: "owner" }, { account: "acct_b", name: "Jo", role: "editor" }] },
  },
];

/** The fixture page at `path`, with the platform's answers. */
async function open(page: Page, path: string) {
  const json = (r: Route, body: unknown, status = 200) => r.fulfill({ status, contentType: "application/json", body: JSON.stringify(body) });
  // What this spec doesn't answer, nobody does.
  await page.route(/\/api\//, (r) => json(r, { error: "not here" }, 404));
  await page.route(/\/api\/projects$/, (r) => json(r, { me: { account: "acct_a", login: "sam", name: "Sam" }, projects, hosted: false }));
  await page.route(/\/api\/notify\/prefs$/, (r) =>
    json(r, { channels: "mentions", dms: "all", threads: "all", snooze_until: null, tz: "America/New_York", quiet: [{ days: [1, 2, 3, 4, 5], from: 22 * 60, to: 8 * 60 }], all_notices: false, paused: false }),
  );
  await page.route(/\/api\/linked-products$/, (r) => json(r, { links: [] }));
  await page.route(/\/api\/agent-tokens$/, (r) =>
    json(r, { mcp_url: "https://example.test/mcp", tokens: [{ id: "tok_1", name: "ci-bot", created_at: 1, last_used_at: null, revoked_at: null, live: true, grants: [] }] }),
  );
  await page.route(/\/api\/grants$/, (r) => json(r, { grants: [] }));
  await page.route(/\/api\/projects\/[^/]+\/repos$/, (r) => json(r, { repos: [{ repo_id: 1, full_name: "acme/site", private: false, mirror_content: false, purge_after: null }] }));
  await page.route(/\/api\/(projects|owners)\/[^/]+\/settings$/, (r) => json(r, []));
  const body = await fixturePage("./move-to-project-page.tsx");
  await page.route(/\/(p\/prj_|settings|t\/)/, (r) => (r.request().resourceType() === "document" ? r.fulfill({ contentType: "text/html", body }) : r.fallback()));
  await page.goto(path);
}

const row = (page: Page, id: string) => page.locator(`[data-settings-row="${id}"]`);
const section = (page: Page, id: string) => page.locator(`[data-settings-section="${id}"]`);

test.describe("on a computer", () => {
  test.use({ viewport: { width: 1280, height: 900 } });

  test("You: each section a row with what's set, the first beside the list, and a row opens its own at its address", async ({ page }) => {
    await open(page, "/settings");
    await expect(page.locator('[data-settings-scope="you"]')).toHaveAttribute("aria-selected", "true");
    // What's set, read from what each section loads.
    await expect(row(page, "notifications").locator(".list-row-detail")).toHaveText("Mentions only · not paused");
    await expect(row(page, "quiet-hours").locator(".list-row-detail")).toHaveText("Weeknights 10 PM – 8 AM · New York");
    await expect(row(page, "linked").locator(".list-row-detail")).toHaveText("None");
    await expect(row(page, "tokens").locator(".list-row-detail")).toHaveText("1 token");
    await expect(row(page, "agents").locator(".list-row-detail")).toHaveText("No machines yet");
    // The first is shown, the rest wait; its help is under its heading, with no More to open.
    await expect(row(page, "account")).toHaveAttribute("aria-current", "true");
    await expect(section(page, "account")).toBeVisible();
    await expect(section(page, "quiet-hours")).toBeHidden();
    await expect(page.locator("details.settings-more")).toHaveCount(0);
    await row(page, "quiet-hours").click();
    await expect(page).toHaveURL(/\/settings\/you\/quiet-hours$/);
    await expect(section(page, "quiet-hours")).toBeVisible();
    await expect(section(page, "account")).toBeHidden();
    await expect(row(page, "quiet-hours")).toHaveAttribute("aria-current", "true");
    // A link opens its section, and an old one's `#section` lands on it.
    await page.goto("/settings/you/tokens");
    await expect(section(page, "tokens")).toBeVisible();
    await page.goto("/settings#notifications");
    await expect(page).toHaveURL(/\/settings\/you\/notifications$/);
    await expect(section(page, "notifications")).toBeVisible();
  });

  test("the old addresses land in their scopes, the picker chooses which one, and the scopes' switch goes between them", async ({ page }) => {
    // A project's settings, with a repo to link: Linked repos, the repo in the box.
    await open(page, `/p/${SITE}/settings?link=acme%2Fnew`);
    await expect(page).toHaveURL(new RegExp(`/settings/project/${SITE}[^/]*/repos\\?link=acme%2Fnew$`));
    await expect(page.locator(`[data-project-settings="${SITE}"]`)).toBeVisible();
    await expect(section(page, "repos")).toBeVisible();
    await expect(page.locator("[data-link-repo]")).toHaveValue("acme/new");
    await expect(row(page, "repos").locator(".list-row-detail")).toHaveText("acme/site");
    // The same section in another project.
    await page.locator('[data-settings-pick="project"]').selectOption(ME);
    await expect(page).toHaveURL(new RegExp(`/settings/project/${ME}[^/]*/repos$`));
    await expect(section(page, "repos")).toBeVisible();
    // Team and billing: the project's owner.
    await page.locator('[data-settings-scope="team"]').click();
    await expect(page).toHaveURL(/\/settings\/team\/personal$/);
    await expect(page.locator('[data-owner-settings="personal"]')).toBeVisible();
    await expect(row(page, "general").locator(".list-row-detail")).toHaveText("Default project: None");
    await page.locator('[data-settings-pick="team"]').selectOption("tm_1");
    await expect(page).toHaveURL(/\/settings\/team\/tm_1[^/]*$/);
    await expect(row(page, "members").locator(".list-row-detail")).toHaveText("2 people · 1 owner");
    // Personal's and a team's from before.
    await page.goto("/settings/personal");
    await expect(page).toHaveURL(/\/settings\/team\/personal$/);
    await page.goto("/t/tm_1/settings");
    await expect(page).toHaveURL(/\/settings\/team\/tm_1[^/]*$/);
    await expect(page.locator('[data-owner-settings="tm_1"]')).toBeVisible();
  });
});

test.describe("on a phone", () => {
  const { viewport, userAgent, deviceScaleFactor, isMobile, hasTouch } = devices["Pixel 7"];
  test.use({ viewport, userAgent, deviceScaleFactor, isMobile, hasTouch });

  test("the list alone, then a section full width with the way back", async ({ page }) => {
    await open(page, "/settings");
    await expect(row(page, "quiet-hours")).toBeVisible();
    await expect(page.locator("[data-settings-detail]")).toBeHidden();
    // Nothing is beside the list, so no row looks chosen.
    await expect(page.locator("[data-settings-row][aria-current]")).toHaveCount(0);
    await row(page, "quiet-hours").click();
    await expect(page).toHaveURL(/\/settings\/you\/quiet-hours$/);
    await expect(section(page, "quiet-hours")).toBeVisible();
    await expect(row(page, "quiet-hours")).toBeHidden();
    await page.locator("[data-settings-back]").click();
    await expect(page).toHaveURL(/\/settings$/);
    await expect(row(page, "quiet-hours")).toBeVisible();
    await expect(page.locator("[data-settings-row][aria-current]")).toHaveCount(0);
  });
});
