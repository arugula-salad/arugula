// The agent panel's boxes (Arugula #1555, #1556), on a board (its lead) and
// on the Overview (a peeked worker): in the Terminal view only the
// terminal's own scrollback scrolls, not the page, the panel's body or the
// terminal's slot; the terminal fits its slot, its pane taking that size;
// the Transcript fills the same box; and the tab row never scrolls.
import { expect, test, type Locator, type Page } from "@playwright/test";
import { fixturePage } from "./fixture-page";
import { panes, reset } from "./helpers";
import { overview, P, paneItem, say, suiteHost } from "./overview-fixture";

test.use({ permissions: ["local-network-access"] });

const B = "brd_0123456789abcdef0123456789abcdef";

/** The suite's daemon by control's id, `m_suite`, as the hub lists it: the panel's terminal reaches it. */
async function suiteIsListed(page: Page) {
  await page.route("**/api/hosts", async (r) => {
    const list = (await (await r.fetch()).json()) as { this: string; hosts: unknown[] };
    await r.fulfill({ json: { ...list, hosts: [...list.hosts, { name: list.this, id: "m_suite", urls: [], transport: "tailnet", added_ms: 0, last_seen_ms: null }] } });
  });
}

/** A board led by a fresh pane of the suite's daemon. */
async function ledBoard(page: Page): Promise<{ panel: Locator; pane: number }> {
  await reset(page);
  const [pane] = await panes(page);
  return { panel: await boardLedBy(page, pane), pane };
}

/** The board, led by `pane`: its panel. */
async function boardLedBy(page: Page, pane: number): Promise<Locator> {
  await suiteIsListed(page);
  const body = await fixturePage("./app-frame-page.tsx");
  await page.route(new RegExp(`/p/${P}`), (r) => r.fulfill({ contentType: "text/html", body }));
  const lead = { machine: "m_suite", machine_name: "suite-box", session: "Launch lead", pane, at: 1 };
  const board = { id: B, project: P, title: "Launch", created_by: "acct_a", created_at: 1, updated_at: 1, goal: "", done_note: "", closed_by: "", progress: { done: 0, total: 0, proven: 0, review: 0, doing: 0 }, lead };
  await page.route(new RegExp(`/api/boards/${B}$`), (r) => r.fulfill({ json: { board, zones: [], groups: [], docs: [] } }));
  await page.goto(`/p/${P}/boards/${B}`);
  return page.locator(".board-desk-side [data-kit=panel]");
}

/** What overflows: each box from the terminal (or the transcript) up to the page, and the tab row. */
const overflows = (panel: Locator) =>
  panel.evaluate((p) => {
    const out: string[] = [];
    const from = p.querySelector("[data-panel-terminal] .xterm") ?? p.querySelector("[data-panel-transcript]");
    // The panel's body is the transcript's one scroller; a terminal's is its own.
    const scroller = p.querySelector("[data-panel-terminal]") ? null : p.querySelector("[data-panel-body]");
    for (let el = from?.parentElement ?? null; el; el = el.parentElement) {
      if (el === scroller) continue;
      if (el.scrollWidth > el.clientWidth || el.scrollHeight > el.clientHeight) out.push(`${el.tagName.toLowerCase()}.${[...el.classList].join(".")}`);
    }
    const tabs = p.querySelector<HTMLElement>(".k-panel-tabs .k-tabs");
    // It may scroll across, with no bar; never down.
    const across = tabs && getComputedStyle(tabs);
    if (tabs && (across!.overflowY !== "hidden" || across!.scrollbarWidth !== "none")) out.push("the tab row");
    return out;
  });

/** The terminal: in its slot, and most of it. */
async function fits(panel: Locator) {
  const { slot, screen } = await panel.evaluate((p) => ({
    slot: p.querySelector(".work-slot")!.getBoundingClientRect().toJSON() as DOMRect,
    screen: p.querySelector(".work-slot .xterm-screen")!.getBoundingClientRect().toJSON() as DOMRect,
  }));
  expect(screen.right).toBeLessThanOrEqual(slot.right + 0.5);
  expect(screen.bottom).toBeLessThanOrEqual(slot.bottom + 0.5);
  expect(screen.width).toBeGreaterThan(slot.width - 30);
  expect(screen.height).toBeGreaterThan(slot.height - 40);
}

