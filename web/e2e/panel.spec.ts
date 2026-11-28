// The agent panel (Lift 4, #1480) on the Overview, in the app's frame over
// the suite's daemon (./overview-fixture.ts): the chief of staff first, a
// peek that follows the selection in Needs you, the widths (`[` `]` `\`,
// remembered), `l` and `h` moving focus, the Terminal (`v`) live in a well,
// the floor (`f`: the body over the page; ⤢ or Esc out), an ended session
// read only, a message's replies opening in the panel (one Back, Follow in
// its ⋯) and back to the same place, a tab's ×, and a picture dropped on
// an agent's composer reaching it (#1557).
import { expect, test, type Page } from "@playwright/test";
import { overview, P, paneItem, proposing, say, START, suiteHost } from "./overview-fixture";

test.use({ permissions: ["local-network-access"], viewport: { width: 1440, height: 900 } });

const panel = (page: Page) => page.locator("[data-kit=panel]");
const col = (page: Page) => page.locator("[data-panel-col]");
const row = (page: Page, id: string) => page.locator(`[data-need-item="${id}"]`);
/** Nothing focused: the page's keys are the table's. */
const blur = (page: Page) => page.evaluate(() => (document.activeElement as HTMLElement | null)?.blur());

/** The suite's daemon by control's id, `m_suite`, as the hub lists it: its panes' tabs name it, and the app's live panes reach it. */
async function suiteIsListed(page: Page) {
  await page.route("**/api/hosts", async (r) => {
    const list = (await (await r.fetch()).json()) as { this: string; hosts: unknown[] };
    await r.fulfill({ json: { ...list, hosts: [...list.hosts, { name: list.this, id: "m_suite", urls: [], transport: "tailnet", added_ms: 0, last_seen_ms: null }] } });
  });
}

/** A pane's tab key, as the panel keys it, once the page's fleet has the pane. */
async function keyOf(page: Page, pane: number): Promise<string> {
  let key = "";
  await expect
    .poll(
      async () =>
        (key = await page.evaluate((id) => {
          const f = (window as unknown as { __app: { fleet: { panes: { id: number; host: string; session: { name: string } | null }[]; host(n: string): { id?: string } | undefined } } }).__app.fleet;
          const p = f.panes.find((x) => x.id === id);
          return p ? `agent:${f.host(p.host)?.id ?? p.host}:${p.session ? p.session.name : `%${p.id}`}` : "";
        }, pane)),
      { timeout: 15_000 },
    )
    .not.toBe("");
  return key;
}

/** A made-up agent pane of the reader's, pane 1 of `host` in session `session`, in the project: the fleet takes it as it takes a fake one. */
async function inject(page: Page, host: string, session: string, info: Record<string, unknown>) {
  await page.evaluate(
    ({ P, host, session, info }) => {
      const fleet = (window as unknown as { __app: { fleet: { inject(p: unknown[]): void } } }).__app.fleet;
      fleet.inject([
        {
          key: `${host}:1`,
          host,
          id: 1,
          session: { id: 1, name: session },
          stale: false,
          person: { id: "me", name: "me", kind: "me" },
          driver: null,
          watchers: [],
          unread: 0,
          mention: false,
          info: { id: 1, title: "claude", kind: "agent", type: "agent", session_project: { id: P, name: "Arugula" }, activity: { bps: 0, last_ms: Date.now() }, ...info },
        },
      ]);
    },
    { P, host, session, info },
  );
}

const APPROVAL = {
  id: "prp_x",
  kind: "approval",
  group: "decide",
  counts: true,
  for: "acct_a",
  title: "Start a worker on #12 on geek?",
  from: { name: "Chief of staff · Arugula" },
  subject: { type: "proposal", id: "prp_x", url: `/p/${P}/chat`, project: P },
  since: 3,
  answer: { via: "card", options: ["Start", "Not now"] },
};

