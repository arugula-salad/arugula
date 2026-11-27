// Settings' Team scope (Arugula #1106, Lift 7 #1487): the owner's sections
// in one flat list (the team's own, then what every project starts with), a
// kit row each with what's set, opening in place at its address; with no
// section named, every row is closed. Members lists the roster; its owners
// invite people, change roles and remove them in place, on control's word,
// and making or removing an owner is a ↗ row to control's page (aru-141).
// Machines lists what control's directory says is the owner's, one of yours
// opening its settings, and changes them in place the same way. Chat is the
// team's on or off, its owners' to switch (control's Teams panel doesn't
// have it). Labs is control's Teams panel's list of flags (Arugula #1118),
// only with `dev`. No other ↗ rows: a read that fails says so. On #971's
// fixture page (move-to-project-page.tsx): the platform's answers are this
// spec's.
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

const HUB = "https://control.test";

const json = (r: Route, body: unknown, status = 200) => r.fulfill({ status, contentType: "application/json", body: JSON.stringify(body) });

// `GET /api/flags`: yours, and each team's with whether you own it.
const kept = (dev = true) => ({
  account: { studio: true },
  teams: { tm_1: { flags: { "sandbox-providers": false }, owner: true }, tm_2: { flags: {}, owner: false } },
  dev,
});

/** The fixture page at `path`, with the platform's answers. `flags` is
 * `/api/flags`' answer, or null for a 404; `hub` adds the hub's answers
 * (else `/api/teams` is a 404). */
