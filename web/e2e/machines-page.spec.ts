// The Machines page (Arugula #1346): a section per machine, a card per
// session, its panes as rows. This computer first, then online before
// offline, yours before teammates'; machines with nothing of the project
// folded into one line at the bottom; pinned sessions first; the pane you
// had open last lit; another project's session nowhere; and on a phone,
// one column and nothing wider than the screen. On the app's fixture page
// (app-frame-page.tsx) over the suite's daemon, with made-up machines
// added to the page's fleet beside it, as their summaries would come.
import { expect, test, type Page } from "@playwright/test";
import { fixturePage } from "./fixture-page";
import { inProject, panes, reset } from "./helpers";

const P = "prj_aaaa0002";
const SITE = "prj_aaaa0003";

test.use({ permissions: ["local-network-access"], serviceWorkers: "block" });

/** `[id, project, [[tab, [pane...], name?]...]]`: a made-up machine's session. */
type FakeSession = [number, string, [number, number[], string?][]];
type FakeHost = { name: string; id: string; online: boolean; owner?: string; sessions: FakeSession[]; asks?: number[]; agents?: number[] };

const HOSTS: FakeHost[] = [
  // Yours, online: a session of two tabs, one of Site's (not shown) and one asking.
  {
    name: "amy-laptop",
    id: "a11a000000000001",
    online: true,
    sessions: [
      [1, P, [[10, [1, 2], "build"], [11, [3], "logs"]]],
      [2, SITE, [[12, [4]]]],
      [3, P, [[13, [5]]]],
    ],
    asks: [5],
  },
  // Yours, offline.
  { name: "zed-box", id: "2ed0000000000001", online: false, sessions: [[1, P, [[10, [1]]]]] },
  // A teammate's, online: two sessions, one of which gets pinned.
  {
    name: "bob-mini",
    id: "b0b0000000000001",
    online: true,
    owner: "Bob",
    sessions: [
      [1, P, [[10, [1]]]],
      [2, P, [[11, [2]]]],
    ],
  },
  // Quiet: yours with no sessions, and yours with Site's only.
  { name: "idle-box", id: "1d1e000000000001", online: true, sessions: [] },
  { name: "old-box", id: "01d0000000000001", online: false, sessions: [[1, SITE, [[10, [1]]]]] },
  // A teammate's with nothing of the project: not here at all.
  { name: "eve-pc", id: "e7e0000000000001", online: true, owner: "Eve", sessions: [] },
];

/** The made-up machines in the page's fleet, beside the suite's daemon. */
async function addMachines(page: Page, hosts: FakeHost[]) {
  await page.waitForFunction(() => !!(window as unknown as { __app?: unknown }).__app);
  await page.evaluate(
    ([hosts, names]) => {
      const fleet = (window as unknown as { __app: { fleet: { inject(p: unknown[]): void } } }).__app.fleet;
      const proto = Object.getPrototypeOf(fleet);
      const real = (k: string) => Object.getOwnPropertyDescriptor(proto, k)!.get!;
      const project = (id: string) => ({ id, name: names[id] });
      const fakeHosts = hosts.map((h) => ({
        name: h.name,
        id: h.id,
        transport: "relay",
        status: h.online ? "online" : "offline",
        state: h.online ? "connected" : "offline",
        ...(h.owner ? { owner: h.owner, ownerId: `acct_${h.owner}` } : {}),
        lastSeen: null,
        path: null,
        summary: {
          rev: 1,
          sessions: h.sessions.map(([id, p, tabs]) => ({ id, name: `${h.name.split("-")[0]}-${id}`, tabs: tabs.map(([t]) => t), project: project(p) })),
          tabs: h.sessions.flatMap(([, , tabs]) => tabs.map(([id, ps, name]) => ({ id, name: name ?? null, layout: { panes: ps.map((p) => [p, { x: 0, y: 0, w: 1, h: 1 }]), splits: [] } }))),
          panes: [],
          machines: [],
          options: {},
        },
      }));
      const fakePanes = hosts.flatMap((h) =>
        h.sessions.flatMap(([sid, p, tabs]) =>
          tabs.flatMap(([, ps]) =>
            ps.map((id) => ({
              key: `${h.name}:${id}`,
              host: h.name,
              id,
              info: { id, type: "terminal", attention: h.asks?.includes(id) ? "needs_input" : "idle", command: `job-${h.name}-${id}`, branch: "main", session_project: project(p) },
              session: { id: sid, name: `${h.name.split("-")[0]}-${sid}` },
              stale: !h.online,
              owner: h.owner,
              person: h.owner ? { id: `account:acct_${h.owner}`, name: h.owner, kind: "person" } : { id: "me", name: "me", kind: "me" },
              driver: null,
              watchers: [],
              unread: 0,
              mention: false,
            })),
          ),
        ),
      );
      Object.defineProperty(fleet, "list", { configurable: true, get: () => [...real("list").call(fleet), ...fakeHosts] });
      Object.defineProperty(fleet, "panes", { configurable: true, get: () => [...real("panes").call(fleet).filter((p: { key: string }) => !fakePanes.some((f) => f.key === p.key)), ...fakePanes] });
      fleet.inject([]);
    },
    [hosts, { [P]: "Arugula", [SITE]: "Site" }] as const,
  );
}

