// Settings (Arugula #1104, Lift 7 #1487): one page, three scopes in its
// tabs (You, Project, Team; a machine's settings are Machines', with no tab),
// each one list of its sections, a row each with what's set, opening in
// place. The section is the address: a link opens its row, a click on a
// closed row goes to its address and opens it, and on the open one goes back
// to the scope's and closes it; with no section named, every row is closed.
// The old addresses (a project's settings, Personal's, a team's, `#section`)
// land in their scope. On a phone it's the same list, opening in place. The
// Project scope's sections are six rows in a flat list (Arugula #1105,
// #1323), with CI alerts, Runs and workspaces and Claude spend parts of
// Agents, and an old section's address lands on the row it's part of. You is
// four sections (Arugula #1320): Notifications, Preferences, Sign-in and
// security, Connected apps and agents; how it looks is Look's (`s`), so
// Appearance's address lands on Preferences. What's on control's page until
// the single origin (aru-141) is a ↗ row to it there, and Help is the
// palette's. Sign-in and security is control's dialog drawn in place
// (Arugula #1311), adding or removing a passkey a ↗ row; what every device
// is pushed about is set in place (Arugula #1310), this device's push a ↗
// row, and your devices' list (Arugula #1327) is `/api/push/subscriptions`.
// A read that fails says so, with no ↗ row in its place. On #971's fixture
// page (move-to-project-page.tsx): the platform's answers are this spec's.
import { devices, expect, test, type Page, type Route } from "@playwright/test";
import { fixturePage } from "./fixture-page";

const ME = "prj_aaaa0001";
const SITE = "prj_aaaa0002";

const projects = [
  { id: ME, name: "Default", owner: { kind: "personal" }, team: null },
  {
    id: SITE,
    name: "Site",
    owner: { kind: "team", id: "tm_1", name: "Acme" },
    team: { id: "tm_1", name: "Acme", role: "owner", members: [{ account: "acct_a", name: "Sam", role: "owner" }, { account: "acct_b", name: "Jo", role: "editor" }] },
  },
];

const json = (r: Route, body: unknown, status = 200) => r.fulfill({ status, contentType: "application/json", body: JSON.stringify(body) });

/** The fixture page at `path`, with the platform's answers, and `more` of a test's own. */
async function open(page: Page, path: string, more?: () => Promise<unknown>) {
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
  await page.route(/\/api\/projects\/[^/]+\/trackers$/, (r) => json(r, { project: SITE, connections: [] }));
  await more?.();
  const body = await fixturePage("./move-to-project-page.tsx");
  await page.route(/\/(p\/prj_|settings|t\/)/, (r) => (r.request().resourceType() === "document" ? r.fulfill({ contentType: "text/html", body }) : r.fallback()));
  await page.goto(path);
}

const row = (page: Page, id: string) => page.locator(`[data-settings-row="${id}"]`);
/** A row's line: a click opens it, or closes it where it's open. */
const line = (page: Page, id: string) => row(page, id).locator(":scope > button[aria-expanded]");
const section = (page: Page, id: string) => page.locator(`[data-settings-section="${id}"]`);
const tab = (page: Page, id: string) => page.locator(`[data-settings-scopes] [data-tab="${id}"]`);
const rowIds = (page: Page) => page.locator("[data-settings-row]").evaluateAll((els) => els.map((e) => (e as HTMLElement).dataset.settingsRow));