async function open(
  page: Page,
  path: string,
  { role = "owner", dir = true, flags = kept() as unknown, hub }: { role?: string; dir?: boolean; flags?: unknown; hub?: (page: Page) => Promise<void> } = {},
) {
  // What this spec doesn't answer, nobody does.
  await page.route(/\/api\//, (r) => json(r, { error: "not here" }, 404));
  // Control's page, as the hub says (`x-arugula-hub`).
  await page.route(/\/api\/projects$/, (r) =>
    r.fulfill({ contentType: "application/json", headers: { "x-arugula-hub": HUB }, body: JSON.stringify({ me: { account: "acct_a", login: "sam", name: "Sam" }, projects: projects(role), hosted: false }) }),
  );
  await page.route(/\/api\/(projects|owners)\/[^/]+\/settings$/, (r) => json(r, []));
  // Settings › Machines lists the machines with grants: orchard, not attic.
  await page.route(/\/api\/grants$/, (r) => json(r, { grants: [{ id: "g1", live: true, power: "threads.read", subject: { kind: "daemon", id: "dmn_orchard", name: "orchard" }, scope: { kind: "account", id: "acct_a" } }] }));
  await page.route(/\/api\/flags$/, (r) => (flags === null ? r.fulfill({ status: 404, body: "" }) : json(r, flags)));
  await page.route(/\/api\/directory$/, (r) => (dir ? json(r, { daemons: directory, offers: [] }) : json(r, { error: "control is down" }, 500)));
  await hub?.(page);
  const body = await fixturePage("./move-to-project-page.tsx");
  await page.route(/\/(p\/prj_|settings|t\/)/, (r) => (r.request().resourceType() === "document" ? r.fulfill({ contentType: "text/html", body }) : r.fallback()));
  await page.goto(path);
}

const row = (page: Page, id: string) => page.locator(`[data-settings-row="${id}"]`);
/** A row's line: a click opens it, or closes it where it's open. */
const line = (page: Page, id: string) => row(page, id).locator(":scope > button[aria-expanded]");
const section = (page: Page, id: string) => page.locator(`[data-settings-section="${id}"]`);
/** Each row's title, in the list's order. */
const titles = (page: Page) => page.locator("[data-settings-row] > button [data-row-title]");

test.use({ viewport: { width: 1280, height: 900 } });

test("a team's owner: a flat list; the roster, saying so when the hub's teams fail, the team's machines, and its Labs flags", async ({ page }) => {
  await open(page, "/settings/team/tm_1");
  // Its sections in their order, in one list: Project defaults before Labs. None open until one's asked for.
  await expect(titles(page)).toHaveText([/^Members/, /^Machines/, /^Chat/, /^Project defaults/, /^Labs/]);
  await expect(page.locator('[data-settings-row] > button[aria-expanded="true"]')).toHaveCount(0);
  await expect(section(page, "members")).toBeHidden();
  // Members: the roster, and why it can't be changed here: no ↗ to control's Teams.
  await line(page, "members").click();
  await expect(page).toHaveURL(/\/settings\/team\/tm_1[^/]*\/members$/);
  await expect(section(page, "members")).toBeVisible();
  await expect(section(page, "members").locator("[data-owner-member]")).toHaveText(["SamOwner", "JoEditor"]);
  await expect(section(page, "members")).toContainText("not here");
  await expect(section(page, "members").locator("[data-settings-control]")).toHaveCount(0);
  // Machines: the team's computers, not the Cloud workspace's box, not another team's.
  await expect(row(page, "machines")).toHaveAttribute("data-settings-summary", "2 machines · 1 online");
  await line(page, "machines").click();
  await expect(page).toHaveURL(/\/settings\/team\/tm_1[^/]*\/machines$/);
  const machines = section(page, "machines");
  await expect(machines).toBeVisible();
  await expect(section(page, "members")).toBeHidden();
  await expect(machines.locator("[data-owner-machine]")).toHaveCount(2);
  await expect(machines.locator('[data-owner-machine="dmn_kiln"]')).toContainText("Jo's");
  await expect(machines.locator('[data-owner-machine="dmn_kiln"]')).toContainText("Offline, seen 3h ago");
  // No ↗ to control's page while the hub's teams aren't read; the join command.
  await expect(machines.locator("[data-settings-control]")).toHaveCount(0);
  await expect(machines.locator("[data-join-cmd]")).toHaveText(`arugulad join ${HUB} --team tm_1`);
  // Mine opens its settings.
  await machines.locator('[data-owner-machine-open="dmn_orchard"]').click();
  await expect(page).toHaveURL(/\/settings\/machine\/dmn_orchard/);
  // Labs: the team's flags, which its owner sets here.
  await page.goto("/settings/team/tm_1/labs");
  const labs = section(page, "labs");
  await expect(row(page, "labs")).toHaveAttribute("data-settings-summary", "1 set");
  await expect(labs.locator('[data-labs-flag="sandbox-providers"] select')).toHaveValue("off");
  await expect(labs.locator('[data-labs-flag="agent-delegation"]')).toHaveCount(0);
  const sent: unknown[] = [];
  await page.route(/\/api\/flags$/, (r) =>
    r.request().method() === "POST"
      ? (sent.push(r.request().postDataJSON()), r.fulfill({ contentType: "application/json", body: JSON.stringify({ flags: { "sandbox-providers": false, studio: true } }) }))
      : r.fallback(),
  );
  await labs.locator('[data-labs-flag="studio"] select').selectOption("on");
  await expect(row(page, "labs")).toHaveAttribute("data-settings-summary", "2 set");
  expect(sent).toEqual([{ name: "studio", on: true, team: "tm_1" }]);
});

test("Labs: yours to set on Just you, a team's to read for its viewers, and none without Developer settings", async ({ page }) => {
  await open(page, "/settings/team/personal/labs");
  await expect(section(page, "labs").locator('[data-labs-flag="studio"] select')).toHaveValue("on");
  await page.goto("/settings/team/tm_2/labs");
  const theirs = section(page, "labs");
  await expect(theirs).toContainText("Bakery's owners set its own");
  await expect(theirs.locator("[data-labs-flag] select")).toHaveCount(0);
  await expect(theirs.locator('[data-labs-flag="studio"]')).toContainText("Not set");
  // Strangers never see Labs.
  await open(page, "/settings/team/tm_1", { flags: kept(false) });
  await expect(titles(page)).toHaveText([/^Members/, /^Machines/, /^Chat/, /^Project defaults/]);
});

test("Labs whose flags don't load says so, with no ↗ to control's page", async ({ page }) => {
  await open(page, "/settings/team/tm_1/labs", { flags: null });
  await expect(row(page, "labs")).toHaveAttribute("data-settings-summary", "Couldn't load");
  await expect(section(page, "labs")).toBeVisible();
  await expect(section(page, "labs")).toContainText("Request failed (404)");
  await expect(section(page, "labs").locator("[data-settings-control]")).toHaveCount(0);
});

test("Chat: a team's owner turns it on here, and a viewer reads it", async ({ page }) => {
  await open(page, "/settings/team/tm_1/team-chat");
  const chat = section(page, "team-chat");
  await expect(row(page, "team-chat")).toHaveAttribute("data-settings-summary", "Off");
  const on = chat.locator('[data-team-chat-on="tm_1"]');
  await expect(on).not.toBeChecked();
  await expect(on).toBeEnabled();
  const sent: unknown[] = [];
  await page.route(/\/api\/flags$/, (r) =>
    r.request().method() === "POST" ? (sent.push(r.request().postDataJSON()), json(r, { flags: { "sandbox-providers": false, chat: true } })) : r.fallback(),
  );
  await on.click();
  await expect(on).toBeChecked();
  await expect(row(page, "team-chat")).toHaveAttribute("data-settings-summary", "On");
  expect(sent).toEqual([{ name: "chat", on: true, team: "tm_1" }]);
  // Bakery's viewer: the switch, read-only.
  await page.goto("/settings/team/tm_2/team-chat");
  const theirs = section(page, "team-chat").locator('[data-team-chat-on="tm_2"]');
  await expect(theirs).toBeDisabled();
  await expect(section(page, "team-chat")).toContainText("Its owners turn it on and off");
});

test("Just you: your machines in no team, and no roster", async ({ page }) => {
  await open(page, "/settings/team/personal");
  await expect(row(page, "members")).toHaveCount(0);
  await expect(row(page, "machines")).toHaveAttribute("data-settings-summary", "1 machine · online");
  await line(page, "machines").click();
  await expect(page).toHaveURL(/\/settings\/team\/personal\/machines$/);
  const machines = section(page, "machines");
  await expect(machines).toBeVisible();
  // attic has no grants yet, so it opens its own page, not Machines.
  await expect(machines.locator('[data-owner-machine-open="dmn_attic"]')).toHaveAttribute("href", /dmn_attic/);
  await expect(machines.locator('[data-owner-machine-open="dmn_attic"]')).not.toHaveAttribute("href", /\/settings\/machine\//);
  await expect(machines.locator("[data-settings-control]")).toHaveCount(0);
});

test("a team's viewer reads the roster without the ↗; a directory that fails says so in its row", async ({ page }) => {
  await open(page, "/settings/team/tm_2/members", { dir: false });
  const members = section(page, "members");
  await expect(members).toBeVisible();
  await expect(members.locator("[data-settings-control]")).toHaveCount(0);
  await expect(members).toContainText("Its owners invite people and change roles");
  await expect(row(page, "machines")).toHaveAttribute("data-settings-summary", "Couldn't load");
});

/** Acme as the hub has it: Sam owns it, Jo drives, Ann watches, and Dee
 * waits on an Ask me first link. */
const acme = (version = 4) => ({
  team: "tm_1",
  pin: { team: "tm_1", founder: "acct_a", founder_root: "-" },
  locked: false,
  role: "owner",
  roster: {
    v: 1,
    team: "tm_1",
    name: "Acme",
    version,
    at: 1,
    members: [
      { account: "acct_a", root: "-", role: "owner", name: "Sam" },
      { account: "acct_b", root: "-", role: "editor", name: "Jo" },
      { account: "acct_c", root: "-", role: "viewer", name: "Ann" },
    ],
    by: "",
    sig: "",
  },
  requests: [{ account: "acct_d", root: "r_d", name: "Dee Doe", created: 1, invite: { team: "tm_1", role: "viewer", made: 1, expires: Date.now() + 3600_000, key: "k_ask", by: "", sig: "" } }],
  names: { acct_a: "Sam", acct_b: "Jo", acct_c: "Ann" },
});

test("a team's owner changes the roster in place, on control's word; owners stay control's page's", async ({ page }) => {
  const sent: { method: string; path: string; body: unknown }[] = [];
  const hub = async (p: Page) => {
    await p.route(/\/api\/teams(\/|$)/, async (r) => {
      const path = new URL(r.request().url()).pathname;
      const method = r.request().method();
      if (method === "GET" && path === "/api/teams") return json(r, { teams: [acme()] });
      if (method === "GET" && path === "/api/teams/tm_1/presigned")
        return json(r, { invites: [{ key: "k_one", role: "viewer", expires: Date.now() + 3 * 3600_000, by: "acct_b", by_name: "Jo" }] });
      sent.push({ method, path, body: r.request().postDataJSON() });
      return json(r, {});
    });
  };
  await open(page, "/settings/team/tm_1/members", { hub });
  const members = section(page, "members");
  // Sam (you) and owners: a chip. The rest: a role and Remove.
  await expect(members.locator('[data-owner-member="acct_a"]')).toContainText("Sam (you)");
  await expect(members.locator('[data-owner-member="acct_a"] select')).toHaveCount(0);
  await expect(members.locator('[data-member-role="acct_b"]')).toHaveValue("editor");
  await expect(members.locator('[data-member-role="acct_c"]')).toHaveValue("viewer");
  await expect(members.locator('[data-member-role="acct_b"] option')).toHaveText(["Editor", "Viewer"]);
  await expect(members.locator('[data-settings-control="members"]')).toHaveCount(0);
  await expect(members.locator('[data-settings-control="owners"]')).toHaveAttribute("href", `${HUB}/#teams`);

  // A role: the next version, unsigned.
  await members.locator('[data-member-role="acct_c"]').selectOption("editor");
  await expect.poll(() => sent.length).toBe(1);
  const roster = (n: number) => (sent[n].body as { roster: { version: number; by: string; sig: string; members: { account: string; role: string; name: string; root: string }[] } }).roster;
  expect(sent[0].path).toBe("/api/teams/tm_1/roster");
  expect(roster(0)).toMatchObject({ version: 5, by: "", sig: "" });
  expect(roster(0).members.find((m) => m.account === "acct_c")?.role).toBe("editor");

  // Remove asks first.
  await members.locator('[data-remove-member="acct_b"]').click();
  await expect(page.locator("[data-confirm-dialog]")).toContainText("Remove Jo from Acme?");
  await page.locator("[data-confirm-remove]").click();
  await expect.poll(() => sent.length).toBe(2);
  expect(roster(1).members.map((m) => m.account)).toEqual(["acct_a", "acct_c"]);

  // Who waits: let in with the link's role and their root, or turned down.
  await members.locator('[data-admit="acct_d"]').click();
  await expect.poll(() => sent.length).toBe(3);
  expect(roster(2).members.at(-1)).toEqual({ account: "acct_d", root: "r_d", role: "viewer", name: "Dee-Doe" });
  await members.locator('[data-reject="acct_d"]').click();
  await expect.poll(() => sent.length).toBe(4);
  expect(sent[3].path).toBe("/api/teams/tm_1/requests/acct_d/reject");

  // An invite link: control's word, opening on control's page.
  await members.locator("[data-invite-role]").selectOption("viewer");
  await members.locator("[data-invite-make]").click();
  await expect(members.locator("[data-invite-link]")).toHaveText(/^https:\/\/control\.test\/#pinvite=tm_1\.[0-9a-f]{64}$/);
  expect(sent[4]).toMatchObject({ path: "/api/teams/tm_1/invites", body: { role: "viewer", presigned: { team: "tm_1", role: "viewer", by: "", sig: "" } } });
  // Ask me first: the other kind of link.
  await members.locator("[data-invite-ask-first]").check();
  await members.locator("[data-invite-make]").click();
  await expect(members.locator("[data-invite-link]")).toHaveText(/^https:\/\/control\.test\/#askinvite=tm_1\.[0-9a-f]{64}$/);
  expect(sent[5]).toMatchObject({ path: "/api/teams/tm_1/invites", body: { role: "viewer", ask: { team: "tm_1", by: "", sig: "" } } });

  // One-click links nobody used yet, each revoked.
  await expect(members.locator('[data-presigned="k_one"]')).toContainText("For Viewer, from Jo");
  await members.locator('[data-cancel-presigned="k_one"]').click();
  await expect.poll(() => sent.length).toBe(7);
  expect(sent[6]).toMatchObject({ method: "DELETE", path: "/api/teams/tm_1/presigned/k_one" });
});

/** The hub's answers for Machines: Acme (Sam owns it) and Bakery (Sam
 * watches), every change kept in `sent`, and a move of `refuse` answered
 * with control's 409. */
function machinesHub(sent: { method: string; path: string; body: unknown }[], refuse = "") {
  return async (p: Page) => {
    await p.route(/\/api\/teams$/, (r) =>
      json(r, { teams: [acme(), { ...acme(), team: "tm_2", role: "viewer", pin: { team: "tm_2", founder: "acct_b", founder_root: "-" }, roster: { ...acme().roster, team: "tm_2", name: "Bakery" }, requests: [] }] }),
    );
    await p.route(/\/api\/teams\/tm_1\/presigned$/, (r) => json(r, { invites: [] }));
    await p.route(/\/api\/daemons\//, (r) => {
      const path = new URL(r.request().url()).pathname;
      sent.push({ method: r.request().method(), path, body: r.request().postDataJSON() });
      if (refuse && path === `/api/daemons/${refuse}/team`) return json(r, { error: "attic's Arugula is from before control's moves: update it first" }, 409);
      return json(r, {});
    });
  };
}

test("a team's Machines in place: Settings, Take out and Remove, bringing in one of yours, and Add a machine", async ({ page }) => {
  const sent: { method: string; path: string; body: unknown }[] = [];
  await open(page, "/settings/team/tm_1/machines", { hub: machinesHub(sent, "dmn_attic") });
  const machines = section(page, "machines");
  await expect(machines.locator('[data-take-out="dmn_orchard"]')).toHaveText("Take out of team");
  await expect(machines.locator('[data-owner-machine-open="dmn_orchard"]')).toHaveText("Settings");
  await expect(machines.locator('[data-owner-machine-open="dmn_orchard"]')).toHaveAttribute("href", /\/settings\/machine\/dmn_orchard/);
  // Jo's: the team's owner takes it out, never removes it.
  await expect(machines.locator('[data-remove-machine="dmn_kiln"]')).toHaveCount(0);
  await expect(machines.locator("[data-settings-control]")).toHaveCount(0);
  // The join command, for the team, on control's page.
  await expect(machines.locator("[data-join-cmd]")).toHaveText(`arugulad join ${HUB} --team tm_1`);

  // Take Jo's out: asked first, then a move to no team, unsigned.
  await machines.locator('[data-take-out="dmn_kiln"]').click();
  await expect(page.locator("[data-confirm-dialog]")).toContainText("Take kiln out of Acme?");
  await page.locator("[data-confirm-remove]").click();
  await expect.poll(() => sent.length).toBe(1);
  expect(sent[0]).toMatchObject({ method: "POST", path: "/api/daemons/dmn_kiln/team", body: { team: null, by: "", sig: "" } });

  // Remove mine: asked first.
  await machines.locator('[data-remove-machine="dmn_orchard"]').click();
  await expect(page.locator("[data-confirm-dialog]")).toContainText("Remove orchard?");
  await page.locator("[data-confirm-remove]").click();
  await expect.poll(() => sent.length).toBe(2);
  expect(sent[1]).toMatchObject({ method: "DELETE", path: "/api/daemons/dmn_orchard" });

  // Bring in attic, which is mine and in no team: a machine from before
  // control's moves says so.
  await machines.locator("[data-bring-in-pick]").selectOption("dmn_attic");
  await machines.locator("[data-bring-in-go]").click();
  await expect.poll(() => sent.length).toBe(3);
  expect(sent[2]).toMatchObject({ path: "/api/daemons/dmn_attic/team", body: { team: { team: "tm_1", founder: "acct_a", founder_root: "-" }, by: "", sig: "" } });
  await expect(machines).toContainText("attic's Arugula is from before control's moves: update it first");
});

test("Just you's Machines in place: move one into a team you may add to, and the join command", async ({ page }) => {
  const sent: { method: string; path: string; body: unknown }[] = [];
  await open(page, "/settings/team/personal/machines", { hub: machinesHub(sent) });
  const machines = section(page, "machines");
  // Acme, which Sam owns; not Bakery, which Sam only watches.
  await expect(machines.locator('[data-move-into="dmn_attic"] option')).toHaveText(["Move into a team…", "Acme"]);
  await expect(machines.locator("[data-join-cmd]")).toHaveText(`arugulad join ${HUB}`);
  await machines.locator('[data-move-into="dmn_attic"]').selectOption("tm_1");
  await expect.poll(() => sent.length).toBe(1);
  expect(sent[0]).toMatchObject({ path: "/api/daemons/dmn_attic/team", body: { team: { team: "tm_1" }, by: "", sig: "" } });
});
