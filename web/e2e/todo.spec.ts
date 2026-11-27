// A to-do in the Overview's To do (Arugula #1468), after the live asks with
// the ◇ mark: it expands in place at a phone's width (360px) with its why,
// its how in a mono well to copy, and a note marked no secrets that's
// checked as it's typed; Done (D) answers it through the Inbox with the
// note, and the row stays as a dim receipt that reads the note from the
// to-do's record (#1462), not from what was typed. Control's refusal is
// said in its own words, and the row can be answered again.
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

    test("a to-do is answered in its row with a note, and stays as a dim receipt read from its record", async ({ page }, info) => {
      await overview(page);
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