/** `GET` and `PUT /api/sessions/pins`, kept here. */
async function stubPins(page: Page, pins: { machine: string; session: number }[]) {
  const kept = { pins };
  await page.route("**/api/sessions/pins", async (r) => {
    if (r.request().method() === "PUT") kept.pins = (r.request().postDataJSON() as { pins: typeof pins }).pins;
    await r.fulfill({ json: { pins: kept.pins } });
  });
  return kept;
}

/** A fresh pane on the suite's daemon, in the project, then the app at `path(pane)`. */
async function openApp(page: Page, path: (pane: number) => string): Promise<number> {
  await reset(page);
  const [pane] = await panes(page);
  await inProject(page, P, "Arugula");
  const body = await fixturePage("./app-frame-page.tsx");
  await page.route(new RegExp(`/p/${P}`), (r) => r.fulfill({ contentType: "text/html", body }));
  await page.goto(path(pane));
  return pane;
}

const groups = (page: Page) =>
  page.locator("[data-machines-list] > [data-machines-group]").evaluateAll((els) => els.map((e) => e.getAttribute("aria-label")));

test.describe("on a computer", () => {
  test.use({ viewport: { width: 1280, height: 900 } });

  test("machines in order, each with its sessions as cards and their panes; pins first; the last pane lit; quiet ones folded", async ({ page }, info) => {
    await stubPins(page, [{ machine: "b0b0000000000001", session: 2 }]);
    // Open the suite's pane, so it's the one you had open last; then Machines.
    const pane = await openApp(page, (p) => `/p/${P}/machines?pane=${p}`);
    await expect(page.locator(`[data-pane="${pane}"]`)).toBeVisible();
    await page.locator(".ws-rail [data-open-machines]").click();
    await expect(page).toHaveURL(new RegExp(`/p/${P}[^/]*/machines$`));
    await addMachines(page, HOSTS);
    const list = page.locator("[data-machines-list]");

    // This computer, then online (yours, then teammates'), then offline.
    await expect.poll(() => groups(page)).toHaveLength(4);
    const order = await groups(page);
    expect(order.slice(1)).toEqual(["amy-laptop", "bob-mini", "zed-box"]);
    const suite = list.locator("> [data-machines-group]").first();
    await expect(suite.locator("[data-machines-here]")).toHaveText("this computer");
    await expect(list.locator('> [data-machines-group="b0b0000000000001"] .machines-head')).toContainText("Bob's");

    // Grouped: amy's sessions are cards, each with its panes; two tabs name theirs.
    const amy = list.locator('> [data-machines-group="a11a000000000001"]');
    await expect(amy.locator("[data-machines-session]")).toHaveCount(2);
    // What needs you first.
    await expect(amy.locator("[data-machines-session]").first()).toHaveAttribute("data-machines-session", "3");
    await expect(amy.locator("[data-machines-session]").first()).toContainText("Needs you");
    const two = amy.locator('[data-machines-session="1"]');
    await expect(two.locator(".machines-session-meta")).toHaveText("2 tabs · 3 panes");
    await expect(two.locator(".machines-tab")).toHaveText(["build", "logs"]);
    await expect(two.locator("[data-panes-pane]")).toHaveCount(3);
    await expect(amy.locator('[data-machines-session="3"] .machines-tab')).toHaveCount(0);
    // Site's session isn't anywhere.
    await expect(list.locator('[data-panes-host="amy-laptop"][data-panes-pane="4"]')).toHaveCount(0);
    await expect(list).not.toContainText("amy-2");

    // Pinned first, as a card; not again under its machine.
    const pinned = list.locator("[data-machines-pinned]");
    const firsts = await list.locator("> [data-machines-pinned], > [data-machines-group]").evaluateAll((els) => els.map((e) => (e.hasAttribute("data-machines-pinned") ? "pinned" : "machine")));
    expect(firsts[0]).toEqual("pinned");
    await expect(pinned.locator('[data-machines-pinned-session="b0b0000000000001/2"]')).toContainText("on bob-mini");
    await expect(list.locator('> [data-machines-group="b0b0000000000001"] [data-machines-session]')).toHaveCount(1);

    // The pane you had open last is lit, where you were.
    const last = list.locator("[data-panes-last]");
    await expect(last).toHaveCount(1);
    await expect(last).toHaveAttribute("data-panes-pane", String(pane));
    await expect(last).toContainText("where you were");
    await expect(suite.locator("[data-panes-last]")).toHaveCount(1);

    // The quiet ones: one line at the bottom that opens to a list; a teammate's with nothing isn't there.
    const quiet = list.locator("[data-machines-quiet]");
    await expect(quiet.locator("summary")).toHaveText("2 machines with no Arugula sessions");
    await expect(quiet.locator("[data-machines-group]").first()).toBeHidden();
    await quiet.locator("summary").click();
    await expect(quiet.locator("[data-machines-group]")).toHaveCount(2);
    await expect(quiet.locator("[data-machines-machine-link]")).toHaveText(["idle-box", "old-box"]);
    await expect(list).not.toContainText("eve-pc");
    await expect(list.locator("[data-machines-stats]")).toHaveText(/^6 machines · \d+ sessions · \d+ panes · 1 needs you$/);
    await page.screenshot({ path: info.outputPath("machines-desktop.png"), fullPage: true });

    // A session's name opens it on its machine's page.
    await expect(two.locator("[data-machines-session-link]")).toHaveAttribute("href", new RegExp(`/p/${P}[^/]*/machines/a11a000000000001[^?]*\\?pane=1$`));
  });
});

test.describe("on a phone", () => {
  test.use({ viewport: { width: 390, height: 844 }, isMobile: true, hasTouch: true });

  test("one column of cards, no row meta, and nothing wider than the screen", async ({ page }, info) => {
    await stubPins(page, []);
    await openApp(page, () => `/p/${P}/machines`);
    await addMachines(page, HOSTS);
    const list = page.locator("[data-machines-list]");
    const amy = list.locator('> [data-machines-group="a11a000000000001"]');
    await expect(amy.locator("[data-machines-session]")).toHaveCount(2);
    // One column: every card starts at the same x.
    const xs = await amy.locator("[data-machines-session]").evaluateAll((els) => els.map((e) => Math.round(e.getBoundingClientRect().x)));
    expect(new Set(xs).size).toBe(1);
    await expect(list.locator(".panes-row-meta")).toHaveCount(0);
    await list.locator("[data-machines-quiet] summary").click();
    expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBe(await page.evaluate(() => document.documentElement.clientWidth));
    await page.screenshot({ path: info.outputPath("machines-phone.png"), fullPage: true });
  });
});
