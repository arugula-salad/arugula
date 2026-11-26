// Go to… (Lift 3, #1479; the command palette's, #139, before it): its
// checks on a machine's page, run in Chrome (goto.spec.ts) and WebKit
// (goto.webkit.spec.ts), on a desktop and a phone.

import { expect, type Page } from "@playwright/test";
import { newDoc, type Post } from "./doc-steps";
import { active, paneEl, panes, ready, reset, tabsInSession, text, type as typeIn, sheetTab } from "./helpers";

export const goTo = (page: Page) => page.getByRole("dialog", { name: "Go to…" });
export const field = (page: Page) => goTo(page).getByRole("combobox", { name: "Go to" });
const rows = (page: Page) => goTo(page).getByRole("option");

/** Type into Go to… and run the best match with Enter. */
async function pick(page: Page, query: string, key: string) {
  await field(page).fill(query);
  await expect(rows(page).first()).toHaveAttribute("data-command", key);
  await field(page).press("Enter");
  await expect(goTo(page)).toBeHidden();
}

/** The pane menu's rows, by label. */
async function paneMenu(page: Page, pane: number): Promise<string[]> {
  await paneEl(page, pane).click({ button: "right", position: { x: 60, y: 60 } });
  const menu = page.getByRole("menu");
  await expect(menu).toBeVisible();
  const labels = await menu.getByRole("menuitem").evaluateAll((els) =>
    els.map((el) =>
      Array.from(el.childNodes)
        .filter((n) => !(n instanceof HTMLElement && (n.classList.contains("menu-check") || n.classList.contains("menu-key"))))
        .map((n) => n.textContent)
        .join("")
        .trim(),
    ),
  );
  await page.keyboard.press("Escape");
  await expect(menu).toBeHidden();
  return labels;
}

/** From a terminal, Ctrl+Shift+K; elsewhere `chord` (⌘K on a Mac, Ctrl+K). */
export async function desktop(page: Page, chord = "Control+Shift+KeyK") {
  await page.addInitScript(() => localStorage.removeItem("arugula.goto.recent"));
  await reset(page);
  const [first] = await panes(page);
  await ready(page, first);

  // The chord from a focused terminal: Go to… opens and the shell gets nothing.
  await typeIn(page, first, "echo goto-$((2+3))\n");
  await expect.poll(() => text(page, first)).toMatch(/goto-5\n[^\n]*\$ /);
  const before = await text(page, first);
  await page.keyboard.press(chord);
  await expect(goTo(page)).toBeVisible();
  await expect(field(page)).toBeFocused();
  await page.keyboard.press("Escape");
  await expect(goTo(page)).toBeHidden();

  // The pane menu's kept actions are commands, for the same pane; its
  // clipboard rows aren't.
  const menu = await paneMenu(page, first);
  expect(menu).toContain("Split right");
  expect(menu).toContain("Copy");
  await page.keyboard.press(chord);
  const listed = await goTo(page)
    .locator('[data-command^="Pane/"]')
    .evaluateAll((els) => els.map((e) => (e as HTMLElement).dataset.command!.slice("Pane/".length)));
  expect(listed).toContain("Split right");
  expect(listed).not.toContain("Copy");

  // Type and run: Split right on the active pane.
  await pick(page, "split right", "Pane/Split right");
  await expect.poll(async () => (await panes(page)).length).toBe(2);
  // Nothing reached the first pane: its prompt line is as it was.
  const lastLine = (t: string) => t.trimEnd().split("\n").at(-1);
  const after = await text(page, first);
  expect(lastLine(after)).toBe(lastLine(before));
  expect(after).toContain("goto-5");

  // Move the new pane to a tab of its own, then rename that tab through a prompt.
  await page.keyboard.press(chord);
  await pick(page, "move to new tab", "Pane/Move to new tab");
  await expect.poll(async () => (await tabsInSession(page)).length).toBe(2);
  await page.keyboard.press(chord);
  await pick(page, "rename tab", "Tab/Rename tab");
  const prompt = page.getByRole("dialog", { name: "Rename tab" });
  await prompt.getByRole("textbox").fill("goto-tab");
  await prompt.getByRole("textbox").press("Enter");
  await expect(page.locator(".tab.selected .tab-label")).toHaveText("goto-tab");

  // Recent picks come first, newest first, with nothing typed.
  await page.keyboard.press(chord);
  await expect(rows(page).nth(0)).toHaveAttribute("data-command", "Tab/Rename tab");
  await expect(rows(page).nth(1)).toHaveAttribute("data-command", "Pane/Move to new tab");

  // Jump to the first tab by name; arrows move the selection.
  const firstTab = (await tabsInSession(page))[0];
  await field(page).fill("tab");
  const jump = goTo(page).locator(`[data-command="Go/tab ${firstTab}"]`);
  await expect(jump).toBeVisible();
  const at = await rows(page).evaluateAll((els, key) => els.findIndex((e) => (e as HTMLElement).dataset.command === key), `Go/tab ${firstTab}`);
  for (let i = 0; i < at; i++) await field(page).press("ArrowDown");
  await expect(jump).toHaveAttribute("aria-selected", "true");
  await field(page).press("Enter");
  await expect.poll(() => page.evaluate(() => window.__arugula.client.tab)).toBe(firstTab);
  await expect.poll(() => active(page)).toBe(first);

  // The chord again closes it.
  await page.keyboard.press(chord);
  await expect(goTo(page)).toBeVisible();
  await page.keyboard.press(chord);
  await expect(goTo(page)).toBeHidden();

  // `g p` from outside a field opens it, and so does the bar's button.
  await page.evaluate(() => (document.activeElement as HTMLElement | null)?.blur());
  await page.keyboard.press("g");
  await page.keyboard.press("p");
  await expect(goTo(page)).toBeVisible();
  await page.keyboard.press("Escape");
  await page.locator(".ws-head [data-open-palette]").click();
  await expect(goTo(page)).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(goTo(page)).toBeHidden();
}

