// #472: a click on a board pane that another pane has the keys over used to
// scroll the board to its top (activation focused the block's first button)
// and lose the click. Alice has a project with a board of many cards, pinned
// beside a terminal on her machine. The shared cases of
// board-activate.spec.ts and its WebKit version.

import { expect, test, type Browser, type Page } from "@playwright/test";
import { closeContexts } from "./helpers";
import { TeamControl, call, home, live, show } from "./team-fixture";

const ROWS = 40;

export function boardActivateCases(name: string, newPerson: (browser: Browser, t: TeamControl) => Promise<Page>) {
  const t = new TeamControl(name);
  test.describe.configure({ mode: "serial" });
  test.afterAll(async ({ browser }) => {
    await closeContexts({ browser });
    t.stop();
  });

  let alice: Page;
  let terminal = 0;
  let boardPane = 0;

  const api = (method: string, path: string, body?: unknown) =>
    alice.evaluate(
      async ([m, u, b]) => {
        const r = await fetch(u as string, { method: m as string, credentials: "same-origin", headers: { "Content-Type": "application/json" }, body: b === undefined ? undefined : JSON.stringify(b) });
        return { status: r.status, body: (await r.json().catch(() => null)) as Record<string, any> | null };
      },
      [method, path, body] as const,
    );

  /** The board block's scrolling ancestor, whichever element that is. */
  const scroller = (p: Page) =>
    p.evaluate(
      ([pane]) => {
        let el: HTMLElement | null = document.querySelector(`[data-pane="${pane}"] [data-board-block]`);
        while (el && el.scrollHeight <= el.clientHeight + 1) el = el.parentElement;
        return el?.closest("[data-pane]") ? el.scrollTop : -1;
      },
      [boardPane],
    );
  const scrollTo = (p: Page, top: number) =>
    p.evaluate(
      ([pane, y]) => {
        let el: HTMLElement | null = document.querySelector(`[data-pane="${pane}"] [data-board-block]`);
        while (el && el.scrollHeight <= el.clientHeight + 1) el = el.parentElement;
        el!.scrollTop = y as number;
      },
      [boardPane, top] as const,
    );

  test("a board of many cards, pinned beside a terminal", async ({ browser }) => {
    test.setTimeout(240_000);
    await t.start(["--platform-host", "app.localhost"]);
    alice = await newPerson(browser, t);
    await alice.evaluate(() => window.__arugula.control!.createTeam("Acme"));
    const team = await alice.evaluate(() => window.__arugula.control!.teams[0].team);
    await alice.evaluate((tm) => window.__arugula.control!.setLabsFlag("boards", true, tm), team);
    await home(alice);
    await t.addMachine(alice, "alices", team);
    await expect.poll(() => live(alice, ["alices"]), { timeout: 30_000 }).toBe(true);
    await show(alice, "alices");
    const projects = await api("GET", "/api/projects");
    const project = (projects.body!.projects as { id: string; name: string }[]).find((p) => p.name === "Acme")!.id;
    const made = await api("POST", `/api/projects/${project}/boards`, { title: "Long" });
    expect(made.status).toBe(200);
    const board = made.body!.board.id as string;
    const zone = await api("PUT", `/api/boards/${board}/entries`, { entry: "zone", title: "Wave" });
    const wave = (zone.body!.groups as { id: string }[])[0].id;
    for (let i = 0; i < ROWS; i++) {
      const row = await api("PUT", `/api/boards/${board}/entries`, { entry: "row", note: `Card number ${i}`, zone: wave });
      expect(row.status).toBe(200);
    }
    const run = await call(alice, "POST", "/api/run", { command: "sleep 600" });
    expect(run.status, JSON.stringify(run.body)).toBe(200);
    terminal = (run.body as { pane: number }).pane;
    const opened = await call(alice, "POST", "/api/blocks", { type: "board", config: { board }, split: terminal, local: true });
    expect(opened.status, JSON.stringify(opened.body)).toBe(200);
    boardPane = (opened.body as { block: number }).block;
    await alice.bringToFront();
    await alice.evaluate((p) => window.__arugula.client.setActive(p), terminal);
    await expect(alice.locator(`[data-pane="${boardPane}"] [data-board-row]`).first()).toBeVisible({ timeout: 30_000 });
  });

  test("a click on a card of an inactive board, scrolled down, keeps the scroll and opens the card", async () => {
    test.setTimeout(90_000);
    await alice.setViewportSize({ width: 1400, height: 700 });
    // The other pane has the keys; the board is scrolled with no click on it.
    await alice.evaluate((p) => window.__arugula.client.setActive(p), terminal);
    await expect(alice.locator(`[data-pane="${boardPane}"]`)).not.toHaveClass(/\bactive\b/);
    await scrollTo(alice, 1200);
    const before = await scroller(alice);
    expect(before).toBeGreaterThan(300);
    const card = alice.locator(`[data-pane="${boardPane}"] [data-board-row]`);
    const below = await card.evaluateAll((els, pane) => {
      const box = document.querySelector(`[data-pane="${pane}"]`)!.getBoundingClientRect();
      const i = els.findIndex((e) => e.getBoundingClientRect().top > box.top + 150 && e.getBoundingClientRect().bottom < box.bottom - 20);
      return i;
    }, boardPane);
    expect(below).toBeGreaterThanOrEqual(0);
    // The scroll as the click lands, before the drawer it opens changes the board's height.
    await alice.evaluate(
      ([pane]) => {
        const w = window as unknown as { __atClick?: number };
        document.addEventListener(
          "click",
          () => {
            let el: HTMLElement | null = document.querySelector(`[data-pane="${pane}"] [data-board-block]`);
            while (el && el.scrollHeight <= el.clientHeight + 1) el = el.parentElement;
            w.__atClick = el?.scrollTop ?? -1;
          },
          { capture: true, once: true },
        );
      },
      [boardPane],
    );
    // A person's press lasts a moment: the shell's focus after the pointer-down lands inside it.
    const at = (await card.nth(below).boundingBox())!;
    await alice.mouse.move(at.x + 4, at.y + 4);
    await alice.mouse.down();
    await alice.waitForTimeout(200);
    await alice.mouse.up();
    await expect(alice.locator(`[data-pane="${boardPane}"]`)).toHaveClass(/\bactive\b/);
    // The click acted: the card's drawer opened, and the board stayed where it was.
    await expect(alice.locator(`[data-pane="${boardPane}"] [data-board-drawer]`)).toBeVisible({ timeout: 10_000 });
    expect(await alice.evaluate(() => (window as unknown as { __atClick?: number }).__atClick)).toBe(before);
    // ...and stayed there once the drawer was open (it may shorten the board a little, not send it to the top).
    expect(await scroller(alice)).toBeGreaterThan(before / 2);
  });
}
