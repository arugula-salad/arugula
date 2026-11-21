// Settings' This machine in the app (Arugula #1107): the machine's own
// settings (Access, Permission rules, Usage and crash reports, Guest SSH
// access, Developer settings) as sections, read from the machine through
// the app's client to it, each saying what's set; its agents' grants, one
// editable section per machine, after Access; and its name. A machine the
// app can't reach says so in each of its sections' summaries. On the app
// frame's fixture page (app-frame-page.tsx), served on the suite daemon's
// origin, which stands for the machine `SUITE`; the platform's grants and
// the hub's rename are this spec's. Who reached it through Arugula cloud
// and the machine's old addresses on control's `#settings` are here too
// (Arugula #1110).
import { expect, test, type Page, type Route } from "@playwright/test";
import { fixturePage } from "./fixture-page";
import { panes, reset } from "./helpers";

const SUITE = "5e771e0000000001";
const AWAY = "0ff11e0000000002";
const P = "prj_aaaa0002";

// A page a route fulfils has no address of its own, so Chrome counts it as
// public and won't let it reach the daemon on loopback without this.
test.use({ permissions: ["local-network-access"], viewport: { width: 1280, height: 900 } });

type Grant = { id: string; subject: { kind: string; id: string; name: string }; power: string; scope: { kind: string; id: string; name?: string }; live: boolean };

/** The app's Settings at `path`, with the platform's grants (every machine
 * of yours has `cloud.drive`) and the hub's rename answered here. */
async function open(page: Page, path: string, renames: boolean) {
  const names: Record<string, string> = { [SUITE]: "studio-mini", [AWAY]: "zoo-box" };
  const grants: Grant[] = Object.entries(names).map(([id, name]) => ({ id: `g_${id}`, subject: { kind: "daemon", id, name }, power: "cloud.drive", scope: { kind: "account", id: "acct_a" }, live: true }));
  const json = (r: Route, body: unknown, status = 200) => r.fulfill({ status, contentType: "application/json", body: JSON.stringify(body) });
  await page.route(/\/api\/grants$/, async (r) => {
    if (r.request().method() === "POST") {
      const b = r.request().postDataJSON() as { daemon: string; power: string; scope: { kind: string; id?: string } };
      grants.push({ id: `g_${grants.length}`, subject: { kind: "daemon", id: b.daemon, name: names[b.daemon] }, power: b.power, scope: { kind: b.scope.kind, id: b.scope.id ?? "acct_a" }, live: true });
      return json(r, {});
    }
    return json(r, { grants: grants.map((g) => ({ ...g, subject: { ...g.subject, name: names[g.subject.id] } })) });
  });
  // The hub's rename, or (as on the app's host today) a bare 404.
  await page.route(/\/api\/daemons\/[0-9a-f]+\/name$/, async (r) => {
    if (!renames) return r.fulfill({ status: 404, body: "" });
    const id = /daemons\/([0-9a-f]+)\//.exec(r.request().url())![1];
    const was = names[id];
    names[id] = (r.request().postDataJSON() as { name: string }).name;
    return json(r, { id, name: names[id], was });
  });
  const body = await fixturePage("./app-frame-page.tsx");
  // Settings, and a project's machines (whose machine page has the shell's own places).
  await page.route(/\/(settings|p\/)/, (r) => (r.request().resourceType() === "document" ? r.fulfill({ contentType: "text/html", body }) : r.fallback()));
  await page.goto(path);
}

const row = (page: Page, id: string) => page.locator(`[data-settings-row="${id}"]`);
const said = (page: Page, id: string) => row(page, id).locator(".list-row-detail");
const section = (page: Page, id: string) => page.locator(`[data-settings-section="${id}"]`);