/** Type a line into a pane through the daemon's API. */
const send = (page: Page, pane: number, text: string) =>
  page.evaluate(({ pane, text }) => fetch(`/api/panes/${pane}/send`, { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ text, enter: true }) }), { pane, text });
const screenOf = (page: Page, pane: number) => page.evaluate(async (p) => (await fetch(`/api/panes/${p}/capture?format=text`)).text(), pane);
/** Where the terminal's own scrollbar is: its slider's top. */
const slider = (panel: Locator) => panel.evaluate((p) => p.querySelector<HTMLElement>("[data-panel-terminal] .xterm .scrollbar.vertical .slider")?.offsetTop ?? -1);

/** Terminal, then Transcript: one scroller each, the terminal fitted and its pane that size, the wheel over its text scrolling it, the page still. */
async function oneScroller(page: Page, panel: Locator, pane: number, shot: string) {
  await panel.locator('[data-panel-view="terminal"]').click();
  await expect(panel.locator("[data-panel-terminal] .xterm")).toBeVisible({ timeout: 20_000 });
  // The head names its machine; the terminal's frame doesn't again (#1572).
  await expect(panel.locator(".k-well-label")).toHaveCount(0);
  await expect.poll(() => overflows(panel), { timeout: 15_000 }).toEqual([]);
  await expect.poll(() => fits(panel).then(() => true, () => false), { timeout: 15_000 }).toBe(true);
  // The program sees the slot's size: its PTY took as many columns as fit, drawn at full size.
  await send(page, pane, "clear; stty size");
  await expect.poll(async () => /^\d+ \d+$/m.test(await screenOf(page, pane))).toBe(true);
  const cols = Number((await screenOf(page, pane)).match(/^\d+ (\d+)$/m)![1]);
  const drawn = await panel.evaluate((p) => {
    const slot = p.querySelector<HTMLElement>(".work-slot")!;
    const host = slot.querySelector<HTMLElement>(".term-host")!;
    return { slot: slot.clientWidth, screen: host.querySelector<HTMLElement>(".xterm-screen")!.offsetWidth, scaled: host.style.transform };
  });
  expect(drawn.scaled).toBe("");
  expect(Math.floor(drawn.slot / (drawn.screen / cols))).toBe(cols);
  await send(page, pane, "seq 1 300");
  await expect.poll(() => screenOf(page, pane)).toMatch(/\n300\n/);
  await page.screenshot({ path: shot });
  // The wheel over the terminal's text, not its scrollbar, scrolls its scrollback.
  const box = (await panel.locator("[data-panel-terminal] .xterm-screen").boundingBox())!;
  const before = await slider(panel);
  await page.mouse.move(box.x + box.width / 3, box.y + box.height / 2);
  await page.mouse.wheel(0, -600);
  await expect.poll(() => slider(panel)).toBeLessThan(before);
  await panel.locator('[data-panel-view="transcript"]').click();
  await expect(panel.locator("[data-panel-transcript]")).toBeVisible();
  await expect.poll(() => overflows(panel)).toEqual([]);
}

for (const width of [1440, 1200]) {
  test(`a board's lead at ${width}×900: one scroller, the terminal fitted; no scrollbar in the tab row`, async ({ page }, info) => {
    await page.setViewportSize({ width, height: 900 });
    const { panel, pane } = await ledBoard(page);
    await expect(panel.locator("[role=tab][aria-selected=true]")).toHaveText("Lead 🔒");
    await oneScroller(page, panel, pane, info.outputPath(`board-${width}.png`));
  });

  test(`the Overview's peek at ${width}×900: one scroller, the terminal fitted; no scrollbar in the tab row`, async ({ page }, info) => {
    await page.setViewportSize({ width, height: 900 });
    await suiteIsListed(page);
    const { term } = await overview(page);
    const host = await suiteHost(page);
    await say(page, [{ ...paneItem(host, term), title: "Run the migration?" }]);
    await page.locator(`[data-need-item="${host}.${term}:1"] button[aria-expanded]`).first().click();
    await oneScroller(page, page.locator("[data-kit=panel]"), term, info.outputPath(`overview-${width}.png`));
  });
}

