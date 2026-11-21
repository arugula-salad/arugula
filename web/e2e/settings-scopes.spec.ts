// Settings (Arugula #1104): one page, four scopes (You, Machines,
// Project, Team and billing), each a list of its sections with what's set,
// and the chosen one's controls beside it. The section is the address, so a
// link opens it; the old addresses (a project's settings, Personal's, a
// team's Settings tab, `#section`) land in their scope. On a phone, the
// list, then a section full width with a way back. The Project scope's 17
// sections are 10 rows in a flat list (Arugula #1105, #1323), and an old section's
// address lands on the row it's part of. You holds control's device and
// preference rows too (Arugula #1108): what's on control's page is a ↗ row
// to its dialog there, and Help is the palette's. Sign-in and security is
// control's dialog drawn in place (Arugula #1311), a ↗ row while the hub
// doesn't answer `/api/me/*` on the app's host; so are what every device
// is pushed about (Arugula #1310) and `/api/push/prefs`. On #971's fixture page
// (move-to-project-page.tsx): the platform's answers are this spec's.
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
    // Your machines' agents are each machine's, under Machines (Arugula #1107).
    await expect(row(page, "agents")).toHaveCount(0);
    // The first is shown, the rest wait; its help is under its heading, with no More to open.
    await expect(row(page, "notifications")).toHaveAttribute("aria-current", "true");
    await expect(section(page, "notifications")).toBeVisible();
    await expect(section(page, "quiet-hours")).toBeHidden();
    await expect(page.locator("details.settings-more")).toHaveCount(0);
    await row(page, "quiet-hours").click();
    await expect(page).toHaveURL(/\/settings\/you\/quiet-hours$/);
    await expect(section(page, "quiet-hours")).toBeVisible();
    await expect(section(page, "notifications")).toBeHidden();
    await expect(row(page, "quiet-hours")).toHaveAttribute("aria-current", "true");
    // A link opens its section, and an old one's `#section` lands on it.
    await page.goto("/settings/you/tokens");
    await expect(section(page, "tokens")).toBeVisible();
    await page.goto("/settings#notifications");
    await expect(page).toHaveURL(/\/settings\/you\/notifications$/);
    await expect(section(page, "notifications")).toBeVisible();
    // The old Agents on your machines' address opens Machines' Agents (Arugula #1107).
    await page.goto("/settings#agents");
    await expect(page.locator('[data-settings-scope="machine"]')).toHaveAttribute("aria-selected", "true");
  });

  test("You: control's device and preference rows join it in one flat list, a device's are ↗ rows to control's page at their dialog, and there's no second index or Help", async ({ page }) => {
    // A hub that doesn't answer push's prefs on the app's host yet.
    await open(page, "/settings", () => page.route(/\/api\/push\/prefs$/, (r) => r.fulfill({ status: 404, body: "" })));
    // One flat list: no band headings over the sections.
    await expect(page.locator(".settings-list .list-group-title")).toHaveCount(0);
    const ids = () => page.locator("[data-settings-row]").evaluateAll((els) => els.map((e) => (e as HTMLElement).dataset.settingsRow));
    // (Chat alerts, after quiet hours, shows once chat is on: it isn't here.)
    await expect.poll(ids).toEqual(["notifications", "quiet-hours", "push", "appearance", "security", "machines-teams", "linked", "tokens"]);
    // Appearance in one place: its controls drawn in its own section, no dialog to open.
    await expect(page.locator("[data-appearance-controls]")).toHaveCount(1);
    await expect(section(page, "appearance").locator("[data-appearance-controls]")).toHaveCount(1);
    await expect(page.getByRole("button", { name: "Appearance…" })).toHaveCount(0);
    await expect(row(page, "appearance").locator(".list-row-detail")).toHaveText(/ on this device · Tips on · opens in VS Code$/);
    // No index of control's sections, and no Help.
    await expect(section(page, "elsewhere")).toHaveCount(0);
    await expect(page.locator("[data-settings-elsewhere]")).toHaveCount(0);
    await expect(page.getByText("Getting started")).toHaveCount(0);
    await expect(page.getByText("What's new")).toHaveCount(0);

    // What's on control's page: a ↗ row each, at its dialog there. No Add a
    // phone or browser (#1004), and no Machines: Team and billing has them.
    const origin = new URL(page.url()).origin;
    await row(page, "machines-teams").click();
    await expect(page).toHaveURL(/\/settings\/you\/machines-teams$/);
    const account = section(page, "machines-teams");
    await expect(account.locator("h2")).toHaveText("Machines and teams");
    const out = account.locator("[data-settings-control]");
    await expect(out).toHaveText(["Teams", "Shared Docs"].map((t) => new RegExp(`^${t}`)));
    expect(await out.evaluateAll((els) => els.map((e) => (e as HTMLAnchorElement).href))).toEqual(["teams", "shared-docs"].map((h) => `${origin}/#${h}`));
    // Phones and browsers, while the hub doesn't answer its prefs here: This
    // device and All your devices, as one, on control's page.
    await row(page, "push").click();
    const push = section(page, "push").locator("[data-settings-control]");
    await expect(push).toHaveCount(2);
    for (const href of await push.evaluateAll((els) => els.map((e) => (e as HTMLAnchorElement).href))) expect(href).toBe(`${origin}/#settings=push`);
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
    await expect(sec.locator("h2")).toHaveText("Sign-in and security");
    await expect(row(page, "security").locator(".list-row-detail")).toHaveText("1 passkey · GitHub linked · 3 sign-ins");
    // Passkeys: a list, with nothing to remove here; adding or removing one is on control's page, at the top.
    const origin = new URL(page.url()).origin;
    await expect(sec.locator("[data-passkey-name]")).toHaveText(["iCloud Keychain"]);
    await expect(sec.locator("[data-remove-passkey], [data-add-passkey]")).toHaveCount(0);
    await expect(sec.locator('[data-security-away="passkeys"]')).toHaveAttribute("href", `${origin}/#account`);
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
  });

  test("You: a hub that doesn't answer /api/me/* on the app's host yet gets Sign-in and security as a ↗ row to control's dialog", async ({ page }) => {
    await open(page, "/settings/you/security", () => page.route(/\/api\/me\//, (r) => r.fulfill({ status: 404, body: "" })));
    const sec = section(page, "security");
    await expect(row(page, "security").locator(".list-row-detail")).toHaveText("On your machines page");
    const out = sec.locator("[data-settings-control]");
    await expect(out).toHaveText([/^Sign-in and account/]);
    await expect(out).toHaveAttribute("href", `${new URL(page.url()).origin}/#account`);
    await expect(sec.locator("[data-passkeys]")).toHaveCount(0);
  });

  test("You: Phones and browsers sets what every device is pushed about in place; this device's switch and the test stay on control's page (Arugula #1310)", async ({ page }) => {
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
    const push = section(page, "push");
    await expect(row(page, "push").locator(".list-row-detail")).toHaveText("Pushed: Chat, Machines");
    const kinds = push.locator("[data-push-kind]");
    await expect(kinds).toHaveCount(3);
    await expect(push.locator('[data-push-kind="chat"]')).toBeChecked();
    await expect(push.locator('[data-push-kind="projects"]')).not.toBeChecked();
    await push.locator('[data-push-kind="projects"]').click();
    await expect(push.locator('[data-push-kind="projects"]')).toBeChecked();
    await expect(row(page, "push").locator(".list-row-detail")).toHaveText("Pushed: Everything");
    expect(sent).toEqual([{ projects: true }]);
    // This device: one ↗ row to control's push settings.
    const out = push.locator("[data-settings-control]");
    await expect(out).toHaveText([/^Notify this device/]);
    await expect(out).toHaveAttribute("href", `${new URL(page.url()).origin}/#settings=push`);
  });

  test("Status: on hosted Arugula your picture's menu and You link status.arugula.io in a new tab; on a control you run there's none (Arugula #1176)", async ({ page, context }) => {
    // Nothing leaves for arugula.io: the status page is this spec's.
    await context.route(/^https:\/\/[^/]*arugula\.io\//, (r) => r.fulfill({ contentType: "text/html", body: "<title>Status</title>" }));
    const menu = async () => {
      await page.locator(".ws-rail [data-signed-in-as]").click();
      return page.locator(".menu-item");
    };
    // A control you run yourself (this page's own hub): no Status anywhere.
    await open(page, "/settings/you/security");
    await expect(section(page, "security")).toBeVisible();
    await expect(row(page, "status-page")).toHaveCount(0);
    await expect(page.locator("[data-status-page]")).toHaveCount(0);
    await expect(await menu()).toHaveText(["Send feedback…", "Sign out of this page"]);
    await page.keyboard.press("Escape");

    // Hosted Arugula: control says it's the hub on every answer on the app's host.
    await page.unrouteAll({ behavior: "wait" });
    await open(page, "/settings/you/status-page", () =>
      page.route(/\/api\/projects$/, (r) =>
        r.fulfill({ contentType: "application/json", headers: { "x-arugula-hub": "https://control.arugula.io" }, body: JSON.stringify({ me: { account: "acct_a", login: "sam", name: "Sam" }, projects, hosted: false }) }),
      ),
    );
    // Settings › You: a Status row after Machines and teams.
    const ids = () => page.locator("[data-settings-row]").evaluateAll((els) => els.map((e) => (e as HTMLElement).dataset.settingsRow));
    await expect.poll(ids).toEqual(["notifications", "quiet-hours", "push", "appearance", "security", "machines-teams", "status-page", "linked", "tokens"]);
    await expect(row(page, "status-page").locator(".list-row-detail")).toHaveText("status.arugula.io");
    const link = section(page, "status-page").locator("a[data-status-page]");
    await expect(link).toHaveAttribute("href", "https://status.arugula.io");
    await expect(link).toHaveAttribute("target", "_blank");
    await expect(link).toHaveAttribute("rel", "noopener");
    // The menu: Status first, opening a tab that can't reach back into this one.
    const items = await menu();
    await expect(items).toHaveText([/^Status/, "Send feedback…", "Sign out of this page"]);
    const tab = context.waitForEvent("page");
    await page.locator(".menu-item[data-status-page]").click();
    const status = await tab;
    await expect(status).toHaveURL("https://status.arugula.io/");
    expect(await status.evaluate(() => window.opener)).toBeNull();
    await expect(page).toHaveURL(/\/settings\/you\/status-page$/);
  });

  test("You: Appearance draws its controls in place, Preferences under them, and its summary says what's set", async ({ page }) => {
    // Preferences' old address lands on Appearance, which holds it now.
    await open(page, "/settings/you/preferences");
    await expect(page).toHaveURL(/\/settings\/you\/appearance$/);
    const look = section(page, "appearance");
    await expect(look).toBeVisible();
    await expect(look.locator("#appearance-h")).toHaveText("Appearance");
    const detail = row(page, "appearance").locator(".list-row-detail");
    // A palette applies at once, and the row says it.
    await look.locator('[data-palette-choice="latte"]').click();
    await expect(page.locator("html")).toHaveAttribute("data-palette", "latte");
    await expect(detail).toHaveText(/^Latte, .* on this device · /);
    await look.getByRole("button", { name: "Reset", exact: true }).click();
    await expect(page.locator("html")).not.toHaveAttribute("data-palette", "latte");
    // Preferences, a part of it, below the controls.
    const prefs = look.locator('[data-settings-part="preferences"]');
    await expect(prefs).toBeVisible();
    await expect(detail).toHaveText(/ · Tips on · opens in VS Code$/);
    const tips = prefs.getByRole("switch", { name: "Tips", exact: true });
    await expect(tips).toBeChecked();
    await tips.click();
    await expect(tips).not.toBeChecked();
    await prefs.locator('[data-pref="editor"]').selectOption("nvim");
    await expect(detail).toHaveText(/ · Tips off · opens in nvim$/);
    await prefs.getByRole("switch", { name: "Tips", exact: true }).click();
    await prefs.getByRole("switch", { name: "Big tips" }).click();
    await prefs.locator('[data-pref="editor"]').selectOption("ask");
    await expect(detail).toHaveText(/ · Tips on, not big ones · asks which editor$/);
    // Lending this device to agents, this browser's alone, between the two.
    const lend = look.getByRole("switch", { name: "Lend this device to agents" });
    await expect(lend).not.toBeChecked();
    await lend.click();
    await expect(lend).toBeChecked();
    await expect(detail).toHaveText(/ on this device · lent to agents · Tips on/);
    expect(await page.evaluate(() => localStorage.getItem("arugula-hand"))).toBe("on");
    await lend.click();
    await expect(detail).not.toContainText("lent to agents");
    expect(await page.evaluate(() => localStorage.getItem("arugula-hand"))).toBe("off");
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
    // The default project is the top of Project defaults, and General's address lands there.
    await expect(row(page, "defaults").locator(".list-row-detail")).toHaveText(/^Default project: None · /);
    await expect(row(page, "general")).toHaveCount(0);
    await page.goto("/settings/team/personal/general");
    await expect(page).toHaveURL(/\/settings\/team\/personal\/defaults$/);
    await expect(section(page, "defaults").locator("[data-default-project-form]")).toBeVisible();
    await page.goto("/settings/team/personal");
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
    ]);

  test("the rows in a flat list, a merged row saying its parts' in a line, each part under its own heading", async ({ page }) => {
    await open(page, `/settings/project/${SITE}`, () => project(page));
    await expect(page.locator(".settings-list .list-group-title")).toHaveCount(0);
    await expect(row(page, "spend")).toBeVisible();
    // Apps from products, the tenth, shows once a product keeps one here.
    const rows = await page.locator("[data-settings-row]").evaluateAll((els) => els.map((e) => e.getAttribute("data-settings-row")));
    expect(rows).toEqual(["general", "access", "repos", "issues", "ci-alerts", "runs", "agents", "secrets", "spend"]);
    await expect(row(page, "repos").locator(".list-row-detail")).toHaveText("acme/site");
    await expect(row(page, "agents").locator(".list-row-detail")).toHaveText("The strongest (Opus) · 1 token");
    await expect(row(page, "secrets").locator(".list-row-detail")).toHaveText("No environments · 1 secret");
    await expect(row(page, "runs").locator(".list-row-detail")).toHaveText("No standing grant");
    await row(page, "secrets").click();
    await expect(page).toHaveURL(new RegExp(`/settings/project/${SITE}[^/]*/secrets$`));
    const secrets = section(page, "secrets");
    await expect(secrets.locator("h2")).toHaveText("Secrets and environments");
    await expect(secrets.locator("[data-settings-part] > h3")).toHaveText([/^Environments/, /^Project secrets/]);
    await expect(secrets.locator("[data-project-secret]")).toHaveText(/API_KEY/);
  });

  test("an old section's address lands on the row it's part of now", async ({ page }) => {
    await open(page, `/settings/project/${SITE}/environments`, () => project(page));
    await expect(page).toHaveURL(new RegExp(`/settings/project/${SITE}[^/]*/secrets$`));
    await expect(section(page, "secrets").locator('[data-settings-part="environments"]')).toBeVisible();
    await expect(row(page, "secrets")).toHaveAttribute("aria-current", "true");
    // An old `#section` too, and one that's still a row of its own.
    await page.goto(`/p/${SITE}/settings#finish`);
    await expect(page).toHaveURL(new RegExp(`/settings/project/${SITE}[^/]*/runs$`));
    await expect(section(page, "runs")).toBeVisible();
    await page.goto(`/settings/project/${SITE}/agent-model`);
    await expect(page).toHaveURL(new RegExp(`/settings/project/${SITE}[^/]*/agents$`));
    await expect(section(page, "agents").locator('[data-settings-part="agent-model"]')).toBeVisible();
    await page.goto(`/settings/project/${SITE}/ci-alerts`);
    await expect(section(page, "ci-alerts")).toBeVisible();
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
    await row(page, "agents").click();
    const line = page.locator('[data-team-default="agents_model"]');
    await expect(line).toHaveText("Set here · team default is Opus · Reset");
    await line.locator('[data-team-default-reset="agents_model"]').click();
    await expect(line).toHaveText("Team default · follows Acme");
    expect(sent).toEqual([{ key: "agents_model", value: null }]);
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
