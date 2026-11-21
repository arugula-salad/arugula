// Settings › Machines in the app (Arugula #1107): four sections, each
// saying what's set. Access and sharing (who reaches it, Guest SSH access,
// who reached it through Arugula cloud) and Agents (its agents' grants, one
// editable part per machine, and its permission rules), then its name and
// Advanced (usage and crash reports, developer settings). The machine's
// own are read from it through the app's client to it. A machine the app
// can't reach says so in its sections' summaries. On the app
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
const part = (page: Page, id: string) => page.locator(`[data-settings-part="${id}"]`);

test("a machine's own settings, its agents' grants and its name, each a row with what's set", async ({ page }) => {
  await open(page, "/settings/machine", true);
  // The first machine, picked.
  await expect(page).toHaveURL(new RegExp(`/settings/machine/${SUITE}[^/]*$`));
  await expect(page.locator('[data-settings-pick="machine"]')).toHaveValue(SUITE);
  // Four, in two bands.
  await expect(page.locator(".settings-list .list-group-title")).toHaveText(["Who and what", "About it"]);
  await expect(page.locator("[data-settings-row]")).toHaveText([/^Access and sharing/, /^Agents/, /^Name/, /^Advanced/]);
  // What's set, read from the machine: each part's, in a line.
  await expect(said(page, "access")).toHaveText(/^(Only you|You \+ .+) · Guest SSH (on|off)/);
  await expect(said(page, "agents")).toHaveText(/^No grants yet · (No rules yet|\d+ rules?)$/);
  await expect(said(page, "advanced")).toHaveText(/^(On|Off|Off on this machine|Not answered yet) · (All off|\d+ of \d+ on)$/);
  await expect(said(page, "name")).toHaveText("studio-mini");
  await expect(section(page, "access")).toBeVisible();
  await expect(part(page, "who").locator("[data-access-kind]")).toBeVisible();
  await expect(section(page, "access").locator("[data-settings-part]")).toHaveText([/^Who can reach it/, /^Guest SSH access/, /^Remote sessions/]);

  // Guest SSH: the dialog's switch, here; its row follows.
  const was = await said(page, "access").textContent();
  const ssh = part(page, "guest-ssh").locator("[data-sharing-guest-ssh]");
  const on = await ssh.isChecked();
  await ssh.click();
  await expect(said(page, "access")).toContainText(on ? "Guest SSH off" : "Guest SSH on");
  await ssh.click();
  await expect(said(page, "access")).toHaveText(was!);

  // Advanced: its reports and developer settings.
  await row(page, "advanced").click();
  await expect(page).toHaveURL(new RegExp(`/settings/machine/${SUITE}[^/]*/advanced$`));
  await expect(section(page, "advanced").locator("[data-settings-part]")).toHaveText([/^Usage and crash reports/, /^Developer settings/]);
  await expect(part(page, "developer").locator("[data-flag]").first()).toBeVisible();

  // Agents: its grants, a row per project and your DMs, then its permission rules; a switch gives, and the row says so.
  await row(page, "agents").click();
  await expect(section(page, "agents").locator("[data-settings-part]")).toHaveText([/^Agents' grants/, /^Permission rules/]);
  const grants = part(page, "grants");
  await expect(grants.locator("tbody tr")).toHaveCount(4);
  await grants.locator(`[data-agent-cell="${SUITE}/${P}/threads"]`).click();
  await expect(said(page, "agents")).toHaveText(/^1 project · /);
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
  for (const id of ["access", "advanced"]) await expect(said(page, id)).toHaveText("Can't reach it from here");
  await expect(section(page, "access")).toContainText("This page can't reach zoo-box yet");
  // A ↗ row to its own page, where its settings are.
  await expect(section(page, "access").locator('[data-settings-control="machine"]')).toBeVisible();
  await expect(said(page, "agents")).toHaveText("No grants yet");
  await row(page, "name").click();
  await section(page, "name").locator("[data-machine-rename-name]").fill("near-box");
  await section(page, "name").locator("[data-machine-rename]").click();
  await expect(section(page, "name").locator("[data-machine-rename-elsewhere]")).toBeVisible();
  await expect(section(page, "name").locator('[data-settings-control="rename"]')).toBeVisible();
  await expect(said(page, "name")).toHaveText("zoo-box");
});

test("the old address of Agents on your machines opens the first machine's Agents", async ({ page }) => {
  await open(page, "/settings#agents", false);
  await expect(page).toHaveURL(new RegExp(`/settings/machine/${SUITE}[^/]*/agents$`));
  await expect(part(page, "grants")).toBeVisible();
});

test("a machine's sections from before the four land on the one holding each", async ({ page }) => {
  await open(page, `/settings/machine/${SUITE}/guest-ssh`, false);
  await expect(page).toHaveURL(new RegExp(`/settings/machine/${SUITE}[^/]*/access$`));
  await expect(part(page, "guest-ssh")).toBeVisible();
  for (const [old, now, holds] of [
    ["rules", "agents", "rules"],
    ["reports", "advanced", "reports"],
    ["developer", "advanced", "developer"],
  ]) {
    await page.goto(`/settings/machine/${SUITE}/${old}`);
    await expect(page).toHaveURL(new RegExp(`/settings/machine/${SUITE}[^/]*/${now}$`));
    await expect(part(page, holds)).toBeVisible();
  }
});

// Arugula #1110: what was a section of control's `#settings`.
test("who reached the machine through Arugula cloud is a part of Access and sharing, saying how many in its row", async ({ page }) => {
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
  await expect(page).toHaveURL(new RegExp(`/settings/machine/${SUITE}[^/]*/access$`));
  await expect(said(page, "access")).toContainText("2 sessions, 1 open · watch only");
  await expect(part(page, "remote-sessions").locator("[data-remote-session]")).toHaveCount(2);
  await expect(part(page, "remote-sessions").locator("[data-remote-input]")).toHaveAttribute("data-remote-input", "off");
});

test("a machine's old Settings addresses land on Machines", async ({ page }) => {
  // Control's page sends `#settings=machine` here, over the machines (machine-paths' MOVES):
  // the list names no machine, so Machines opens at the first.
  await open(page, `/p/${P}/machines#settings=machine`, false);
  await expect(page).toHaveURL(new RegExp(`/settings/machine/${SUITE}[^/]*$`));
  await expect(section(page, "access")).toBeVisible();
  // Back doesn't land on the old address again.
  await page.goBack();
  await expect(page).not.toHaveURL(/#settings/);
  await page.goto(`/p/${P}/machines#settings=remote-sessions`);
  await expect(page).toHaveURL(new RegExp(`/settings/machine/${SUITE}[^/]*/access$`));
  // Another of control's sections: You's, by the same id.
  await page.goto(`/p/${P}/machines#settings=preferences`);
  await expect(page).toHaveURL(/\/settings\/you\/preferences$/);
  // On a machine's page, the old address lands on the machine shown. (Its menu's Machine
  // settings… goes the same way; with one machine here, the page draws no machine menu.)
  await reset(page);
  const [pane] = await panes(page);
  await page.goto(`/p/${P}/machines?pane=${pane}#settings=machine`);
  await expect(page).toHaveURL(/\/settings\/machine\/[0-9a-f]+[^/]*$/);
  await expect(page.locator("[data-machine-settings-page]")).toBeVisible();
});
