// The project's Overview (Lift 4, #1480), in the app's frame over the
// suite's daemon (./overview-fixture.ts): two thirds and one third. The
// left: the title line, Boards as cards (a quiet board folded until it's
// opened, a card's click to its board), Running on to Machines; the old zoom
// addresses (#1287) landing where their things live; the boards' views read
// in one request (#1296), read board by board from a hub without the route,
// and read again only after a long absence (#1297); a card's address
// opening its board's drawer over the page, its session live and its
// question answered there. The right: Needs you (./overview-needs.spec.ts)
// over the agent panel (./panel.spec.ts), whose first tab is the chief of
// staff's room (#1275), its head saying where it runs (#1342), Start with no
// room (#1341), and asleep or led by no one, run elsewhere (#1453, #1472,
// #1500).
// And on a phone, one column with Needs you on top.
import { devices, expect, test } from "@playwright/test";
import { B, chatOn, cli, hookInput, LATER, overview, P, proposing, spyOpened, opened, staffRoomOn, startsOnLaptop, START, suiteHost } from "./overview-fixture";

// A page a route fulfils has no address of its own, so Chrome counts it as
// public and won't let it reach the daemon on loopback without this.
test.use({ permissions: ["local-network-access"] });

test.describe("on a computer", () => {
  test.use({ viewport: { width: 1440, height: 900 } });

  test("two thirds and one third: the title line, Boards with the quiet ones folded, Running; Needs you over the panel on the right", async ({ page }, info) => {
    await overview(page);
    const main = page.locator("[data-ov-main]");
    await expect(main.locator("h1")).toHaveText("Arugula");
    await expect(main.locator("[data-ov-stats]")).toHaveText(/^\d+ working · 0 failing · 0 done today$/);
    // Launch moves: its mark, its bar and count, its words; a click is its board.
    const launch = page.locator(`[data-ov-board="${B}"]`);
    await expect(launch.locator("[data-ov-board-count]")).toHaveText("1/5");
    await expect(launch.locator("[data-ov-board-says]")).not.toBeEmpty();
    await expect(launch.getByRole("progressbar")).toHaveAttribute("aria-valuenow", "1");
    // Later has nothing in flight: folded into one card naming it.
    await expect(page.locator(`[data-ov-board="${LATER}"]`)).toHaveCount(0);
    await expect(page.locator("[data-ov-quiet]")).toHaveText(/1 quiet/);
    await expect(page.locator("[data-ov-quiet-names]")).toHaveText("Later");
    // Running: what runs here, on to Machines.
    await expect(page.locator("[data-ov-running]")).toContainText(/Running\s*\d+ (agent|shell)/);
    // The right: Needs you above the panel, as wide as the panel (420 by default).
    const [m, side] = await Promise.all([main.boundingBox(), page.locator("[data-ov-side]").boundingBox()]);
    expect(side!.x).toBeGreaterThan(m!.x + m!.width);
    expect(Math.round(side!.width)).toBe(420);
    await expect(page.locator("[data-ov-side] [data-panel-col] > [data-ov-needs] + [data-kit=panel]")).toHaveCount(1);
    // Nothing of the old Overview: no zoom pill, no divider, no briefing, no Not on a card.
    await expect(page.locator("[data-ov-levels], [data-ov-split], [data-ov-briefing], [data-ov-loose]")).toHaveCount(0);
    await page.screenshot({ path: info.outputPath("overview.png") });
    await launch.locator("[data-ov-board-title]").click();
    await expect(page).toHaveURL(new RegExp(`/p/${P}[^/]*/boards/${B}`));
    await page.goBack();
    await page.locator("[data-ov-running]").click();
    await expect(page).toHaveURL(new RegExp(`/p/${P}[^/]*/machines`));
  });

  test("the old zoom addresses land where their things live: a board on its board, a card in its drawer here (Arugula #1287)", async ({ page }) => {
    await overview(page);
    await page.goto(`/p/${P}/board/${B}`);
    await expect(page).toHaveURL(new RegExp(`/p/${P}[^/]*/boards/${B}`));
    await page.goto(`/p/${P}/board/${B}/card/row_c`);
    await expect(page).toHaveURL(new RegExp(`/p/${P}[^/?]*\\?board=${B}&card=row_c$`));
    const drawer = page.locator('[data-board-drawer="row_c"]');
    await expect(drawer).toBeVisible();
    await page.keyboard.press("Escape");
    await expect(drawer).toHaveCount(0);
    await expect(page).toHaveURL(new RegExp(`/p/${P}[^/?]*$`));
  });

  test("the boards' views are one read, painted together behind a skeleton, and the page opens no thread (Arugula #1296)", async ({ page }) => {
    const asked: string[] = [];
    page.on("request", (r) => {
      const u = new URL(r.url());
      if (u.pathname.startsWith("/api/") && r.method() !== "OPTIONS") asked.push(`${r.method()} ${u.pathname}`);
    });
    await spyOpened(page);
    let release!: () => void;
    const hold = new Promise<void>((done) => (release = done));
    await overview(page, null, false, undefined, hold);
    // The hub hasn't answered: the list is busy, with no card and no "No open boards".
    await expect(page.locator("[data-ov-boards]")).toHaveAttribute("aria-busy", "true");
    await expect(page.locator("[data-ov-board], [data-ov-empty], [data-ov-quiet]")).toHaveCount(0);
    release();
    // Launch moves; Later is quiet, folded until it's opened.
    await expect(page.locator("[data-ov-board]")).toHaveCount(1);
    await expect(page.locator("[data-ov-boards]")).not.toHaveAttribute("aria-busy");
    await expect(page.locator(`[data-ov-board="${B}"] [data-ov-board-count]`)).toHaveText("1/5");
    await page.locator("[data-ov-quiet]").click();
    await expect(page.locator(`[data-ov-board="${LATER}"] [data-ov-board-count]`)).toHaveText("0/1");
    const mine = (path: RegExp) => asked.filter((a) => path.test(a));
    expect(mine(/^GET \/api\/projects\/[^/]+\/boards\/views$/)).toHaveLength(1);
    expect(mine(/^GET \/api\/boards\//)).toHaveLength(0);
    expect(await opened(page)).toEqual([]);
  });

  test("until the boards' views land the title line says it's reading them, never 0 working (Arugula #1380)", async ({ page }) => {
    let release!: () => void;
    const hold = new Promise<void>((done) => (release = done));
    await overview(page, null, false, undefined, hold);
    const stats = page.locator("[data-ov-stats]");
    await expect(stats).toHaveText("Reading the boards…");
    await expect(stats).toHaveAttribute("aria-busy", "true");
    release();
    await expect(stats).toHaveText(/^\d+ working · 0 failing · 0 done today$/);
    await expect(stats).not.toHaveAttribute("aria-busy");
  });

  test("a hub without the views route (404) is read board by board, each thread opened, the rows still painted together (Arugula #1296)", async ({ page }) => {
    const asked: string[] = [];
    page.on("request", (r) => {
      const u = new URL(r.url());
      if (u.pathname.startsWith("/api/")) asked.push(`${r.method()} ${u.pathname}`);
    });
    await spyOpened(page);
    await overview(page, null, false, undefined, null);
    await expect(page.locator(`[data-ov-board="${B}"] [data-ov-board-count]`)).toHaveText("1/5");
    await expect(page.locator("[data-ov-quiet]")).toHaveText(/1 quiet/);
    expect(asked.filter((a) => a === `GET /api/boards/${B}` || a === `GET /api/boards/${LATER}`).sort()).toEqual([`GET /api/boards/${B}`, `GET /api/boards/${LATER}`].sort());
    expect(await opened(page)).toEqual([B, LATER].sort());
  });


  test("coming back after a short absence reads nothing; after a long one, one read of the boards (Arugula #1297)", async ({ page }) => {
    const asked: string[] = [];
    page.on("request", (r) => {
      const u = new URL(r.url());
      if (u.pathname.startsWith("/api/") && r.method() === "GET") asked.push(u.pathname);
    });
    await overview(page);
    await expect(page.locator("[data-ov-board]")).toHaveCount(1);
    const reads = () => ({
      views: asked.filter((a) => /^\/api\/projects\/[^/]+\/boards\/views$/.test(a)).length,
      boards: asked.filter((a) => a.startsWith("/api/boards/")).length,
    });
    await expect.poll(() => reads().views).toBe(1);
    expect(reads()).toEqual({ views: 1, boards: 0 });
    // The page's clock runs ahead by what the test says: no wait.
    await page.evaluate(() => {
      const w = window as unknown as { __skew: number };
      w.__skew = 0;
      const now = Date.now.bind(Date);
      Date.now = () => now() + w.__skew;
    });
    const show = (state: "hidden" | "visible") =>
      page.evaluate((state) => {
        Object.defineProperty(document, "visibilityState", { value: state, configurable: true });
        document.dispatchEvent(new Event("visibilitychange"));
      }, state);
    const away = async (ms: number) => {
      await show("hidden");
      await page.evaluate((ms) => ((window as unknown as { __skew: number }).__skew += ms), ms);
      await show("visible");
    };
    // Out for 29 s: the live pushes kept running, so nothing is read.
    await away(29_000);
    // A read after it would have been asked by now: the page answers a round trip first.
    await page.evaluate(() => fetch("/api/projects").catch(() => {}));
    expect(reads()).toEqual({ views: 1, boards: 0 });
    // Out for 30 s: one read of every board.
    await away(30_000);
    await expect.poll(() => reads()).toEqual({ views: 2, boards: 0 });
    await page.evaluate(() => fetch("/api/projects").catch(() => {}));
    expect(reads()).toEqual({ views: 2, boards: 0 });
  });

  test("a card's address opens the board's drawer over the Overview, drawing its session live and its question at the top, as on the board", async ({ page }, info) => {
    const { term } = await overview(page, null, true);
    await page.goto(`/p/${P}?board=${B}&card=row_b`);
    const drawer = page.locator('[data-board-drawer="row_b"]');
    const live = drawer.locator("[data-board-drawer-live]");
    await expect(live.locator(`[data-live-slot="${term}"] .xterm`)).toBeVisible({ timeout: 20_000 });
    // The session's question, answered at the drawer's top.
    await page.evaluate(
      ({ term, line }) => fetch(`/api/panes/${term}/send`, { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ text: line, enter: true }) }),
      { term, line: `out=$(${cli} ask < ${hookInput}); echo "$out" | grep -o '"Which [a-z ]*?":"[^"]*"'` },
    );
    const ask = drawer.locator("[data-board-drawer-ask]");
    await expect(ask).toBeVisible({ timeout: 20_000 });
    await expect(ask.locator("h3")).toHaveText("Claude Code asks");
    await page.screenshot({ path: info.outputPath("overview-drawer-live.png") });
    await ask.locator('[data-question="0"]').getByRole("radio", { name: /^Blue/ }).click();
    await ask.locator('[data-question="1"]').getByRole("checkbox", { name: /Pear/ }).check();
    await ask.locator('[data-question="2"]').getByRole("radio", { name: /^Cat/ }).click();
    await ask.getByRole("button", { name: "Submit" }).click();
    await expect(ask).toHaveCount(0);
    await expect.poll(() => page.evaluate(async (p) => (await fetch(`/api/panes/${p}/capture?format=text`)).text(), term)).toContain('"Which colour do you prefer?":"Blue"');
  });


  test("the chief of staff is the panel's first tab, pinned: its staff room, its head saying where it runs and whether it's awake (Arugula #1275, #1342)", async ({ page }) => {
    await overview(page);
    const panel = page.locator("[data-kit=panel]");
    await page.waitForFunction(() => (window as unknown as { __chat?: { on: boolean | null } }).__chat?.on === false);
    // Chat off: the tab is there, and says rooms come with chat.
    const tab = panel.locator('[data-tab="chief"]');
    await expect(tab).toHaveText("Chief of staff");
    await expect(panel.locator("[data-panel-chat-off]")).toBeVisible();
    await page.evaluate((P) => {
      const room = { id: "dm_cos", kind: "dm", team: "t", name: "Chief of staff · Arugula", machine: null, machine_name: null, target: null, project: P, with: null, others: [], private: false, last: 1, read: 0, unread: 1, mentions: 0, wants: 1, muted: false, level: "all", level_own: false, archived: false, writable: true, topic: "", description: "", pins: 0, topic_writable: true, staff: true, lead: { machine: "m_geek", machine_name: "geek", session: "cos" } };
      const said = [{ seq: 1, at: Date.now() - 60_000, author: "acct_me", name: "Me's agent on geek", text: "3 boards moved since this morning.", quote: null, pane: null, mentions: [], edited: null, deleted: null, imported: null, agent: { machine: "m_geek", machine_name: "geek" }, reactions: [], files: [], pinned: false, also: false }];
      const json = (body: unknown) => new Response(JSON.stringify(body), { headers: { "Content-Type": "application/json" } });
      const before = window.fetch;
      window.fetch = async (input, init) => {
        const url = new URL(typeof input === "string" ? input : input instanceof URL ? input.href : input.url, location.href);
        if (url.pathname === "/api/chat/rooms/dm_cos/messages") return json({ room, messages: said, more: false });
        return before(input, init);
      };
      const c = (window as unknown as { __chat: { on: boolean | null; rooms: unknown[]; emit(): void } }).__chat;
      c.on = true;
      c.rooms = [...c.rooms, room];
      c.emit();
    }, P);
    await expect(tab).toHaveAttribute("aria-selected", "true");
    await expect(panel.locator('[data-tab-x="chief"]')).toHaveCount(0);
    await expect(panel.locator("[data-tab]").first()).toHaveAttribute("data-tab", "chief");
    await expect(panel.locator('[data-ov-staff="dm_cos"]')).toContainText("3 boards moved since this morning.");
    // Its head (#1342): a machine this page doesn't reach tells only where it runs.
    const head = panel.locator("[data-panel-head]");
    await expect(head).toHaveAttribute("data-panel-head", "Chief of staff");
    await expect(head).toHaveAttribute("data-panel-state", "unknown");
    await expect(head).toContainText("chief of staff");
    await expect(head).toContainText("geek");
    // Led by the project's scratch session on the suite's daemon, which this page reads: idle there, its shell quiet.
    const host = await suiteHost(page);
    await page.evaluate((host) => {
      const c = (window as unknown as { __chat: { rooms: { id: string; lead?: unknown }[]; emit(): void } }).__chat;
      c.rooms = c.rooms.map((r) => (r.id === "dm_cos" ? { ...r, lead: { machine: "m_suite", machine_name: host, session: "scratch" } } : r));
      c.emit();
    }, host);
    await expect(head).toHaveAttribute("data-panel-state", "idle");
    await expect(head).toContainText(host);
    await expect(head.locator('.k-panel-meta[title$="wakes on its own"]')).toHaveCount(1);
    // No session leads it: none.
    await page.evaluate(() => {
      const c = (window as unknown as { __chat: { rooms: { id: string; lead?: unknown }[]; emit(): void } }).__chat;
      c.rooms = c.rooms.map((r) => (r.id === "dm_cos" ? { ...r, lead: undefined } : r));
      c.emit();
    });
    await expect(head).toHaveAttribute("data-panel-state", "none");
    await expect(head).toContainText("no one leads it");
  });

  test("with no staff room, the chief of staff's tab is Start: what it does and this computer to run it on; Start calls start_agent there, and the tab becomes its room with its head (Arugula #1341)", async ({ page }) => {
    await overview(page);
    const panel = page.locator("[data-kit=panel]");
    // Chat off: no staff room to have, so no Start.
    await page.waitForFunction(() => (window as unknown as { __chat?: { on: boolean | null } }).__chat?.on === false);
    await expect(panel.locator("[data-ov-staff-start]")).toHaveCount(0);
    await chatOn(page);
    const tab = panel.locator('[data-tab="chief"]');
    await expect(tab).toHaveText("Chief of staff");
    await expect(tab).toHaveAttribute("aria-selected", "true");
    await expect(panel.locator('[data-tab-x="chief"]')).toHaveCount(0);
    await expect(panel.locator("[data-panel-head]")).toHaveAttribute("data-panel-state", "none");
    const start = panel.locator("[data-ov-staff-start]");
    await expect(start.getByRole("heading")).toHaveText("Start your chief of staff");
    // The suite's daemon is the only machine, this page's own, and new enough.
    const host = await suiteHost(page);
    await expect(start.locator("[data-ov-staff-machine-one]")).toHaveText(`On ${host} (this computer)`);

    // Start asks the machine, over this page's connection to it, as its owner: start_agent, recipe chief-of-staff, in the project.
    const calls: unknown[] = [];
    page.on("request", (r) => void (new URL(r.url()).pathname === "/api/mcp/call" && calls.push(r.postDataJSON())));
    // This daemon has joined no Arugula cloud, so it can't choose the project: it says so, and nothing starts.
    const paneCount = () => page.evaluate(async () => ((await (await fetch("/api/panes")).json()) as unknown[]).length);
    const before = await paneCount();
    await start.locator("[data-ov-staff-go]").click();
    await expect(start.locator("[data-ov-staff-failed]")).toBeVisible();
    await expect(start.locator("[data-ov-staff-failed]")).not.toBeEmpty();
    expect(calls).toEqual([{ tool: "start_agent", arguments: { agent: "claude", recipe: "chief-of-staff", project: P, prompt: "" }, client: "Overview" }]);
    expect(await paneCount()).toBe(before);
    await expect(start.locator("[data-ov-staff-go]")).toBeEnabled();

    // On a joined machine it starts the block and makes it the staff room's lead (#1338): the hub's staff room then has it.
    await page.route("**/api/mcp/call", (r) =>
      r.fulfill({ json: { content: [{ type: "text", text: "Started an agent in %9" }], structuredContent: { block: 9, chief_of_staff: { project: P, session: "scratch", lead: true, said: "It leads your person's staff room in Arugula" } } } }),
    );
    await page.evaluate(
      ({ P, host }) => {
        const room = { id: "dm_cos", kind: "dm", team: "t", name: "Chief of staff · Arugula", machine: null, machine_name: null, target: null, project: P, with: null, others: [], private: false, last: 0, read: 0, unread: 0, mentions: 0, wants: 0, muted: false, level: "all", level_own: false, archived: false, writable: true, topic: "", description: "", pins: 0, topic_writable: true, staff: true, lead: { machine: "m_suite", machine_name: host, session: "scratch" } };
        const json = (body: unknown) => new Response(JSON.stringify(body), { headers: { "Content-Type": "application/json" } });
        const before = window.fetch;
        window.fetch = async (input, init) => {
          const url = new URL(typeof input === "string" ? input : input instanceof URL ? input.href : input.url, location.href);
          if (url.pathname === `/api/chat/projects/${P}/staff` && init?.method === "POST") return json(room);
          if (url.pathname === "/api/chat/rooms/dm_cos/messages") return json({ room, messages: [], more: false });
          return before(input, init);
        };
      },
      { P, host },
    );
    await start.locator("[data-ov-staff-go]").click();
    // The tab is the room's now, still first and open, its head saying where the chief of staff runs.
    await expect(tab).toHaveAttribute("aria-selected", "true");
    await expect(panel.locator("[data-tab]").first()).toHaveAttribute("data-tab", "chief");
    await expect(panel.locator("[data-ov-staff-start]")).toHaveCount(0);
    await expect(panel.locator('[data-ov-staff="dm_cos"]')).toBeVisible();
    await expect(panel.locator("[data-panel-head]")).toHaveAttribute("data-panel-state", "idle");
    await expect(panel.locator("[data-panel-head]")).toContainText(host);
    expect(calls).toHaveLength(2);
  });

  test("a chief of staff asleep on an offline machine: its tab offers Run it on another machine, which asks for the grant, starts it on the machine picked and leaves the room led there (Arugula #1453)", async ({ page }) => {
    const { host, head } = await staffRoomOn(page, { machine: "m_pi", machine_name: "pi", session: "Chief of staff · Arugula" });
    const panel = page.locator("[data-kit=panel]");
    await expect(head).toHaveAttribute("data-panel-state", "asleep");
    await expect(head).toContainText("pi is offline");
    // The offer, over the room: the reader's machines online to run it on, this computer first and picked; pi (asleep) not among them.
    await expect(panel.locator('[data-ov-staff="dm_cos"]')).toBeVisible();
    const move = panel.locator('[data-ov-staff-move="pi"]');
    await expect(move).toBeVisible();
    await expect(move).toContainText("the one on pi stands down");
    const machine = move.locator("[data-ov-staff-machine]");
    await expect(machine.locator("option")).toHaveText([`${host} (this computer)`, "laptop"]);
    await expect(machine).toHaveValue(host);
    await expect(move.locator("[data-ov-staff-cannot]")).toHaveCount(0);
    await expect(move.locator("[data-ov-staff-go]")).toHaveText("Run it on another machine");
    // Restart is for one that runs: not while it's asleep.
    await expect(page.locator("[data-ov-staff-restart]")).toHaveCount(0);
    await machine.selectOption("laptop");
    await startsOnLaptop(page, move, head, "It leads your person's staff room in Arugula. Session Chief of staff · Arugula on pi led it before: it's told to stand down");
    await expect(panel.locator('[data-tab="chief"]')).toHaveAttribute("aria-selected", "true");
  });

  test("a staff room no session leads offers the same pick with Start, which starts it on the machine picked and leaves the room led there (Arugula #1472)", async ({ page }) => {
    const { host, head } = await staffRoomOn(page, null);
    const panel = page.locator("[data-kit=panel]");
    await expect(head).toHaveAttribute("data-panel-state", "none");
    // The offer, over the room: the reader's machines online, this computer first and picked; pi (offline) left out, and said so.
    await expect(panel.locator('[data-ov-staff="dm_cos"]')).toBeVisible();
    const move = panel.locator('[data-ov-staff-lead="none"]');
    await expect(move).toBeVisible();
    await expect(move).toContainText("No chief of staff leads this room");
    const machine = move.locator("[data-ov-staff-machine]");
    await expect(machine.locator("option")).toHaveText([`${host} (this computer)`, "laptop"]);
    await expect(machine).toHaveValue(host);
    await expect(move.locator("[data-ov-staff-cannot]")).toHaveText("Not listed: pi (offline).");
    await expect(move.locator("[data-ov-staff-go]")).toHaveText("Start");
    await machine.selectOption("laptop");
    await startsOnLaptop(page, move, head, "It leads your person's staff room in Arugula");
    await expect(panel.locator('[data-tab="chief"]')).toHaveAttribute("aria-selected", "true");
  });

  test("a running chief of staff has Restart: after one confirm it starts a new one on the same machine through Start's path, closes nothing itself, and the room ends led by the new block (Arugula #1500)", async ({ page }) => {
    const { head } = await staffRoomOn(page, { machine: "m_laptop", machine_name: "laptop", session: "old chief" });
    const panel = page.locator("[data-kit=panel]");
    await expect(head).toHaveAttribute("data-panel-state", "idle");
    await expect(head).toContainText("laptop");
    const restart = panel.locator('[data-ov-staff-restart="laptop"]');
    await expect(panel.locator("[data-ov-staff-move]")).toHaveCount(0);
    const calls: { tool: string; arguments: unknown }[] = [];
    await page.route("**/api/mcp/call", (r) => {
      calls.push(r.request().postDataJSON());
      return r.fulfill({ json: { content: [{ type: "text", text: "Started an agent in %9" }], structuredContent: { block: 9, chief_of_staff: { project: P, session: "Chief of staff · Arugula", lead: true, said: "It leads your person's staff room in Arugula", closed: [4] } } } });
    });
    await page.route("**/api/grants", (r) => r.fulfill({ json: r.request().method() === "POST" ? {} : { grants: [] } }));
    const lead = () => page.evaluate(() => (window as unknown as { __chat: { rooms: { staff?: boolean; lead: { session: string; machine_name: string } }[] } }).__chat.rooms.find((x) => x.staff)!.lead);
    // One confirm first: Cancel starts nothing.
    await restart.locator("[data-ov-staff-restart-open]").click();
    await expect(restart.locator("[data-ov-staff-confirm]")).toContainText("Restart your chief of staff?");
    await restart.locator("[data-ov-staff-restart-cancel]").click();
    await expect(restart.locator("[data-ov-staff-confirm]")).toHaveCount(0);
    expect(calls).toHaveLength(0);
    await restart.locator("[data-ov-staff-restart-open]").click();
    await restart.locator("[data-ov-staff-restart-go]").click();
    await restart.locator("[data-ov-staff-allow]").click();
    await expect.poll(() => calls.map((c) => c.tool)).toEqual(["start_agent"]);
    expect(calls[0].arguments).toEqual({ agent: "claude", recipe: "chief-of-staff", project: P, prompt: "" });
    expect(await page.evaluate(() => (window as unknown as { __asked: string[] }).__asked)).toEqual(["laptop"]);
    await expect.poll(lead).toEqual({ machine: "m_laptop", machine_name: "laptop", session: "Chief of staff · Arugula" });
    await expect(head).toHaveAttribute("data-panel-state", "idle");
    await expect(restart.locator("[data-ov-staff-restart-open]")).toBeEnabled();
  });

  test("a Restart that fails to start says why and leaves the room led by the running chief of staff (Arugula #1500)", async ({ page }) => {
    const { head } = await staffRoomOn(page, { machine: "m_laptop", machine_name: "laptop", session: "old chief" });
    const restart = page.locator('[data-kit=panel] [data-ov-staff-restart="laptop"]');
    const calls: { tool: string }[] = [];
    await page.route("**/api/mcp/call", (r) => {
      calls.push(r.request().postDataJSON());
      return r.fulfill({ json: { isError: true, content: [{ type: "text", text: "no agent adapter here" }] } });
    });
    await page.route("**/api/grants", (r) => r.fulfill({ json: r.request().method() === "POST" ? {} : { grants: [] } }));
    await restart.locator("[data-ov-staff-restart-open]").click();
    await restart.locator("[data-ov-staff-restart-go]").click();
    await restart.locator("[data-ov-staff-allow]").click();
    await expect(restart.locator("[data-ov-staff-failed]")).toHaveText("no agent adapter here");
    expect(calls.map((c) => c.tool)).toEqual(["start_agent"]);
    const session = await page.evaluate(() => (window as unknown as { __chat: { rooms: { staff?: boolean; lead: { session: string } }[] } }).__chat.rooms.find((x) => x.staff)!.lead.session);
    expect(session).toBe("old chief");
    await expect(head).toHaveAttribute("data-panel-state", "idle");
    await expect(head).toContainText("laptop");
  });

  test("a proposal's message in the staff room is its card in the staff tab; answered, a line (Arugula #1278)", async ({ page }) => {
    await overview(page);
    await proposing(page, START);
    const staff = page.locator('[data-ov-staff="dm_cos"]');
    const card = staff.locator('[data-proposal="prp_start"]');
    await expect(card).toHaveAttribute("data-ask", "2");
    await expect(card.locator("[data-proposal-approve]")).toBeVisible();
    // Its plain text is only the fallback.
    await expect(staff).not.toContainText("Your chief of staff asks to start an agent");
    await expect(page.locator('[data-kit=panel] [data-tab="chief"]')).toHaveAttribute("aria-selected", "true");
    await card.locator("[data-proposal-approve]").click();
    const done = staff.locator('[data-proposal="prp_start"][data-ask-state="answered"]');
    await expect(done).toHaveText("Sam approved: Started a session on #12. Its session");
    await expect(done.locator("[data-proposal-started]")).toHaveAttribute("href", /ses_started/);
  });

test("the team's faces are in the header while chat is on", async ({ page }) => {
    await overview(page);
    await expect(page.locator("[data-project-faces]")).toHaveCount(0);
    await page.waitForFunction(() => (window as unknown as { __chat?: { on: boolean | null } }).__chat?.on === false);
    await page.evaluate(() => {
      const c = (window as unknown as { __chat: { on: boolean | null; emit(): void } }).__chat;
      c.on = true;
      c.emit();
    });
    const faces = page.locator(`[data-project-home="${P}"] [data-ov-title] [data-project-faces]`);
    await expect(faces.locator("[data-project-face]")).toHaveCount(2);
    await expect(faces.locator('[data-project-face="acct_b"]')).toHaveAttribute("title", "Jo");
  });
});

test.describe("on a phone", () => {
  const { viewport, userAgent, deviceScaleFactor, isMobile, hasTouch } = devices["Pixel 7"];
  test.use({ viewport, userAgent, deviceScaleFactor, isMobile, hasTouch });

  test("the Overview is one column, with Needs you on top", async ({ page }, info) => {
    await overview(page);
    const ov = page.locator("[data-overview]");
    await expect(ov).toHaveClass(/phone/);
    const first = await ov.locator("> *").first().evaluate((e) => [...e.attributes].map((a) => a.name));
    expect(first).toContain("data-ov-needs");
    await expect(page.locator(`[data-ov-board="${B}"]`)).toBeVisible();
    const fits = await ov.evaluate((e) => e.scrollWidth <= e.clientWidth);
    expect(fits).toBe(true);
    await page.screenshot({ path: info.outputPath("overview-phone.png") });
  });
});