test("the peek follows the selection: j and k put each asker in front, the chief of staff for its own asks, and the address stays", async ({ page }, info) => {
  const { agent } = await overview(page);
  const host = await suiteHost(page);
  const a = await keyOf(page, agent);
  // A second asker, in a session of its own on another machine of the reader's.
  await inject(page, "ws-3", "migrate", { attention: "needs_input" });
  const s = "agent:ws-3:migrate";
  await say(page, [{ ...paneItem(host, agent), title: "Which colour?", since: 1 }, { ...paneItem("ws-3", 1), title: "Run the migration?", since: 2 }, APPROVAL]);
  await expect(row(page, APPROVAL.id)).toBeVisible();
  const url = page.url();
  // The chief of staff is first, and in front.
  await expect(panel(page).locator("[data-tab]").first()).toHaveAttribute("data-tab", "chief");
  await expect(panel(page).locator('[data-tab="chief"]')).toHaveAttribute("aria-selected", "true");
  await blur(page);
  await page.keyboard.press("j");
  await expect(panel(page).locator(`[data-tab="${a}"]`)).toHaveAttribute("aria-selected", "true");
  // A visitor's tab is in italics, with its ×.
  await expect(panel(page).locator(`[data-tab="${a}"]`)).toHaveClass(/is-peek/);
  await expect(panel(page).locator(`[data-tab-x="${a}"]`)).toBeVisible();
  await expect(panel(page).locator("[data-panel-transcript]")).toContainText("hello", { timeout: 20_000 });
  await expect(panel(page).locator("[data-kit=composer]")).toBeVisible();
  await page.screenshot({ path: info.outputPath("panel-worker.png") });
  await page.keyboard.press("j");
  await expect(panel(page).locator(`[data-tab="${s}"]`)).toHaveAttribute("aria-selected", "true");
  await page.keyboard.press("j");
  await expect(panel(page).locator('[data-tab="chief"]')).toHaveAttribute("aria-selected", "true");
  await page.keyboard.press("k");
  await expect(panel(page).locator(`[data-tab="${s}"]`)).toHaveAttribute("aria-selected", "true");
  expect(page.url()).toBe(url);
});