/** The pane's columns as its program sees them, asked afresh: each answer is numbered. */
let asked = 0;
async function stty(page: Page, pane: number) {
  const n = ++asked;
  await send(page, pane, `clear; echo "size-${n} $(stty size)"`);
  const at = new RegExp(`^size-${n} \\d+ (\\d+)$`, "m");
  let cols = 0;
  await expect.poll(async () => (cols = Number((await screenOf(page, pane)).match(at)?.[1] ?? 0))).toBeGreaterThan(0);
  return cols;
}
/** The columns of the panel's terminal slot, in cells `cell` px wide. */
const slotCols = (panel: Locator, cell: number) => panel.evaluate((p, cell) => Math.floor(p.querySelector<HTMLElement>(".work-slot")!.clientWidth / cell), cell);

// #1572: the person's latest window takes the size, as tmux's
// latest-client does. Each window here only says it came to the front, as
// a window does when you switch to it; nobody clicks.
test("a board's lead: narrow, then wide, then narrow, the window in front sizes the terminal", async ({ page }) => {
  await page.context().addInitScript(() => {
    let front = false;
    Document.prototype.hasFocus = () => front;
    (window as unknown as { front: (on: boolean) => void }).front = (on) => {
      front = on;
      window.dispatchEvent(new Event(on ? "focus" : "blur"));
    };
  });
  const front = (p: Page, on: boolean) => p.evaluate((on) => (window as unknown as { front: (on: boolean) => void }).front(on), on);
  const terminal = async (panel: Locator) => {
    await panel.locator('[data-panel-view="terminal"]').click();
    await expect(panel.locator("[data-panel-terminal] .xterm")).toBeVisible({ timeout: 20_000 });
  };

  // Narrow: a 1200px window, in front.
  await page.setViewportSize({ width: 1200, height: 900 });
  const { panel: narrow, pane } = await ledBoard(page);
  await terminal(narrow);
  await front(page, true);
  await expect.poll(() => fits(narrow).then(() => true, () => false), { timeout: 15_000 }).toBe(true);
  const cols = await stty(page, pane);
  const cell = await narrow.evaluate((p, cols) => p.querySelector<HTMLElement>(".work-slot .xterm-screen")!.offsetWidth / cols, cols);
  expect(await slotCols(narrow, cell)).toBe(cols);

  // Wide: a 1800px window comes to the front while the narrow one stays open.
  const other = await page.context().newPage();
  await other.setViewportSize({ width: 1800, height: 900 });
  const wide = await boardLedBy(other, pane);
  await terminal(wide);
  await front(page, false);
  await front(other, true);
  const wideCols = await slotCols(wide, cell);
  expect(wideCols).toBeGreaterThan(cols + 10);
  await expect.poll(() => stty(other, pane), { timeout: 15_000 }).toBe(wideCols);
  // The narrow window, behind, draws it smaller.
  await expect.poll(() => narrow.evaluate((p) => p.querySelector<HTMLElement>(".work-slot .term-host")!.style.transform)).toMatch(/^scale\(/);

  // Narrow again: its window comes back to the front.
  await front(other, false);
  await front(page, true);
  await expect.poll(() => stty(page, pane), { timeout: 15_000 }).toBe(cols);

  // And the window in front, widened, takes its new width.
  await page.setViewportSize({ width: 1800, height: 900 });
  await expect.poll(() => stty(page, pane), { timeout: 15_000 }).toBe(wideCols);
  await page.setViewportSize({ width: 1200, height: 900 });
  await expect.poll(() => stty(page, pane), { timeout: 15_000 }).toBe(cols);
});