/** On a phone: from the sheet's Go to… button. */
export async function phone(page: Page) {
  await reset(page);
  await page.locator(".sheet-button").click();
  await sheetTab(page, "More");
  await page.locator("[data-open-palette]").click();
  await expect(goTo(page)).toBeVisible();
  await field(page).fill("split down");
  await expect(rows(page).first()).toHaveAttribute("data-command", "Pane/Split down");
  await rows(page).first().tap();
  await expect(goTo(page)).toBeHidden();
  await expect.poll(() => page.evaluate(() => window.__arugula.client.state!.panes.length)).toBe(2);
}

/** The Docs page and the Docs a machine keeps (#1494): "Docs on this
 * machine" goes to the page, and a Doc whose pane is closed is found by its
 * title, asked for once, not on every key. */
export async function docs(page: Page, chord = "Control+Shift+KeyK") {
  await page.addInitScript(() => localStorage.removeItem("arugula.goto.recent"));
  const asked: string[] = [];
  page.on("request", (r) => r.method() === "GET" && new URL(r.url()).pathname === "/api/docs" && asked.push(r.url()));
  await reset(page);
  const [term] = await panes(page);
  await ready(page, term);
  const title = `GoTo Doc ${Date.now()}`;
  const post: Post = async (path, data) => (await page.request.post(path, { data })).json();
  const made = await newDoc(post, term, title);
  await post(`/api/panes/${made}/close`, {});
  await expect(page.locator(`[data-doc-block="${made}"]`)).toHaveCount(0);

  // Asked when the page came up, before the Doc was made: a page that
  // comes up again asks again.
  await page.reload();
  await page.waitForFunction(() => !!window.__arugula?.client?.state);
  await ready(page, term);
  await page.keyboard.press(chord);
  await expect(goTo(page)).toBeVisible();
  const was = asked.length;
  await field(page).pressSequentially("goto doc", { delay: 20 });
  const entry = goTo(page).locator('[data-command^="Docs/open "]').filter({ hasText: title });
  await expect(entry).toHaveCount(1);
  expect(asked.length).toBe(was);
  await expect(rows(page).first()).toContainText(title);
  await field(page).press("Enter");
  await expect(goTo(page)).toBeHidden();
  await expect.poll(async () => page.evaluate((p) => window.__arugula.client.info(p)?.type, await active(page))).toBe("doc");
  await expect(page.locator(`[data-doc-block="${await active(page)}"] .doc-title`)).toHaveText(title);

  // The Docs page is a place.
  await page.keyboard.press(chord);
  await field(page).fill("docs on this");
  await goTo(page).locator('[data-command="Places/docs"]').click();
  await expect(goTo(page)).toBeHidden();
  await expect(page.locator("[data-docs-page]")).toBeVisible();
}