test.describe("on a computer", () => {
  test.use({ viewport: { width: 1280, height: 900 } });

  test("You: each section a row with what's set, none open until the address names one, and a row's line opens it there and closes it", async ({ page }) => {
    await open(page, "/settings", () => page.route(/\/api\/push\/prefs$/, (r) => json(r, { chat: true, machines: true, projects: false })));
    await expect(page.locator("h1")).toHaveText("Settings");
    await expect(tab(page, "you")).toHaveAttribute("aria-selected", "true");
    await expect(page.locator("[data-settings-scopes] [data-tab]")).toHaveText(["You", "Project", "Team"]);
    // What's set, read from what each section's parts load, in their order.
    await expect(row(page, "notifications")).toHaveAttribute("data-settings-summary", "Pushed: Chat, Machines · Mentions only · not paused · Weeknights 10 PM – 8 AM · New York");
    await expect(row(page, "connected")).toHaveAttribute("data-settings-summary", "None · 1 token");
    // Your machines' agents are each machine's, under Machines (Arugula #1107).
    await expect(row(page, "agents")).toHaveCount(0);
    // No section in the address: every row closed.
    await expect(page.locator('[data-settings-row] > button[aria-expanded="true"]')).toHaveCount(0);
    await expect(section(page, "notifications")).toBeHidden();
    await expect(section(page, "connected")).toBeHidden();
    // A row's line opens it at its address; its help is under the line.
    await line(page, "notifications").click();
    await expect(page).toHaveURL(/\/settings\/you\/notifications$/);
    await expect(line(page, "notifications")).toHaveAttribute("aria-expanded", "true");
    await expect(section(page, "notifications")).toBeVisible();
    // Notifications: this device, what's pushed, how loud chat is, quiet hours (words that notify you, once chat is on).
    await expect(section(page, "notifications").locator("[data-settings-part]")).toHaveCount(4);
    expect(await section(page, "notifications").locator("[data-settings-part]").evaluateAll((els) => els.map((e) => e.id))).toEqual(["this-device", "push", "chat-notify", "quiet-hours"]);
    // Another's line opens that one, and the first closes.
    await line(page, "connected").click();
    await expect(page).toHaveURL(/\/settings\/you\/connected$/);
    await expect(section(page, "connected")).toBeVisible();
    await expect(section(page, "notifications")).toBeHidden();
    await expect(line(page, "connected")).toHaveAttribute("aria-expanded", "true");
    await expect(line(page, "notifications")).toHaveAttribute("aria-expanded", "false");
    await expect(row(page, "connected").locator("[data-row-title]")).toHaveText("Connected apps and agents");
    expect(await section(page, "connected").locator("[data-settings-part]").evaluateAll((els) => els.map((e) => e.id))).toEqual(["linked", "tokens"]);
    // The open one's line closes it, back at the scope's address.
    await line(page, "connected").click();
    await expect(page).toHaveURL(/\/settings$/);
    await expect(section(page, "connected")).toBeHidden();
    await expect(page.locator('[data-settings-row] > button[aria-expanded="true"]')).toHaveCount(0);
    // A link opens its section, and an old one's address, by path or `#section`, lands on the one that holds it now.
    await page.goto("/settings/you/tokens");
    await expect(page).toHaveURL(/\/settings\/you\/connected$/);
    await expect(section(page, "connected").locator('[data-settings-part="tokens"]')).toBeVisible();
    await page.goto("/settings#quiet-hours");
    await expect(page).toHaveURL(/\/settings\/you\/notifications$/);
    await expect(section(page, "notifications").locator('[data-settings-part="quiet-hours"]')).toBeVisible();
    // The old Agents on your machines' address opens a machine's settings (Arugula #1107): no tab of their own.
    await page.goto("/settings#agents");
    await expect(page).toHaveURL(/\/settings\/machine/);
    await expect(page.locator("h1")).toHaveText("Machine settings");
    await expect(page.locator("[data-settings-scopes]")).toHaveCount(0);
  });

  test("You: control's device and preference rows join it in one flat list, only what control's origin keeps is a ↗ row, and there's no second index or Help", async ({ page }) => {
    // A hub that fails push's prefs.
    await open(page, "/settings", () => page.route(/\/api\/push\/prefs$/, (r) => r.fulfill({ status: 404, body: "" })));
    await expect.poll(() => rowIds(page)).toEqual(["notifications", "preferences", "security", "connected"]);
    // How it looks is Look's: no Appearance here, nor a way to its dialog.
    await expect(page.locator("[data-appearance-controls]")).toHaveCount(0);
    await expect(page.getByRole("button", { name: "Appearance…" })).toHaveCount(0);
    // No index of control's sections, and no Help.
    await expect(section(page, "elsewhere")).toHaveCount(0);
    await expect(page.locator("[data-settings-elsewhere]")).toHaveCount(0);
    await expect(page.getByText("Getting started")).toHaveCount(0);
    await expect(page.getByText("What's new")).toHaveCount(0);

    // What's on control's page until the single origin: a ↗ row each, at
    // its dialog there. No Add a phone or browser (#1004), no Machines or
    // Teams (Team has them), and no Shared Docs (Docs has them). Machines
    // and teams' old address lands on Sign-in and security, whose one ↗ row
    // is adding or removing a passkey.
    const origin = new URL(page.url()).origin;
    await expect(page.locator('[data-settings-control="teams"]')).toHaveCount(0);
    await page.goto("/settings/you/machines-teams");
    await expect(page).toHaveURL(/\/settings\/you\/security$/);
    const out = section(page, "security").locator("[data-settings-control]");
    await expect(out).toHaveText([/^Add or remove a passkey/]);
    await expect(out).toHaveAttribute("href", `${origin}/#account`);
    // Notifications, when the hub fails push's prefs: it says so, and this
    // device's ↗ row to control's page is the only one.
    await line(page, "notifications").click();
    const push = section(page, "notifications").locator('[data-settings-control="push"]');
    await expect(push).toHaveCount(1);
    await expect(push).toHaveAttribute("href", `${origin}/#settings=push`);
    await expect(page.locator('[data-settings-part="push"]')).toContainText("Request failed (404)");
  });

  test("You: Sign-in and security draws control's dialog in place; adding or removing a passkey and linking GitHub open control's page (Arugula #1311)", async ({ page }) => {
    const posted: string[] = [];
    const DAY = 86_400_000;
    await open(page, "/settings/you/account", async () => {
      await page.route(/\/api\/me\/sessions$/, (r) =>
        json(r, {
          sessions: [
            { id: "s_here", created: Date.now() - DAY, expires: Date.now() + DAY, agent: "Mozilla/5.0 (Macintosh) AppleWebKit Chrome/130 Safari/537", current: true },
            { id: "s_phone", created: Date.now() - 2 * DAY, expires: Date.now() + DAY, agent: "Mozilla/5.0 (iPhone) AppleWebKit Version/18 Mobile Safari/604", current: false },
            { id: "s_old", created: Date.now() - 9 * DAY, expires: Date.now() + DAY, agent: "", current: false },
          ],
        }),
      );
      await page.route(/\/api\/me\/passkeys$/, (r) =>
        json(r, { passkeys: [{ id: "pk_1", created: Date.now() - 5 * DAY, agent: "", provider: "iCloud Keychain", used: null, prf: null, device: null }], github: "sam", ways: 2 }),
      );
      await page.route(/\/api\/me\/email$/, (r) => json(r, { email: "sam@example.test", source: "github", verified: true, pending: null, sends: true }));
      await page.route(/\/api\/me\/(sessions\/[^/]+\/end|github\/unlink)$/, (r) => {
        posted.push(new URL(r.request().url()).pathname);
        return json(r, {});
      });
      await page.route(/\/api\/me\/delete$/, (r) =>
        r.request().method() === "GET" ? json(r, { confirm: "sam", disband: [], blockers: [], pays: [], machines: 2, stays_in: [] }) : json(r, {}),
      );
    });
    // Sign-in and machines' old address lands here.
    await expect(page).toHaveURL(/\/settings\/you\/security$/);
    const sec = section(page, "security");
    await expect(line(page, "security")).toHaveAttribute("aria-expanded", "true");
    await expect(row(page, "security").locator("[data-row-title]")).toHaveText("Sign-in and security");
    await expect(row(page, "security")).toHaveAttribute("data-settings-summary", "1 passkey · GitHub linked · 3 sign-ins");
    // Passkeys: a list, with nothing to remove here; adding or removing one is on control's page, at the top.
    const origin = new URL(page.url()).origin;
    await expect(sec.locator("[data-passkey-name]")).toHaveText(["iCloud Keychain"]);
    await expect(sec.locator("[data-remove-passkey], [data-add-passkey]")).toHaveCount(0);
    await expect(sec.locator('[data-settings-control="passkeys"]')).toHaveAttribute("href", `${origin}/#account`);
    // GitHub and email.
    await expect(sec.locator("[data-github-login]")).toHaveText("GitHub: sam");
    await expect(sec.locator("[data-email-address]")).toHaveText("sam@example.test");
    // Where you're signed in: this browser, the phone; the earlier one in the count.
    await expect(sec.locator("[data-session]")).toHaveText([/^Chrome on Mac \(this browser\)/, /^Safari on iPhone/]);
    await expect(sec.getByText("2 other sessions, 1 of them from before Arugula kept which browser.")).toBeVisible();
    await expect(sec.locator('[data-end-session="s_here"]')).toHaveCount(0);
    await sec.locator('[data-end-session="s_phone"]').click();
    await expect(sec.locator('[data-end-session="s_phone"]')).toHaveText("Really sign out?");
    await sec.locator('[data-end-session="s_phone"]').click();
    await expect.poll(() => posted).toEqual(["/api/me/sessions/s_phone/end"]);
    // Unlink GitHub asks first.
    await sec.locator("[data-unlink-github]").click();
    await expect(page.locator("[data-confirm-remove]")).toBeEnabled();
    await page.locator("[data-confirm-remove]").click();
    await expect.poll(() => posted).toEqual(["/api/me/sessions/s_phone/end", "/api/me/github/unlink"]);
    // Deleting: what goes, then the login typed.
    await sec.locator("[data-delete-account]").click();
    await expect(sec.locator("[data-delete-what]")).toContainText("your 2 machines");
    await expect(sec.locator("[data-delete-go]")).toBeDisabled();
    await sec.locator("[data-delete-confirm]").fill("sam");
    await expect(sec.locator("[data-delete-go]")).toBeEnabled();
    // The passkey row is its only ↗: no Shared Docs (Docs has them).
    await expect(sec.locator("[data-settings-control]")).toHaveCount(1);
  });

  test("You: a hub that fails /api/me/* says so in Sign-in and security, with no ↗ row to control's dialog in its place", async ({ page }) => {
    await open(page, "/settings/you/security", () => page.route(/\/api\/me\//, (r) => r.fulfill({ status: 404, body: "" })));
    const sec = section(page, "security");
    await expect(row(page, "security")).toHaveAttribute("data-settings-summary", "Couldn't load");
    await expect(sec).toContainText("404");
    await expect(sec.locator("[data-settings-control]")).toHaveText([/^Add or remove a passkey/]);
  });

  test("You: Notifications sets what every device is pushed about in place; this device's switch and the test stay on control's page (Arugula #1310)", async ({ page }) => {
    const sent: unknown[] = [];
    let prefs = { chat: true, machines: true, projects: false };
    await open(page, "/settings/you/push", () =>
      page.route(/\/api\/push\/prefs$/, (r) => {
        if (r.request().method() === "POST") {
          const body = r.request().postDataJSON() as Partial<typeof prefs>;
          sent.push(body);
          prefs = { ...prefs, ...body };
        }
        return json(r, prefs);
      }),
    );
    // Phones and browsers' old address lands on Notifications, which holds it now.
    await expect(page).toHaveURL(/\/settings\/you\/notifications$/);
    const push = page.locator('[data-settings-part="push"]');
    const detail = row(page, "notifications");
    await expect(detail).toHaveAttribute("data-settings-summary", /^Pushed: Chat, Machines · /);
    const kinds = push.locator("[data-push-kind]");
    await expect(kinds).toHaveCount(3);
    await expect(push.locator('[data-push-kind="chat"]')).toBeChecked();
    await expect(push.locator('[data-push-kind="projects"]')).not.toBeChecked();
    await push.locator('[data-push-kind="projects"]').click();
    await expect(push.locator('[data-push-kind="projects"]')).toBeChecked();
    await expect(detail).toHaveAttribute("data-settings-summary", /^Pushed: Everything · /);
    expect(sent).toEqual([{ projects: true }]);
    // This device, first: one ↗ row to control's push settings.
    await expect(push.locator("[data-settings-control]")).toHaveCount(0);
    const out = page.locator('[data-settings-part="this-device"] [data-settings-control]');
    await expect(out).toHaveText([/^Notify this device/]);
    await expect(out).toHaveAttribute("href", `${new URL(page.url()).origin}/#settings=push`);
  });

  test("You: Notifications lists your devices under This device, each with a switch and Remove, and none while the hub answers 404 for them (Arugula #1327)", async ({ page }) => {
    const sent: { method: string; path: string; body: unknown }[] = [];
    const HOUR = 3_600_000;
    let rows = [
      { id: "sub_mac", label: "Chrome on Mac", kind: "browser", created: Date.now() - 48 * HOUR, used: Date.now() - HOUR, on: true },
      { id: "sub_phone", label: "Sam's iPhone", kind: "iphone", created: Date.now() - 72 * HOUR, used: null, on: true },
    ];
    await open(page, "/settings/you/notifications", () =>
      page.route(/\/api\/push\/subscriptions(\/[^/]+)?$/, (r) => {
        const req = r.request();
        const path = new URL(req.url()).pathname;
        const id = path.split("/")[4];
        if (req.method() === "GET") return json(r, { subscriptions: rows });
        sent.push({ method: req.method(), path, body: req.postDataJSON() });
        if (req.method() === "POST") {
          rows = rows.map((x) => (x.id === id ? { ...x, on: (req.postDataJSON() as { on: boolean }).on } : x));
          return json(r, rows.find((x) => x.id === id));
        }
        rows = rows.filter((x) => x.id !== id);
        return json(r, {});
      }),
    );
    const here = page.locator('[data-settings-part="this-device"]');
    const list = here.locator("[data-push-devices]");
    // The list, last reached first: its name, kind and when, and what off and Remove mean.
    await expect(list.locator("h3")).toHaveText("Your devices");
    await expect(list.locator("[data-push-device-name]")).toHaveText(["Chrome on Mac", "Sam's iPhone"]);
    await expect(list.locator('[data-push-device="sub_mac"]')).toContainText(/Browser, last used /);
    await expect(list.locator('[data-push-device="sub_phone"]')).toContainText(/iPhone app, added /);
    await expect(list).toContainText("Off: nothing is sent to that device, and it stays off if it turns notifications on again.");
    // The switch: off, then the row as the hub says it.
    const phone = list.locator('[data-push-device-on="sub_phone"]');
    await expect(phone).toBeChecked();
    await phone.click();
    await expect(phone).not.toBeChecked();
    expect(sent).toEqual([{ method: "POST", path: "/api/push/subscriptions/sub_phone", body: { on: false } }]);
    // Remove asks first; Cancel keeps it.
    await list.locator('[data-push-device-remove="sub_mac"]').click();
    await page.getByRole("button", { name: "Cancel" }).click();
    await expect(list.locator('[data-push-device="sub_mac"]')).toBeVisible();
    expect(sent).toHaveLength(1);
    await list.locator('[data-push-device-remove="sub_mac"]').click();
    await expect(page.locator("[data-confirm-remove]")).toBeEnabled();
    await page.locator("[data-confirm-remove]").click();
    await expect(list.locator("[data-push-device-name]")).toHaveText(["Sam's iPhone"]);
    expect(sent[1]).toEqual({ method: "DELETE", path: "/api/push/subscriptions/sub_mac", body: null });

    // A hub that answers 404 for them: no list, This device's ↗ row as before.
    await page.unrouteAll({ behavior: "wait" });
    await open(page, "/settings/you/notifications");
    await expect(page.locator('[data-settings-part="this-device"] [data-settings-control="push"]')).toBeVisible();
    await expect(page.locator("[data-push-devices]")).toHaveCount(0);
    await expect(page.locator('[data-settings-part="this-device"]')).not.toContainText("Request failed");
  });

  test("Status: on hosted Arugula your picture's menu links status.arugula.io in a new tab, and Settings doesn't; on a control you run there's none (Arugula #1176, #1320)", async ({ page, context }) => {
    // Nothing leaves for arugula.io: the status page is this spec's.
    await context.route(/^https:\/\/[^/]*arugula\.io\//, (r) => r.fulfill({ contentType: "text/html", body: "<title>Status</title>" }));
    const menu = async () => {
      await page.locator("[data-rail] [data-signed-in-as]").click();
      return page.getByRole("menuitem");
    };
    // A control you run yourself (this page's own hub): no Status anywhere.
    await open(page, "/settings/you/security");
    await expect(section(page, "security")).toBeVisible();
    await expect(row(page, "status-page")).toHaveCount(0);
    await expect(page.locator("[data-status-page]")).toHaveCount(0);
    // Each scope of Settings, then the rest (Lift 7).
    await expect(await menu()).toHaveText(["Your settings", "Project settings", "Team settings", "Send feedback…", "Sign out of this page"]);
    await page.keyboard.press("Escape");

    // Hosted Arugula: control says it's the hub on every answer on the app's host.
    await page.unrouteAll({ behavior: "wait" });
    await open(page, "/settings/you/status-page", () =>
      page.route(/\/api\/projects$/, (r) =>
        r.fulfill({ contentType: "application/json", headers: { "x-arugula-hub": "https://control.arugula.io" }, body: JSON.stringify({ me: { account: "acct_a", login: "sam", name: "Sam" }, projects, hosted: false }) }),
      ),
    );
    // Settings › You has no Status: its old address is You's, with no row open.
    await expect(page).toHaveURL(/\/settings$/);
    await expect.poll(() => rowIds(page)).toEqual(["notifications", "preferences", "security", "connected"]);
    await expect(page.locator('[data-settings-row] > button[aria-expanded="true"]')).toHaveCount(0);
    await expect(page.locator("[data-status-page]")).toHaveCount(0);
    // The menu: Status after Settings' scopes, opening a tab that can't reach back into this one.
    const items = await menu();
    await expect(items).toHaveText(["Your settings", "Project settings", "Team settings", /^Status/, "Send feedback…", "Sign out of this page"]);
    const tab = context.waitForEvent("page");
    await page.getByRole("menuitem").and(page.locator("[data-status-page]")).click();
    const status = await tab;
    await expect(status).toHaveURL("https://status.arugula.io/");
    expect(await status.evaluate(() => window.opener)).toBeNull();
    await expect(page).not.toHaveURL(/status-page/);
  });

  test("You: Preferences draws its controls in place, its summary says what's set, and Appearance's old address lands on it", async ({ page }) => {
    // How it looks is Look's (`s`): Appearance's old address, by path or `#section`, is Preferences'.
    await open(page, "/settings/you/appearance");
    await expect(page).toHaveURL(/\/settings\/you\/preferences$/);
    await page.goto("/settings#appearance");
    await expect(page).toHaveURL(/\/settings\/you\/preferences$/);
    await page.goto("/settings/you/preferences");
    await expect(page).toHaveURL(/\/settings\/you\/preferences$/);
    const prefs = section(page, "preferences");
    await expect(prefs).toBeVisible();
    await expect(prefs).toHaveAttribute("data-preferences", "");
    await expect(row(page, "preferences").locator("[data-row-title]")).toHaveText("Preferences");
    await expect(page.locator("[data-appearance-controls]")).toHaveCount(0);
    const said = row(page, "preferences");
    await expect(said).toHaveAttribute("data-settings-summary", "Tips on · opens in VS Code");
    const tips = prefs.getByRole("switch", { name: "Tips", exact: true });
    await expect(tips).toBeChecked();
    await tips.click();
    await expect(tips).not.toBeChecked();
    await prefs.locator('[data-pref="editor"]').selectOption("nvim");
    await expect(said).toHaveAttribute("data-settings-summary", "Tips off · opens in nvim");
    await tips.click();
    await prefs.getByRole("switch", { name: "Big tips" }).click();
    await prefs.locator('[data-pref="editor"]').selectOption("ask");
    await expect(said).toHaveAttribute("data-settings-summary", "Tips on, not big ones · asks which editor");
    // Lending this device to agents, this browser's alone.
    const lend = prefs.locator("[data-lend-device]");
    await expect(lend).toHaveAccessibleName("Lend this device to agents");
    await expect(lend).not.toBeChecked();
    await lend.click();
    await expect(lend).toBeChecked();
    await expect(said).toHaveAttribute("data-settings-summary", "Tips on, not big ones · asks which editor · lent to agents");
    expect(await page.evaluate(() => localStorage.getItem("arugula-hand"))).toBe("on");
    await lend.click();
    await expect(said).toHaveAttribute("data-settings-summary", "Tips on, not big ones · asks which editor");
    expect(await page.evaluate(() => localStorage.getItem("arugula-hand"))).toBe("off");
  });

  test("the old addresses land in their scopes, the picker chooses which one, and the scopes' switch goes between them", async ({ page }) => {
    // A project's settings, with a repo to link: Linked repos, the repo in the box.
    await open(page, `/p/${SITE}/settings?link=acme%2Fnew`);
    await expect(page).toHaveURL(new RegExp(`/settings/project/${SITE}[^/]*/repos\\?link=acme%2Fnew$`));
    await expect(page.locator(`[data-project-settings="${SITE}"]`)).toBeVisible();
    await expect(section(page, "repos")).toBeVisible();
    await expect(page.locator("[data-link-repo]")).toHaveValue("acme/new");
    await expect(row(page, "repos")).toHaveAttribute("data-settings-summary", "acme/site");
    // The same section in another project.
    await page.locator('[data-settings-pick="project"]').selectOption(ME);
    await expect(page).toHaveURL(new RegExp(`/settings/project/${ME}[^/]*/repos$`));
    await expect(section(page, "repos")).toBeVisible();
    // Team: the project's owner.
    await expect(tab(page, "project")).toHaveAttribute("aria-selected", "true");
    await tab(page, "team").click();
    await expect(page).toHaveURL(/\/settings\/team\/personal$/);
    await expect(page.locator('[data-owner-settings="personal"]')).toBeVisible();
    await expect(tab(page, "team")).toHaveAttribute("aria-selected", "true");
    // The default project is the top of Project defaults, and General's address lands there.
    await expect(row(page, "defaults")).toHaveAttribute("data-settings-summary", /^Default project: None · /);
    await expect(row(page, "general")).toHaveCount(0);
    await page.goto("/settings/team/personal/general");
    await expect(page).toHaveURL(/\/settings\/team\/personal\/defaults$/);
    await expect(section(page, "defaults").locator("[data-default-project-form]")).toBeVisible();
    await page.goto("/settings/team/personal");
    await page.locator('[data-settings-pick="team"]').selectOption("tm_1");
    await expect(page).toHaveURL(/\/settings\/team\/tm_1[^/]*$/);
    await expect(row(page, "members")).toHaveAttribute("data-settings-summary", "2 people · 1 owner");
    // Personal's and a team's from before.
    await page.goto("/settings/personal");
    await expect(page).toHaveURL(/\/settings\/team\/personal$/);
    await page.goto("/t/tm_1/settings");
    await expect(page).toHaveURL(/\/settings\/team\/tm_1[^/]*$/);
    await expect(page.locator('[data-owner-settings="tm_1"]')).toBeVisible();
    // The Team page went: its Members lands on the team's Members here.
    await page.goto("/t/tm_1/members");
    await expect(page).toHaveURL(/\/settings\/team\/tm_1[^/]*\/members$/);
    await expect(section(page, "members")).toBeVisible();
  });
});

test.describe("a project's settings", () => {
  test.use({ viewport: { width: 1280, height: 900 } });

  /** A project with an environment, a secret, an agent token and a model. */
  const project = (page: Page) =>
    Promise.all([
      page.route(/\/api\/projects\/[^/]+\/environments$/, (r) => json(r, { environments: [] })),
      page.route(/\/api\/projects\/[^/]+\/drivers$/, (r) => json(r, { drivers: [] })),
      page.route(/\/api\/projects\/[^/]+\/secrets$/, (r) => json(r, { secrets: [{ name: "API_KEY", updated_at: 1, updated_by: null, agent: null }], max: 20 })),
      page.route(/\/api\/projects\/[^/]+\/agent-tokens$/, (r) =>
        json(r, { tokens: [{ id: "tok_2", name: "deploy-bot", created_at: 1, last_used_at: null, revoked_at: null, live: true, grants: [], owner: { account: "acct_b", name: "Jo" } }] }),
      ),
      page.route(/\/api\/projects\/[^/]+\/sessions\/settings$/, (r) => json(r, { model: null, models: ["opus", "sonnet"] })),
      page.route(/\/api\/projects\/[^/]+\/runs\/grant$/, (r) => json(r, { role: "owner", grant: null, past: [], stale: false, renew: false, workspaces: 0, offer: false })),
      page.route(/\/api\/projects\/[^/]+\/run-specs$/, (r) => json(r, { project: SITE, specs: [{ id: "rsp_0001", project: SITE, name: "nightly", workspace: "ws_1", kind: "command", argv: ["npm", "test"], schedules: [], rules: [] }] })),
      // One hosted workspace, so its size, finish step and agents' workspace have something to set.
      page.route(/\/api\/projects\/[^/]+\/workspaces$/, (r) => json(r, { workspaces: [{ id: "ws_1", name: "site-box", state: "ready", spec: {}, rebuild_needed: false }] })),
    ]);

  test("the rows in a flat list, a merged row saying its parts' in a line, each part under its own heading", async ({ page }) => {
    await open(page, `/settings/project/${SITE}`, () => project(page));
    await expect(row(page, "agents")).toBeVisible();
    // Apps from products shows once a product keeps one here.
    await expect.poll(() => rowIds(page)).toEqual(["general", "access", "repos", "issues", "agents", "runs", "secrets"]);
    await expect(row(page, "repos")).toHaveAttribute("data-settings-summary", "acme/site");
    await expect(row(page, "agents")).toHaveAttribute("data-settings-summary", /^The strongest \(Opus\) · .* · 1 token$/);
    // Runs is a row of its own (Lift 6a, #1483): its specs, with what the Runs list had, and its standing grant.
    await expect(row(page, "runs")).toHaveAttribute("data-settings-summary", /^1 spec · .*No standing grant/);
    expect(await section(page, "runs").locator("[data-settings-part]").evaluateAll((els) => els.map((e) => e.id))).toEqual(["run-specs", "run-grant"]);
    await expect(row(page, "secrets")).toHaveAttribute("data-settings-summary", "No environments · 1 secret");
    // Agents holds what were CI alerts, workspaces and Claude spend, as parts.
    expect(await section(page, "agents").locator("[data-settings-part]").evaluateAll((els) => els.map((e) => e.id))).toEqual([
      "agent-model",
      "agent-workspace",
      "size",
      "finish",
      "ci-alerts",
      "spend",
      "agent-tokens",
    ]);
    await line(page, "secrets").click();
    await expect(page).toHaveURL(new RegExp(`/settings/project/${SITE}[^/]*/secrets$`));
    const secrets = section(page, "secrets");
    await expect(secrets).toBeVisible();
    await expect(row(page, "secrets").locator("[data-row-title]")).toHaveText("Secrets and environments");
    await expect(secrets.locator("[data-settings-part] > h3")).toHaveText([/^Environments/, /^Project secrets/]);
    await expect(secrets.locator("[data-project-secret]")).toHaveText(/API_KEY/);
  });

  test("an old section's address lands on the row it's part of now", async ({ page }) => {
    await open(page, `/settings/project/${SITE}/environments`, () => project(page));
    await expect(page).toHaveURL(new RegExp(`/settings/project/${SITE}[^/]*/secrets$`));
    await expect(section(page, "secrets").locator('[data-settings-part="environments"]')).toBeVisible();
    await expect(line(page, "secrets")).toHaveAttribute("aria-expanded", "true");
    // An old `#section` too.
    await page.goto(`/p/${SITE}/settings#finish`);
    await expect(page).toHaveURL(new RegExp(`/settings/project/${SITE}[^/]*/agents$`));
    await expect(section(page, "agents").locator('[data-settings-part="finish"]')).toBeVisible();
    await page.goto(`/settings/project/${SITE}/agent-model`);
    await expect(page).toHaveURL(new RegExp(`/settings/project/${SITE}[^/]*/agents$`));
    await expect(section(page, "agents").locator('[data-settings-part="agent-model"]')).toBeVisible();
    // CI alerts, workspaces and Claude spend are Agents' now; the standing grant, Runs'.
    for (const old of ["size", "finish", "ci-alerts", "spend"]) {
      await page.goto(`/settings/project/${SITE}/${old}`);
      await expect(page).toHaveURL(new RegExp(`/settings/project/${SITE}[^/]*/agents$`));
      await expect(section(page, "agents")).toBeVisible();
    }
    await expect(section(page, "agents").locator('[data-settings-part="ci-alerts"]')).toBeVisible();
    await expect(section(page, "agents").locator('[data-settings-part="spend"]')).toBeVisible();
    await page.goto(`/settings/project/${SITE}/run-grant`);
    await expect(page).toHaveURL(new RegExp(`/settings/project/${SITE}[^/]*/runs$`));
    await expect(section(page, "runs").locator('[data-settings-part="run-specs"] [data-run-spec="nightly"]')).toBeVisible();
  });

  test("a setting a team default covers says it follows the team or is set here, and Reset clears its own (Arugula #1323)", async ({ page }) => {
    const sent: unknown[] = [];
    let mine: unknown[] = [{ key: "agents_model", value: "sonnet", source: "project" }];
    await open(page, `/settings/project/${SITE}/overrides`, () =>
      Promise.all([
        project(page),
        page.route(/\/api\/owners\/tm_1\/settings$/, (r) => json(r, [{ key: "agents_model", value: "opus", source: "owner" }])),
        page.route(new RegExp(`/api/projects/${SITE}/settings$`), (r) => {
          if (r.request().method() === "PUT") {
            sent.push(r.request().postDataJSON());
            mine = [{ key: "agents_model", value: "opus", source: "owner" }];
            return json(r, {});
          }
          return json(r, mine);
        }),
      ]),
    );
    // The Overrides section went: its address lands on the first such setting's section.
    await expect(page).toHaveURL(new RegExp(`/settings/project/${SITE}[^/]*/issues$`));
    await line(page, "agents").click();
    const said = page.locator('[data-team-default="agents_model"]');
    await expect(said).toHaveText("Set here · team default is Opus · Reset");
    await said.locator('[data-team-default-reset="agents_model"]').click();
    await expect(said).toHaveText("Team default · follows Acme");
    expect(sent).toEqual([{ key: "agents_model", value: null }]);
  });
});

test.describe("on a phone", () => {
  const { viewport, userAgent, deviceScaleFactor, isMobile, hasTouch } = devices["Pixel 7"];
  test.use({ viewport, userAgent, deviceScaleFactor, isMobile, hasTouch });

  test("the same list: a row opens in place under its line, with no sideways scroll, and closes again", async ({ page }) => {
    await open(page, "/settings");
    await expect(row(page, "connected")).toBeVisible();
    await expect(page.locator('[data-settings-row] > button[aria-expanded="true"]')).toHaveCount(0);
    await line(page, "connected").click();
    await expect(page).toHaveURL(/\/settings\/you\/connected$/);
    await expect(section(page, "connected")).toBeVisible();
    // In place: its line stays, the other rows too.
    await expect(row(page, "connected")).toBeVisible();
    await expect(line(page, "connected")).toHaveAttribute("aria-expanded", "true");
    await expect(row(page, "notifications")).toBeVisible();
    // Its body under its line, in the row.
    const [headBottom, bodyTop] = await row(page, "connected").evaluate((el) => [
      el.querySelector(":scope > button[aria-expanded]")!.getBoundingClientRect().bottom,
      el.querySelector("[data-settings-section]")!.getBoundingClientRect().top,
    ]);
    expect(bodyTop).toBeGreaterThanOrEqual(headBottom - 1);
    // Nothing wider than the phone.
    expect(await page.evaluate(() => document.documentElement.scrollWidth - document.documentElement.clientWidth)).toBeLessThanOrEqual(0);
    await line(page, "connected").click();
    await expect(page).toHaveURL(/\/settings$/);
    await expect(section(page, "connected")).toBeHidden();
    await expect(page.locator('[data-settings-row] > button[aria-expanded="true"]')).toHaveCount(0);
  });
});
