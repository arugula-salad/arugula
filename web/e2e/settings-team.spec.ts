// Settings' Team and billing scope (Arugula #1106): the owner's sections in
// a flat list (the team's own, then what every project starts with). Members lists the roster,
// and inviting and roles (signed by an owner's device) is a ↗ row opening
// control's page; Machines lists what control's directory says is the
// owner's, one of yours opening its settings; Labs is a ↗ row until the app
// reaches its flags. On #971's fixture page (move-to-project-page.tsx): the
// platform's answers are this spec's.
import { expect, test, type Page, type Route } from "@playwright/test";
import { fixturePage } from "./fixture-page";

const ME = "prj_aaaa0001";
const SITE = "prj_aaaa0002";
const SHOP = "prj_aaaa0003";

const roster = [
  { account: "acct_a", name: "Sam", role: "owner" },
  { account: "acct_b", name: "Jo", role: "editor" },
];
const projects = (role: string) => [
  { id: ME, name: "Default", owner: { kind: "personal" }, team: null },
  { id: SITE, name: "Site", owner: { kind: "team", id: "tm_1", name: "Acme" }, team: { id: "tm_1", name: "Acme", role, members: roster } },
  { id: SHOP, name: "Shop", owner: { kind: "team", id: "tm_2", name: "Bakery" }, team: { id: "tm_2", name: "Bakery", role: "viewer", members: roster } },
];

// Mine, a team's; Jo's, the same team's; mine, no team's; a Cloud workspace's box; Jo's, another team's.
const directory = [
  { id: "dmn_orchard", name: "orchard", urls: [], online: true, last_seen: Date.now(), team: "tm_1" },
  { id: "dmn_kiln", name: "kiln", urls: [], online: false, last_seen: Date.now() - 3 * 3600_000, team: "tm_1", account: "acct_b", owner_name: "Jo" },
  { id: "dmn_attic", name: "attic", urls: [], online: true, last_seen: Date.now(), team: null },
  { id: "dmn_box", name: "box", urls: [], online: true, last_seen: Date.now(), team: "tm_1", hosted: { cert: {} } },
  { id: "dmn_mill", name: "mill", urls: [], online: true, last_seen: Date.now(), team: "tm_2", account: "acct_b", owner_name: "Jo" },
];

/** The fixture page at `path`, with the platform's answers. */
async function open(page: Page, path: string, { role = "owner", dir = true }: { role?: string; dir?: boolean } = {}) {
  const json = (r: Route, body: unknown, status = 200) => r.fulfill({ status, contentType: "application/json", body: JSON.stringify(body) });
  // What this spec doesn't answer, nobody does.
  await page.route(/\/api\//, (r) => json(r, { error: "not here" }, 404));
  await page.route(/\/api\/projects$/, (r) => json(r, { me: { account: "acct_a", login: "sam", name: "Sam" }, projects: projects(role), hosted: false }));
  await page.route(/\/api\/(projects|owners)\/[^/]+\/settings$/, (r) => json(r, []));
  // Settings › Machines lists the machines with grants: orchard, not attic.
  await page.route(/\/api\/grants$/, (r) => json(r, { grants: [{ id: "g1", live: true, power: "threads.read", subject: { kind: "daemon", id: "dmn_orchard", name: "orchard" }, scope: { kind: "account", id: "acct_a" } }] }));
  await page.route(/\/api\/directory$/, (r) => (dir ? json(r, { daemons: directory, offers: [] }) : json(r, { error: "control is down" }, 500)));
  const body = await fixturePage("./move-to-project-page.tsx");
  await page.route(/\/(p\/prj_|settings|t\/)/, (r) => (r.request().resourceType() === "document" ? r.fulfill({ contentType: "text/html", body }) : r.fallback()));
  await page.goto(path);
}

const row = (page: Page, id: string) => page.locator(`[data-settings-row="${id}"]`);
const section = (page: Page, id: string) => page.locator(`[data-settings-section="${id}"]`);

test.use({ viewport: { width: 1280, height: 900 } });

test("a team's owner: a flat list, the roster with a ↗ to change it, the team's machines, and Labs on control's page", async ({ page }) => {
  await open(page, "/settings/team/tm_1");
  // The team's own, then what every project starts with, under no band headings.
  await expect(page.locator(".settings-list .list-group-title")).toHaveCount(0);
  await expect(page.locator("[data-settings-row]")).toHaveText([/^Members/, /^Machines/, /^Labs/, /^Default project/, /^Project defaults/]);
  // Members, first: the roster, and what a device signs opens control's Teams.
  await expect(section(page, "members")).toBeVisible();
  await expect(section(page, "members").locator("[data-owner-member]")).toHaveText(["SamOwner", "JoEditor"]);
  await expect(section(page, "members").locator('[data-settings-control="members"]')).toHaveAttribute("href", /\/#teams$/);
  // Machines: the team's computers, not the Cloud workspace's box, not another team's.
  await expect(row(page, "machines").locator(".list-row-detail")).toHaveText("2 machines · 1 online");
  await row(page, "machines").click();
  await expect(page).toHaveURL(/\/settings\/team\/tm_1[^/]*\/machines$/);
  const machines = section(page, "machines");
  await expect(machines.locator("[data-owner-machine]")).toHaveCount(2);
  await expect(machines.locator('[data-owner-machine="dmn_kiln"]')).toContainText("Jo's");
  await expect(machines.locator('[data-owner-machine="dmn_kiln"]')).toContainText("Offline, seen 3h ago");
  await expect(machines.locator('[data-settings-control="machines"]')).toContainText("Share a machine with Acme, or take one out");
  // Mine opens its settings.
  await machines.locator('[data-owner-machine-open="dmn_orchard"]').click();
  await expect(page).toHaveURL(/\/settings\/machine\/dmn_orchard/);
  // Labs: control's page, said plainly.
  await page.goto("/settings/team/tm_1/labs");
  await expect(section(page, "labs").locator('[data-settings-control="labs"]')).toHaveAttribute("href", /\/#teams$/);
  await expect(section(page, "labs")).toContainText("Labs for Acme's machines");
});

test("Just you: your machines in no team, a way to move one into a team, and no roster", async ({ page }) => {
  await open(page, "/settings/team/personal");
  await expect(page.locator(".settings-list .list-group-title")).toHaveCount(0);
  await expect(row(page, "members")).toHaveCount(0);
  await expect(row(page, "machines").locator(".list-row-detail")).toHaveText("1 machine · online");
  const machines = section(page, "machines");
  await expect(machines).toBeVisible();
  // attic has no grants yet, so it opens its own page, not Machines.
  await expect(machines.locator('[data-owner-machine-open="dmn_attic"]')).toHaveAttribute("href", /dmn_attic/);
  await expect(machines.locator('[data-owner-machine-open="dmn_attic"]')).not.toHaveAttribute("href", /\/settings\/machine\//);
  await expect(machines.locator('[data-settings-control="machines"]')).toContainText("Move a machine into a team");
});

test("a team's viewer reads the roster without the ↗; a directory that fails says so in its row", async ({ page }) => {
  await open(page, "/settings/team/tm_2/members", { dir: false });
  const members = section(page, "members");
  await expect(members).toBeVisible();
  await expect(members.locator("[data-settings-control]")).toHaveCount(0);
  await expect(members).toContainText("Its owners invite people and change roles");
  await expect(row(page, "machines").locator(".list-row-detail")).toHaveText("Couldn't load");
});
