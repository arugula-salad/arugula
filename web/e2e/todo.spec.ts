// A to-do in the Overview's To do (Arugula #1468), after the live asks with
// the ◇ mark: it expands in place at a phone's width (360px) with its why,
// its how in a mono well to copy, and a note marked no secrets that's
// checked as it's typed; Done (D) answers it through the Inbox with the
// note, and the row stays as a dim receipt that reads the note from the
// to-do's record (#1462), not from what was typed. Control's refusal is
// said in its own words, and the row can be answered again. The panel's
// card for it, in the staff room, is the same receipt. When the asker's
// session has ended, the panel is its transcript read only, and in place
// of the composer, the note and Done, which continues that conversation:
// answered there, its foot is the receipt.
import { expect, test, type Page } from "@playwright/test";
import { overview, P, say } from "./overview-fixture";

test.use({ permissions: ["local-network-access"] });

const row = (page: Page, id: string) => page.locator(`[data-need-item="${id}"]`);
const posted = (page: Page) => page.evaluate(() => (window as unknown as { __inboxPosted?: unknown[] }).__inboxPosted ?? []);

const ASK = {
  id: "prp_start",
  kind: "approval",
  group: "decide",
  counts: true,
  title: "Start a worker on #12 on geek?",
  from: { name: "Chief of staff · Arugula" },
  subject: {
    type: "proposal",
    id: "prp_start",
    url: `/p/${P}/chat`,
    project: P,
  },
  answer: { via: "card", options: ["Start", "Not now"] },
  since: Date.now() - 9 * 60_000,
};
const TODO = {
  id: "prp_spool",
  kind: "todo",
  group: "decide",
  counts: true,
  title: "Swap the filament spool, then Done",
  from: { agent: "ses_printer", machine: "m_suite" },
  subject: {
    type: "proposal",
    id: "prp_spool",
    url: `/p/${P}/chat`,
    project: P,
    room: "room_staff",
    seq: 41,
  },
  lines: ["Why: the printer stopped, and only you can reach it.", "1. Open the lid\n2. Swap the spool\n3. Press resume"],
  answer: { via: "todo", options: ["Done", "Can't"] },
  since: Date.now() - 2 * 60_000,
};

const panel = (page: Page) => page.locator("[data-kit=panel]");

/** The reader's staff room, with the to-do's card in it (`TODO.subject`'s
 * room and seq), as control has it: open, then (answered in the Inbox) the
 * record the Inbox's answer kept, and the room told its message changed. */
async function staffTodo(page: Page) {
  await page.waitForFunction(() => (window as unknown as { __chat?: { on: boolean | null } }).__chat?.on === false);
  await page.evaluate(
    ({ P, TODO }) => {
      const { room: id, seq } = TODO.subject;
      const room = { id, kind: "dm", team: "t", name: "Chief of staff · Arugula", machine: null, machine_name: null, target: null, project: P, with: null, others: [], private: false, last: seq, read: seq, unread: 0, mentions: 0, wants: 0, muted: false, level: "all", level_own: false, archived: false, writable: true, topic: "", description: "", pins: 0, topic_writable: true, staff: true, lead: { machine: "m_geek", machine_name: "geek", session: "cos" } };
      const w = window as unknown as { __todos?: Record<string, unknown> };
      const kept = () => w.__todos?.[`${id}/${seq}`];
      const msg = { seq, at: Date.now() - 60_000, author: "", name: "", text: TODO.title, quote: null, pane: null, edited: null, deleted: null, imported: null, agent: null, reactions: [], files: [], pinned: false, also: false, via: `ask:${TODO.id}` };
      const open = { room: id, seq, parent: null, kind: "proposal", asker: "acct_a", asker_name: "Sam", question: "", choices: ["Done", "Can't"], open: true, mine: true, session: null, proposal: { id: TODO.id, action: "todo", by_session: "", issue: null, repo: null, prompt: null, sessions: [], why: "the printer stopped", state: "open", answered_by_name: null, outcome: null, started: null, todo: { what: TODO.title, how: "" } } };
      const json = (body: unknown) => new Response(JSON.stringify(body), { headers: { "Content-Type": "application/json" } });
      const before = window.fetch;
      window.fetch = async (input, init) => {
        const url = new URL(typeof input === "string" ? input : input instanceof URL ? input.href : input.url, location.href);
        if (url.pathname === `/api/chat/rooms/${id}/messages`) return json({ room, messages: [{ ...msg, mentions: kept() ? [] : ["acct_a"] }], more: false });
        if (url.pathname === `/api/chat/rooms/${id}/asks/${seq}`) return json(kept() ?? open);
        if (url.pathname === `/api/inbox/${TODO.id}` && init?.method === "POST") {
          const r = await before(input, init);
          if (r.ok) (c as unknown as { heard(ev: unknown): void }).heard({ type: "changed", room: id, seq });
          return r;
        }
        return before(input, init);
      };
      const c = (window as unknown as { __chat: { on: boolean | null; rooms: unknown[]; emit(): void } }).__chat;
      c.on = true;
      c.rooms = [...c.rooms, room];
      c.emit();
    },
    { P, TODO },
  );
}

