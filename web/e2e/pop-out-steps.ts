// Pop a pane out, shared by the Chromium and WebKit specs.
//
// Pop a pane out into a window of its own (Arugula #497): a Doc block's
// Pop out opens a page with only that block, its place in the tab says it's
// out, and closing the window, or Bring back, puts it back. A terminal
// doesn't pop out yet (its size is its tab's until #477).

import { expect, test, type Page } from "@playwright/test";
import { menuItem, panes, reset } from "./helpers";

/** A Doc beside the terminal, through the page's own client. */
async function doc(page: Page, term: number, title: string): Promise<number> {
  const made = await page.evaluate(
    ([term, title]) =>
      window.__arugula.client
        .request("POST", "/api/blocks", {
          type: "doc",
          config: { title },
          split: term,
          local: true,
        })
        .then((r) => r.json<{ block: number }>()),
    [term, title] as const,
  );
  await expect(page.locator(`[data-doc-block="${made.block}"]`)).toBeVisible({
    timeout: 15_000,
  });
  return made.block;
}

/** Its pane menu, by the header's button. */
async function paneMenu(page: Page, pane: number) {
  await page.locator(`[data-pane-more="${pane}"]`).click();
  await expect(page.getByRole("menu")).toBeVisible();
}

export function popOutSpecs(tag: string) {
  test(`${tag}: a Doc pops out into a window of its own, and comes back when the window closes or from Bring back`, async ({
    page,
  }) => {
    await reset(page);
    const [term] = await panes(page);
    const d = await doc(page, term, "Pop me out");

    // A terminal doesn't offer it.
    await paneMenu(page, term);
    await expect(page.getByRole("menuitem", { name: "Pop out" })).toHaveCount(
      0,
    );
    await page.keyboard.press("Escape");

    // Pop out: a page with only the Doc.
    await paneMenu(page, d);
    const [win] = await Promise.all([
      page.context().waitForEvent("page"),
      (await menuItem(page, "Pop out", { exact: true })).click(),
    ]);
    await expect(win.locator(`[data-pop-out-page="${d}"]`)).toHaveAttribute(
      "data-phase",
      "out",
      { timeout: 15_000 },
    );
    await expect(
      win.locator(`[data-pop-out-page="${d}"] [data-doc-block="${d}"]`),
    ).toBeVisible();
    await expect(win.locator("[data-pane-more], .rail, .tab-bar")).toHaveCount(
      0,
    );
    await expect(win).toHaveTitle(/Pop me out/);
    // Its place in the tab here says it's out; the terminal stays.
    const placeholder = page.locator(`[data-popped-out="${d}"]`);
    await expect(placeholder).toBeVisible();
    await expect(placeholder).toContainText("Pop me out");
    await expect(page.locator(`[data-pane="${term}"]`)).toBeVisible();

    // Closing the window brings it back.
    await win.close();
    await expect(placeholder).toHaveCount(0);
    await expect(
      page.locator(`[data-pane="${d}"] [data-doc-block="${d}"]`),
    ).toBeVisible();

    // Out again, then Bring back from here: the window says so.
    await paneMenu(page, d);
    const [again] = await Promise.all([
      page.context().waitForEvent("page"),
      (await menuItem(page, "Pop out", { exact: true })).click(),
    ]);
    await expect(again.locator(`[data-pop-out-page="${d}"]`)).toHaveAttribute(
      "data-phase",
      "out",
      { timeout: 15_000 },
    );
    await page.locator(`[data-bring-back="${d}"]`).click();
    await expect(
      page.locator(`[data-pane="${d}"] [data-doc-block="${d}"]`),
    ).toBeVisible();
    // A window the page didn't open itself may stay: it says the pane is back.
    // It can close between isClosed() and the read (Arugula #911): a read
    // that finds it closing counts as closed.
    await expect
      .poll(
        async () =>
          again.isClosed() ||
          (await again
            .locator(`[data-pop-out-page="${d}"]`)
            .getAttribute("data-phase")
            .catch(() => (again.isClosed() ? "back" : null))) === "back",
      )
      .toBe(true);
  });
}