test("a machine's own settings, its agents' grants and its name, each a row with what's set", async ({ page }) => {
  await open(page, "/settings/machine", true);
  // The first machine, picked.
  await expect(page).toHaveURL(new RegExp(`/settings/machine/${SUITE}[^/]*$`));
  await expect(page.locator('[data-settings-pick="machine"]')).toHaveValue(SUITE);
  // In the machine menu's order, its agents' grants after Access, its name last.
  await expect(page.locator("[data-settings-row]")).toHaveText([/^Access/, /^Agents' grants/, /^Permission rules/, /^Usage and crash reports/, /^Guest SSH access/, /^Developer settings/, /^Remote sessions/, /^Name/]);
  // What's set, read from the machine.
  await expect(said(page, "access")).toHaveText(/^(Only you|You \+ .+)$/);
  await expect(said(page, "rules")).toHaveText(/^(None yet|\d+ rules?)$/);
  await expect(said(page, "reports")).toHaveText(/^(On|Off|Off on this machine|Not answered yet)$/);
  await expect(said(page, "developer")).toHaveText(/^(All off|\d+ of \d+ on)$/);
  await expect(said(page, "agents")).toHaveText("Nothing yet");
  await expect(said(page, "name")).toHaveText("studio-mini");
  await expect(section(page, "access")).toBeVisible();
  await expect(section(page, "access").locator("[data-access-kind]")).toBeVisible();

  // Guest SSH: the dialog's switch, here; its row follows.
  const was = await said(page, "guest-ssh").textContent();
  await row(page, "guest-ssh").click();
  await expect(page).toHaveURL(new RegExp(`/settings/machine/${SUITE}[^/]*/guest-ssh$`));
  const ssh = section(page, "guest-ssh").locator("[data-sharing-guest-ssh]");
  const on = await ssh.isChecked();
  await ssh.click();
  await expect(said(page, "guest-ssh")).toHaveText(on ? /^Guest SSH off/ : /^Guest SSH on/);
  await ssh.click();
  await expect(said(page, "guest-ssh")).toHaveText(was!);

  // Its agents' grants: a row per project and your DMs; a switch gives, and the row says so.
  await row(page, "agents").click();
  const grants = section(page, "agents");
  await expect(grants.locator("tbody tr")).toHaveCount(4);
  await grants.locator(`[data-agent-cell="${SUITE}/${P}/threads"]`).click();
  await expect(said(page, "agents")).toHaveText("1 project");
  await expect(grants.locator(`[data-agent-cell="${SUITE}/${P}/threads"]`)).toBeChecked();

  // Its name, on Arugula cloud.
  await row(page, "name").click();
  await section(page, "name").locator("[data-machine-rename-name]").fill("studio-mini-2");
  await section(page, "name").locator("[data-machine-rename]").click();
  await expect(said(page, "name")).toHaveText("studio-mini-2");
  await expect(page.locator('[data-settings-pick="machine"] option:checked')).toHaveText("studio-mini-2");
});

test("a machine the app can't reach says so in each of its sections; its grants and name are Arugula cloud's", async ({ page }) => {
  // Arugula cloud that doesn't rename from the app's host: a bare 404.
  await open(page, `/settings/machine/${AWAY}`, false);
  for (const id of ["access", "rules", "reports", "guest-ssh", "developer"]) await expect(said(page, id)).toHaveText("Can't reach it from here");
  await expect(section(page, "access")).toContainText("This page can't reach zoo-box yet");
  // A ↗ row to its own page, where its settings are.
  await expect(section(page, "access").locator('[data-settings-control="machine"]')).toBeVisible();
  await expect(said(page, "agents")).toHaveText("Nothing yet");
  await row(page, "name").click();
  await section(page, "name").locator("[data-machine-rename-name]").fill("near-box");
  await section(page, "name").locator("[data-machine-rename]").click();
  await expect(section(page, "name").locator("[data-machine-rename-elsewhere]")).toBeVisible();
  await expect(section(page, "name").locator('[data-settings-control="rename"]')).toBeVisible();
  await expect(said(page, "name")).toHaveText("zoo-box");
});

test("the old address of Agents on your machines opens the first machine's Agents' grants", async ({ page }) => {
  await open(page, "/settings#agents", false);
  await expect(page).toHaveURL(new RegExp(`/settings/machine/${SUITE}[^/]*/agents$`));
  await expect(section(page, "agents")).toBeVisible();
});

// Arugula #1110: what was a section of control's `#settings`.
test("who reached the machine through Arugula cloud is a section of This machine, saying how many in its row", async ({ page }) => {
  const now = Date.now();
  await page.route("**/api/remote-sessions", (r) =>
    r.fulfill({
      json: {
        remote_input: false,
        sessions: [
          { account: "a2", name: "lee", role: "viewer", path: "relay", by: "token", opened_ms: now - 60_000, open: true },
          { account: "a1", name: "sam", role: "editor", path: "relay", by: "token", opened_ms: now - 7_200_000, closed_ms: now - 6_480_000, open: false },
        ],
      },
    }),
  );
  await open(page, `/settings/machine/${SUITE}/remote-sessions`, false);
  await expect(said(page, "remote-sessions")).toHaveText("2 sessions, 1 open · watch only");
  await expect(section(page, "remote-sessions").locator("[data-remote-session]")).toHaveCount(2);
  await expect(section(page, "remote-sessions").locator("[data-remote-input]")).toHaveAttribute("data-remote-input", "off");
});

test("a machine's old Settings addresses land on This machine", async ({ page }) => {
  // Control's page sends `#settings=machine` here, over the machines (machine-paths' MOVES):
  // the list names no machine, so This machine opens at the first.
  await open(page, `/p/${P}/machines#settings=machine`, false);
  await expect(page).toHaveURL(new RegExp(`/settings/machine/${SUITE}[^/]*$`));
  await expect(section(page, "access")).toBeVisible();
  // Back doesn't land on the old address again.
  await page.goBack();
  await expect(page).not.toHaveURL(/#settings/);
  await page.goto(`/p/${P}/machines#settings=remote-sessions`);
  await expect(page).toHaveURL(new RegExp(`/settings/machine/${SUITE}[^/]*/remote-sessions$`));
  // Another of control's sections: You's, by the same id; Preferences is Appearance's now.
  await page.goto(`/p/${P}/machines#settings=account`);
  await expect(page).toHaveURL(/\/settings\/you\/account$/);
  await page.goto(`/p/${P}/machines#settings=preferences`);
  await expect(page).toHaveURL(/\/settings\/you\/appearance$/);
  // On a machine's page, the old address lands on the machine shown. (Its menu's Machine
  // settings… goes the same way; with one machine here, the page draws no machine menu.)
  await reset(page);
  const [pane] = await panes(page);
  await page.goto(`/p/${P}/machines?pane=${pane}#settings=machine`);
  await expect(page).toHaveURL(/\/settings\/machine\/[0-9a-f]+[^/]*$/);
  await expect(page.locator("[data-machine-settings-page]")).toBeVisible();
});