/** The asker's block, on a machine this page doesn't reach: its agent
 * exited 3h ago, and its to-do names it (its subject's machine and pane). */
async function ended(page: Page) {
  const at = Date.now() - 3 * 3600_000 - 60_000;
  await page.evaluate(
    ({ P, at }) => {
      const fleet = (window as unknown as { __app: { fleet: { inject(p: unknown[]): void } } }).__app.fleet;
      fleet.inject([
        {
          key: "ws-2:1",
          host: "ws-2",
          id: 1,
          session: { id: 1, name: "printer" },
          stale: false,
          person: { id: "me", name: "me", kind: "me" },
          driver: null,
          watchers: [],
          unread: 0,
          mention: false,
          info: { id: 1, title: "claude", kind: "agent", type: "agent", session_project: { id: P, name: "Arugula" }, attention: "idle", activity: { bps: 0, last_ms: at }, reason: { kind: "exited", since_ms: at, headline: "claude exited" } },
        },
      ]);
    },
    { P, at },
  );
  return { ...TODO, subject: { ...TODO.subject, machine: "ws-2", pane: "1" }, since: at };
}

/** The to-do, expanded: its row. */
async function open(page: Page) {
  await say(page, [ASK, TODO]);
  const r = row(page, TODO.id);
  await expect(r).toBeVisible();
  await r.locator("button[aria-expanded]").first().click();
  await expect(r.getByRole("textbox", { name: /Note/ })).toBeVisible();
  return r;
}