test("a picture dropped on an agent's composer is outlined while it's dragged, then goes with the message to the agent (#1557)", async ({ page }) => {
  const { agent } = await overview(page);
  const host = await suiteHost(page);
  const a = await keyOf(page, agent);
  await say(page, [{ ...paneItem(host, agent), title: "Which colour?", since: 1 }]);
  await row(page, `${host}.${agent}:1`).locator("button[aria-expanded]").first().click();
  await expect(panel(page).locator(`[data-tab="${a}"]`)).toHaveAttribute("aria-selected", "true");
  const box = panel(page).locator("[data-kit=composer]");
  const drag = (type: string) =>
    box.evaluate((el, type) => {
      const dt = new DataTransfer();
      dt.items.add(new File([new Uint8Array([137, 80, 78, 71, 13, 10, 26, 10])], "shot.png", { type: "image/png" }));
      el.dispatchEvent(new DragEvent(type, { dataTransfer: dt, bubbles: true, cancelable: true }));
    }, type);
  await drag("dragover");
  await expect(box).toHaveClass(/dropping/);
  await drag("drop");
  await expect(box).not.toHaveClass(/dropping/);
  await expect(box.locator('[data-attach-chip="shot.png"]')).toBeVisible();
  await box.locator("textarea").fill("look");
  await box.locator("textarea").press("Enter");
  // The fake agent takes no images, so it's given the file's path on its machine.
  await expect(panel(page).locator("[data-panel-transcript]")).toContainText(/Saw 0 image\(s\) \[\]; text \[.*\.png/, { timeout: 20_000 });
  await expect(box.locator("[data-attach-chip]")).toHaveCount(0);
});

test("widths: 420 by default, [ narrower to 320, ] wider to 600, \\ a strip and back; kept across a reload", async ({ page }) => {
  await overview(page);
  await expect(col(page)).toHaveAttribute("data-panel-width", "420");
  const width = async () => Math.round((await col(page).boundingBox())!.width);
  expect(await width()).toBe(420);
  await blur(page);
  await page.keyboard.press("[");
  await expect(col(page)).toHaveAttribute("data-panel-width", "320");
  expect(await width()).toBe(320);
  await page.keyboard.press("[");
  await expect(col(page)).toHaveAttribute("data-panel-width", "320");
  await page.keyboard.press("]");
  await page.keyboard.press("]");
  await expect(col(page)).toHaveAttribute("data-panel-width", "600");
  expect(await width()).toBe(600);
  await page.keyboard.press("\\");
  await expect(col(page)).toHaveAttribute("data-panel-shut", "");
  await expect(panel(page)).toHaveCount(0);
  expect(await width()).toBeLessThan(60);
  await page.keyboard.press("\\");
  await expect(col(page)).toHaveAttribute("data-panel-width", "600");
  // Kept for the person, across a reload.
  await page.reload();
  await expect(col(page)).toHaveAttribute("data-panel-width", "600", { timeout: 20_000 });
  // The strip's button opens it too.
  await page.locator("[data-panel-collapse]").click();
  await page.locator("[data-panel-open]").click();
  await expect(panel(page)).toBeVisible();
});

test("l focuses the panel; h is unbound by default (#1534)", async ({ page }) => {
  await overview(page);
  await blur(page);
  await page.keyboard.press("l");
  await expect(panel(page)).toBeFocused();
  await page.keyboard.press("h");
  await expect(panel(page)).toBeFocused();
  // Every one of the panel's bound keys is in the table, so `?` lists it.
  const ids = await page.evaluate(() => (window as unknown as { __arugulaKeys: () => string[] }).__arugulaKeys());
  for (const id of ["panel-focus", "panel-staff", "panel-view", "panel-floor", "panel-narrow", "panel-wide", "panel-collapse", "panel-back"]) expect(ids).toContain(id);
  expect(ids).not.toContain("panel-work");
});

test("v shows the peeked session's pane live in a well, and back to its transcript", async ({ page }) => {
  await suiteIsListed(page);
  const { term } = await overview(page);
  const host = await suiteHost(page);
  const key = await keyOf(page, term);
  expect(key).toMatch(/^agent:m_suite:/);
  await say(page, [{ ...paneItem(host, term), title: "Run the migration?" }]);
  await row(page, `${host}.${term}:1`).locator("button[aria-expanded]").first().click();
  await expect(panel(page).locator(`[data-tab="${key}"]`)).toHaveAttribute("aria-selected", "true");
  await expect(panel(page).locator('[data-panel-view="transcript"]')).toHaveAttribute("aria-pressed", "true");
  await blur(page);
  await page.keyboard.press("v");
  await expect(panel(page).locator('[data-panel-view="terminal"]')).toHaveAttribute("aria-pressed", "true");
  await expect(panel(page).locator(`[data-kit=well] [data-panel-terminal="${term}"] .xterm`)).toBeVisible({ timeout: 20_000 });
  await blur(page);
  await page.keyboard.press("v");
  await expect(panel(page).locator("[data-panel-transcript]")).toBeVisible();
  await expect(panel(page).locator("[data-panel-terminal]")).toHaveCount(0);
});

test("s flips to the chief of staff's tab, and back to the tab before (#1534)", async ({ page }) => {
  await suiteIsListed(page);
  const { term } = await overview(page);
  const host = await suiteHost(page);
  const key = await keyOf(page, term);
  await say(page, [{ ...paneItem(host, term), title: "Run the migration?" }]);
  await row(page, `${host}.${term}:1`).locator("button[aria-expanded]").first().click();
  await expect(panel(page).locator(`[data-tab="${key}"]`)).toHaveAttribute("aria-selected", "true");
  await blur(page);
  await page.keyboard.press("s");
  await expect(panel(page).locator('[data-tab="chief"]')).toHaveAttribute("aria-selected", "true");
  await expect(panel(page).locator("[data-panel-head]")).toHaveAttribute("data-panel-head", "Chief of staff");
  await blur(page);
  await page.keyboard.press("s");
  await expect(panel(page).locator(`[data-tab="${key}"]`)).toHaveAttribute("aria-selected", "true");
});

test("the chief of staff's room, then the floor: f puts its body over the page, and ⤢ or Esc leaves it", async ({ page }, info) => {
  await overview(page);
  await proposing(page, START);
  const staff = panel(page).locator('[data-ov-staff="dm_cos"]');
  await expect(staff.locator('[data-msg="1"]')).toBeVisible();
  await expect(panel(page).locator("[data-panel-head]")).toHaveAttribute("data-panel-head", "Chief of staff");
  // Its room writes in the kit's composer, which says where it goes.
  await expect(staff.locator("[data-kit=composer]")).toContainText("staff room");
  await page.screenshot({ path: info.outputPath("panel-chief.png") });
  await blur(page);
  await page.keyboard.press("f");
  await expect(col(page)).toHaveAttribute("data-panel-floor", "");
  // Only the body: the room over the page; the panel, its tabs and head, stay in their column.
  const box = (await panel(page).locator("[data-panel-body]").boundingBox())!;
  expect(box.width).toBeGreaterThan(1300);
  expect(box.height).toBeGreaterThan(800);
  expect((await panel(page).boundingBox())!.width).toBeLessThan(700);
  await expect(panel(page).locator("[data-panel-floor-toggle]")).toHaveCount(1);
  await page.screenshot({ path: info.outputPath("panel-floor.png") });
  const url = page.url();
  await page.keyboard.press("Escape");
  await expect(col(page)).not.toHaveAttribute("data-panel-floor", "");
  expect(page.url()).toBe(url);
  // ⤢ does the same: the head's goes in, the floor's comes out.
  await panel(page).locator("[data-panel-floor-toggle]").click();
  await expect(col(page)).toHaveAttribute("data-panel-floor", "");
  await panel(page).locator("[data-panel-floor-toggle]").click();
  await expect(col(page)).not.toHaveAttribute("data-panel-floor", "");
});

test("an ended session is read only: its transcript ends in when it ended, and on a to-do the composer is Done's line", async ({ page }, info) => {
  await overview(page);
  const ended = Date.now() - 3 * 3600_000 - 60_000;
  await inject(page, "ws-2", "staging-checkout", { title: "#1431 staging checkout", attention: "idle", activity: { bps: 0, last_ms: ended }, reason: { kind: "exited", since_ms: ended, headline: "claude exited" } });
  const todo = {
    id: "todo_1431",
    kind: "todo",
    group: "decide",
    counts: true,
    for: "acct_a",
    title: "Add STRIPE_KEY to the staging secrets",
    from: { name: "claude" },
    subject: { type: "pane", id: "ws-2.1", url: "/", project: P, machine: "ws-2", pane: "1" },
    since: ended,
    lines: ["Why: checkout on staging fails without it."],
    answer: { via: "todo", options: ["Done", "Can't"] },
  };
  await say(page, [todo]);
  await row(page, todo.id).locator("button[aria-expanded]").first().click();
  await expect(panel(page).locator('[data-tab="agent:ws-2:staging-checkout"]')).toHaveAttribute("aria-selected", "true");
  await expect(panel(page).locator("[data-panel-head]")).toHaveAttribute("data-panel-state", "ended");
  await expect(panel(page).locator("[data-panel-ended]")).toHaveText("read-only · session ended 3h ago");
  await expect(panel(page).locator("[data-kit=composer]")).toHaveCount(0);
  await expect(panel(page).locator("[data-panel-todo]")).toHaveText("Done continues this conversation with your note.");
  await page.screenshot({ path: info.outputPath("panel-ended.png") });
});

/** The reader's staff room, 40 messages long, the 30th with two replies in its thread. */
async function staffWithThread(page: Page) {
  await page.waitForFunction(() => (window as unknown as { __chat?: { on: boolean | null } }).__chat?.on === false);
  await page.evaluate((P) => {
    const room = { id: "dm_cos", kind: "dm", team: "t", name: "Chief of staff · Arugula", machine: null, machine_name: null, target: null, project: P, with: null, others: [], private: false, last: 40, read: 40, unread: 0, mentions: 0, wants: 0, muted: false, level: "all", level_own: false, archived: false, writable: true, topic: "", description: "", pins: 0, topic_writable: true, staff: true, lead: { machine: "m_geek", machine_name: "geek", session: "cos" } };
    const now = Date.now();
    const msg = (seq: number, text: string, more = {}) => ({ seq, at: now - (50 - seq) * 60_000, author: "acct_a", name: "Sam's agent on geek", text, quote: null, pane: null, mentions: [], edited: null, deleted: null, imported: null, agent: { machine: "m_geek", machine_name: "geek" }, reactions: [], files: [], pinned: false, also: false, ...more });
    const thread = { replies: 2, last: now - 60_000, repliers: [{ account: "acct_a", name: "Sam" }], following: false, read: 0, unread: 0, mentions: 0 };
    const list = Array.from({ length: 40 }, (_, i) => msg(i + 1, i + 1 === 30 ? "Launch moved: #12 is in review." : `Line ${i + 1} of the morning's news.`, i + 1 === 30 ? { thread } : {}));
    const replies = [msg(41, "Which reviewer?", { parent: 30 }), msg(42, "Jo, as last time.", { parent: 30 })];
    const json = (body: unknown) => new Response(JSON.stringify(body), { headers: { "Content-Type": "application/json" } });
    const before = window.fetch;
    window.fetch = async (input, init) => {
      const url = new URL(typeof input === "string" ? input : input instanceof URL ? input.href : input.url, location.href);
      if (url.pathname === "/api/chat/rooms/dm_cos/messages" && url.searchParams.get("thread") === "30") return json({ room, parent: list[29], messages: url.searchParams.has("after") ? [] : replies, more: false });
      if (url.pathname === "/api/chat/rooms/dm_cos/messages") return json({ room, messages: list, more: false });
      return before(input, init);
    };
    const c = (window as unknown as { __chat: { on: boolean | null; rooms: unknown[]; emit(): void } }).__chat;
    c.on = true;
    c.rooms = [...c.rooms, room];
    c.emit();
  }, P);
}

test("a message's replies open in the panel over its room; back and Esc return to the room where it was, and the address never moves", async ({ page }, info) => {
  await overview(page);
  await staffWithThread(page);
  const room = panel(page).locator('[data-ov-staff="dm_cos"] [data-room="dm_cos"]');
  const footer = room.locator('[data-thread-footer="30"]');
  await footer.scrollIntoViewIfNeeded();
  await expect(footer).toBeVisible();
  const at = await room.evaluate((e) => e.scrollTop);
  const url = page.url();
  await footer.click();
  const thread = panel(page).locator('[data-thread-panel="30"]');
  await expect(thread).toBeVisible();
  await expect(thread).toContainText("Jo, as last time.");
  // One way back, the panel's; no head of the thread's own (its ×, its "Thread"), and Follow in the ⋯.
  await expect(panel(page).locator("[data-panel-back]")).toHaveCount(1);
  await expect(panel(page).locator("[data-panel-back]")).toHaveText(/Back to Chief of staff/);
  await expect(thread.locator("[data-thread-close], [data-thread-back], [data-thread-follow]")).toHaveCount(0);
  await expect(thread.getByText("Thread", { exact: true })).toHaveCount(0);
  await expect(thread.locator("[data-kit=composer]")).toContainText("thread · ");
  expect(page.url()).toBe(url);
  await page.screenshot({ path: info.outputPath("panel-thread.png") });
  await panel(page).locator("[data-panel-more]").click();
  await expect(page.getByRole("menuitem", { name: /^Follow/ })).toBeVisible();
  // Esc shuts the menu first, and the thread stays.
  await page.keyboard.press("Escape");
  await expect(page.getByRole("menuitem", { name: /^Follow/ })).toHaveCount(0);
  await expect(thread).toBeVisible();
  await panel(page).locator("[data-panel-back]").click();
  await expect(thread).toHaveCount(0);
  await expect(footer).toBeVisible();
  expect(Math.abs((await room.evaluate((e) => e.scrollTop)) - at)).toBeLessThanOrEqual(2);
  // Esc, from nowhere in particular, does the same.
  await footer.click();
  await expect(thread).toBeVisible();
  await blur(page);
  await page.keyboard.press("Escape");
  await expect(thread).toHaveCount(0);
  expect(Math.abs((await room.evaluate((e) => e.scrollTop)) - at)).toBeLessThanOrEqual(2);
  expect(page.url()).toBe(url);
});

test("a tab's × closes it, and the chief of staff comes to the front", async ({ page }) => {
  const { agent } = await overview(page);
  const host = await suiteHost(page);
  await say(page, [{ ...paneItem(host, agent), title: "Which colour?" }]);
  await row(page, `${host}.${agent}:1`).locator("button[aria-expanded]").first().click();
  const key = await keyOf(page, agent);
  await expect(panel(page).locator(`[data-tab="${key}"]`)).toHaveAttribute("aria-selected", "true");
  await panel(page).locator(`[data-tab-x="${key}"]`).click();
  await expect(panel(page).locator(`[data-tab="${key}"]`)).toHaveCount(0);
  await expect(panel(page).locator('[data-tab="chief"]')).toHaveAttribute("aria-selected", "true");
});

// Lift 4's "done" (#1480): v6's shots 1, 2, 2b and 13 at 1440×900, in
// Slate (dark, the default) and Latte (light, from Look). Written
// to the test's output, never to the repo.
test.describe("shots", () => {
  /** The page as it is now, in Slate and then Latte, and back to Slate. */
  async function shoot(page: Page, info: { outputPath(name: string): string }, name: string) {
    await blur(page);
    await expect(page.locator("html")).toHaveAttribute("data-scheme", "dark");
    await page.screenshot({ path: info.outputPath(`${name}-slate.png`) });
    const theme = async (t: "light" | "dark") => {
      await page.locator("[data-top] [data-open-look]").click();
      await page.locator(`[data-theme-choice="${t}"]`).click();
      await page.keyboard.press("Escape");
      await expect(page.locator("html")).toHaveAttribute("data-scheme", t);
    };
    await theme("light");
    await page.waitForTimeout(150);
    await page.screenshot({ path: info.outputPath(`${name}-latte.png`) });
    await theme("dark");
  }
  const ago = (min: number) => Date.now() - min * 60_000;
  /** The rows v6 draws around the one a shot is about. */
  const around = (host: string, term: number) => [
    { ...APPROVAL, title: "Start a worker on #1412 on geek?", since: ago(2) },
    { ...paneItem(host, term), id: "perm_1448", kind: "permission", title: "Allow Bash: rm -rf web/node_modules/.vite", since: ago(20) },
    { ...paneItem(host, term), id: "ask_1408", title: "When do you and Jake run the rehearsal?", since: ago(9 * 60) },
  ];

  test("1: an ask expanded in its row, its worker peeked", async ({ page }, info) => {
    const { agent, term } = await overview(page);
    const host = await suiteHost(page);
    const ask = {
      ...paneItem(host, agent),
      title: "Crumb at 900px: truncate the middle, or hide the project?",
      lines: ["At 900px the caret pushes the crumb's last segment off. Truncate the middle segments, or hide the project name?"],
      answer: { via: "card", options: ["Truncate middle", "Hide project"] },
      since: ago(7),
    };
    await say(page, [...around(host, term), ask]);
    await row(page, ask.id).locator("button[aria-expanded]").first().click();
    await expect(row(page, ask.id).getByRole("button", { name: /^Truncate middle/ })).toBeVisible();
    await expect(panel(page).locator("[data-panel-transcript]")).toContainText("hello", { timeout: 20_000 });
    await shoot(page, info, "shot-1");
  });

  test("2: a to-do expanded, its agent peeked", async ({ page }, info) => {
    const { agent, term } = await overview(page);
    const host = await suiteHost(page);
    const todo = {
      ...paneItem(host, agent),
      id: "todo_1430",
      kind: "todo",
      title: "Run the migration with prod creds",
      lines: ["Why: the migration needs the prod database's owner.", "cd ~/src/app\nDATABASE_URL=… pnpm migrate:prod"],
      answer: { via: "todo", options: ["Done", "Can't"] },
      since: ago(60),
    };
    await say(page, [...around(host, term), todo]);
    await row(page, todo.id).locator("button[aria-expanded]").first().click();
    await expect(row(page, todo.id).getByRole("textbox", { name: /Note/ })).toBeVisible();
    await expect(panel(page).locator("[data-panel-transcript]")).toContainText("hello", { timeout: 20_000 });
    await shoot(page, info, "shot-2");
  });

  test("2b: an ended session's to-do, the panel read only", async ({ page }, info) => {
    const { term } = await overview(page);
    const host = await suiteHost(page);
    const ended = Date.now() - 3 * 3600_000 - 60_000;
    await inject(page, "ws-2", "staging-checkout", { title: "#1431 staging checkout", attention: "idle", activity: { bps: 0, last_ms: ended }, reason: { kind: "exited", since_ms: ended, headline: "claude exited" } });
    const todo = {
      id: "todo_1431",
      kind: "todo",
      group: "decide",
      counts: true,
      for: "acct_a",
      title: "Add STRIPE_KEY to the staging secrets",
      from: { name: "claude" },
      subject: { type: "pane", id: "ws-2.1", url: "/", project: P, machine: "ws-2", pane: "1" },
      since: ended,
      lines: ["Why: checkout on staging fails without it."],
      answer: { via: "todo", options: ["Done", "Can't"] },
    };
    await say(page, [...around(host, term), todo]);
    await row(page, todo.id).locator("button[aria-expanded]").first().click();
    await expect(panel(page).locator("[data-panel-ended]")).toBeVisible();
    await expect(panel(page).locator("[data-panel-todo]")).toBeVisible();
    await shoot(page, info, "shot-2b");
  });

  test("13: after answering, the receipt row, and the panel's card answered", async ({ page }, info) => {
    const { term } = await overview(page);
    const host = await suiteHost(page);
    await proposing(page, START);
    // The Needs you row is the staff room's proposal: one id, its card's room and seq.
    const approval = { ...APPROVAL, id: START.id, subject: { ...APPROVAL.subject, id: START.id, room: "dm_cos", seq: 2 }, title: "Start a worker on #1412 on geek?", lines: ["Why: the release waits on it."], since: ago(2) };
    await say(page, [approval, ...around(host, term).slice(1)]);
    await expect(panel(page).locator('[data-ov-staff="dm_cos"] [data-proposal="prp_start"][data-ask-state="open"]')).toBeVisible();
    // Answered once, in its row: a receipt where it was, and the panel's card says so.
    await row(page, approval.id).locator("button[aria-expanded]").first().click();
    await row(page, approval.id).getByRole("button", { name: /^Start/ }).click();
    await expect(row(page, approval.id)).toContainText(/Start by you/);
    await expect(panel(page).locator('[data-proposal="prp_start"][data-ask-state="answered"]')).toBeVisible();
    await shoot(page, info, "shot-13");
  });
});