for (const [name, viewport] of [
  ["at 360px", { width: 360, height: 780 }],
  ["on a computer", { width: 1440, height: 900 }],
] as const)
  test.describe(name, () => {
    test.use({ viewport });

    test("a to-do is answered in its row with a note, and stays as a dim receipt read from its record, as does its card in the panel", async ({ page }, info) => {
      await overview(page);
      await staffTodo(page);
      const r = await open(page);
      // After the live asks, under To do, with ◇.
      const order = await page.locator("[data-ov-needs] [data-need-item]").evaluateAll((es) => es.map((e) => e.getAttribute("data-need-item")));
      expect(order).toEqual([ASK.id, TODO.id]);
      await expect(page.locator("[data-ov-todo]")).toContainText("To do");
      await expect(r.locator(".k-mark").first()).toHaveText("◇");
      await expect(r).toContainText("Why: the printer stopped, and only you can reach it.");
      await expect(r.locator("[data-kit=well] pre")).toHaveText("1. Open the lid\n2. Swap the spool\n3. Press resume");
      await expect(r.getByRole("button", { name: /^Copy/ })).toBeVisible();
      await expect(r).toContainText("no secrets");
      // The row fits the column, expanded.
      expect(await page.locator("[data-ov-needs]").evaluate((e) => e.scrollWidth <= e.clientWidth)).toBe(true);

      // Checked as it's typed: a key's shape (made up here) says so, and holds the answers back.
      const note = r.getByRole("textbox", { name: /Note/ });
      await note.fill(`it's ${"ghp_" + "a1B2".repeat(9)}`);
      await expect(r.locator(".k-field-warn")).toHaveText("That looks like a GitHub token: leave it out");
      await expect(r.getByRole("button", { name: /^Done/ })).toBeDisabled();
      await expect(r.getByRole("button", { name: /^Can't/ })).toBeDisabled();
      await note.press("Enter");
      await page.screenshot({ path: info.outputPath("todo-secret.png") });
      expect(await posted(page)).toEqual([]);

      // Plain words: D from the row answers Done with the note.
      await note.fill("swapped to PETG");
      await expect(r.locator(".k-field-warn")).toHaveCount(0);
      await page.screenshot({ path: info.outputPath("todo-open.png") });
      // The record keeps the note as control does: the receipt shows its words.
      await page.evaluate(() => void ((window as unknown as { __todoKept: string }).__todoKept = "swapped to PETG (kept)"));
      await r.locator("button[aria-expanded]").first().focus();
      await page.keyboard.press("Shift+D");
      await expect.poll(() => posted(page)).toEqual([{ id: TODO.id, choice: "Done", note: "swapped to PETG" }]);
      await expect(r).toContainText(/Done by you .* · swapped to PETG \(kept\)/);
      await expect(r).toHaveClass(/is-done/);
      await expect(r.locator(".k-mark").first()).toHaveText("✓");
      await expect(r.getByRole("textbox", { name: /Note/ })).toHaveCount(0);
      await page.screenshot({ path: info.outputPath("todo-receipt.png") });

      // The panel's card for it, in the staff room: the same receipt, from the record.
      const card = panel(page).locator(`[data-ov-staff="${TODO.subject.room}"] [data-proposal="${TODO.id}"]`);
      await expect(card).toHaveAttribute("data-ask-state", "answered");
      await expect(card).toHaveText(/^Done by you .* · swapped to PETG \(kept\)$/);
      await card.scrollIntoViewIfNeeded();
      await page.screenshot({ path: info.outputPath("todo-card-receipt.png") });
    });

    test("an ended session's to-do: the panel is its transcript read only, and Done with a note continues it", async ({ page }, info) => {
      await overview(page);
      const todo = await ended(page);
      await say(page, [todo]);
      // Its row peeks its asker: the ended session.
      await row(page, todo.id).locator("button[aria-expanded]").first().click();
      const head = panel(page).locator("[data-panel-head]");
      await expect(head).toHaveAttribute("data-panel-state", "ended");
      await expect(panel(page).locator("[data-panel-ended]")).toHaveText("read-only · session ended 3h ago");
      // No composer: Done's line, the note and the answers in its place.
      await expect(panel(page).locator("[data-kit=composer]")).toHaveCount(0);
      const foot = panel(page).locator(`[data-panel-todo-foot="${todo.id}"]`);
      await expect(foot.locator("[data-panel-todo]")).toHaveText("Done continues this conversation with your note.");
      const note = foot.getByRole("textbox", { name: /Note/ });
      await expect(foot).toContainText("no secrets");
      // Checked as it's typed, as the row's is.
      await note.fill(`it's ${"ghp_" + "a1B2".repeat(9)}`);
      await expect(foot.locator(".k-field-warn")).toHaveText("That looks like a GitHub token: leave it out");
      await expect(foot.getByRole("button", { name: "Done" })).toBeDisabled();
      await note.fill("swapped to PETG");
      await foot.scrollIntoViewIfNeeded();
      await page.screenshot({ path: info.outputPath("todo-ended.png") });
      // Done answers the to-do with the note (control wakes its conversation with it); the foot is its receipt.
      await page.evaluate(() => void ((window as unknown as { __todoKept: string }).__todoKept = "swapped to PETG (kept)"));
      await foot.getByRole("button", { name: "Done" }).click();
      await expect.poll(() => posted(page)).toEqual([{ id: todo.id, choice: "Done", note: "swapped to PETG" }]);
      const receipt = panel(page).locator("[data-panel-todo-receipt]");
      await expect(receipt).toHaveText(/^Done by you .* · swapped to PETG \(kept\)$/);
      await expect(foot.getByRole("textbox")).toHaveCount(0);
      await expect(panel(page).locator("[data-kit=composer]")).toHaveCount(0);
      await receipt.scrollIntoViewIfNeeded();
      await page.screenshot({ path: info.outputPath("todo-ended-receipt.png") });
    });
  });

test.describe("at 360px", () => {
  test.use({ viewport: { width: 360, height: 780 } });

  test("control's refusal is said in its words, and the to-do can be answered again", async ({ page }) => {
    await overview(page);
    const r = await open(page);
    await page.evaluate(() => void ((window as unknown as { __inboxRefuse: unknown }).__inboxRefuse = { status: 409, error: "it was answered already" }));
    await r.getByRole("textbox", { name: /Note/ }).fill("couldn't find a spool");
    await r.getByRole("button", { name: /^Can't/ }).click();
    await expect(page.getByText("Couldn't answer it: it was answered already")).toBeVisible();
    await expect(r.getByRole("button", { name: /^Can't/ })).toBeEnabled();
    expect(await posted(page)).toEqual([{ id: TODO.id, choice: "Can't", note: "couldn't find a spool" }]);
  });
});
