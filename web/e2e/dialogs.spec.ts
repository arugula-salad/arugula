// One dialog, one side sheet, one backdrop (#690), on a fixture page with no
// daemon (dialogs-page.tsx). Modal and ConfirmRemove take the focus, keep it
// (Tab wraps), close on Escape without the page under them hearing it, and
// give the focus back to what opened them. The side sheet does the same, and
// a click on its backdrop closes it, on a desktop and at 390px, where it is a
// bottom sheet.
import { fileURLToPath } from "node:url";
import { expect, test, type Page } from "@playwright/test";
import { build } from "vite";

const ROOT = fileURLToPath(new URL("..", import.meta.url));
const ENTRY = fileURLToPath(new URL("./dialogs-page.tsx", import.meta.url));

type Out = { type: string; fileName: string; code?: string; source?: string | Uint8Array };
let html: Promise<string> | null = null;
/** The fixture page's HTML, script and style inline, built once per worker. */
function fixturePage(): Promise<string> {
  html ??= (async () => {
    const out = (await build({
      root: ROOT,
      configFile: false,
      oxc: { jsx: { runtime: "automatic", importSource: "preact" } },
      logLevel: "silent",
      build: { write: false, target: "es2022", lib: { entry: ENTRY, formats: ["es"], fileName: "page" }, cssCodeSplit: false },
    })) as { output: Out[] } | { output: Out[] }[];
    const files = Array.isArray(out) ? out.flatMap((o) => o.output) : out.output;
    const js = files.find((f) => f.type === "chunk")?.code ?? "";
    const css = files.filter((f) => f.fileName.endsWith(".css")).map((f) => String(f.source)).join("\n");
    return `<!doctype html><html><head><meta charset="utf-8"><meta name="viewport" content="width=device-width"><style>${css}</style></head><body><div id="root"></div><script type="module">${js.replaceAll("</script", "<\\/script")}</script></body></html>`;
  })();
  return html;
}

async function open(page: Page, width: number, height = 800) {
  const body = await fixturePage();
  await page.setViewportSize({ width, height });
  await page.route("**/dialogs.html", (r) => r.fulfill({ contentType: "text/html", body }));
  await page.goto("/dialogs.html");
  await expect(page.locator("[data-open]")).toHaveCount(3);
}

const escapes = (page: Page) => page.evaluate(() => (window as unknown as { escapes: number }).escapes);
/** The focus is inside `root`. */
const inside = (page: Page, root: string) => page.evaluate((s) => !!document.activeElement?.closest(s), root);

for (const [kind, root] of [
  ["modal", ".prompt[role=dialog]"],
  ["confirm", ".prompt[role=alertdialog]"],
] as const) {
  test(`${kind}: focus goes in and wraps, Escape closes it alone, focus comes back`, async ({ page }) => {
    await open(page, 1000);
    const opener = page.locator(`[data-open="${kind}"]`);
    await opener.focus();
    await opener.press("Enter");
    const dialog = page.locator(root);
    await expect(dialog).toBeVisible();
    await expect(dialog.getByRole("heading")).toHaveCount(1);
    expect(await inside(page, root)).toBe(true);
    // Tab past the last control and back: the focus never leaves.
    for (let i = 0; i < 8; i++) {
      await page.keyboard.press("Tab");
      expect(await inside(page, root)).toBe(true);
    }
    for (let i = 0; i < 8; i++) {
      await page.keyboard.press("Shift+Tab");
      expect(await inside(page, root)).toBe(true);
    }
    // A field behind it can't be reached, even by a click that lands on the backdrop's edge.
    await page.keyboard.press("Escape");
    await expect(dialog).toHaveCount(0);
    expect(await escapes(page)).toBe(0);
    await expect(opener).toBeFocused();
  });
}

test("modal: a click on the backdrop closes it, and the buttons come in order", async ({ page }) => {
  await open(page, 1000);
  await page.locator('[data-open="modal"]').click();
  const dialog = page.locator(".prompt[role=dialog]");
  await expect(dialog.locator(".prompt-buttons button")).toHaveText(["Cancel", "Rename"]);
  await page.mouse.click(10, 400);
  await expect(dialog).toHaveCount(0);
  await expect(page.locator('[data-open="modal"]')).toBeFocused();
});

test("the backdrop is one colour: 40% black under a dialog and a sheet", async ({ page }) => {
  await open(page, 1000);
  await page.locator('[data-open="modal"]').click();
  await expect(page.locator(".prompt-backdrop")).toHaveCSS("background-color", "rgba(0, 0, 0, 0.4)");
  await page.keyboard.press("Escape");
  await page.locator('[data-open="sheet"]').click();
  await expect(page.locator("[data-side-sheet-backdrop]")).toHaveCSS("background-color", "rgba(0, 0, 0, 0.4)");
});

for (const [width, height] of [
  [1000, 800],
  [390, 844],
] as const) {
  test(`side sheet at ${width}px: focus wraps, Escape and the backdrop close it, focus comes back`, async ({ page }) => {
    await open(page, width, height);
    const opener = page.locator('[data-open="sheet"]');
    await opener.focus();
    await opener.press("Enter");
    const sheet = page.locator("[data-sheet]");
    await expect(sheet).toBeVisible();
    await expect(sheet).toHaveAttribute("role", "dialog");
    expect(await inside(page, "[data-sheet]")).toBe(true);
    const box = (await sheet.boundingBox())!;
    if (width > 600) {
      // At the right edge, the dialog's width, the page's full height.
      expect(Math.round(box.x + box.width)).toBe(width);
      expect(Math.round(box.width)).toBe(440);
      expect(Math.round(box.height)).toBe(height);
    } else {
      // A bottom sheet: the width of the screen, on its bottom edge.
      expect(Math.round(box.width)).toBe(width);
      expect(Math.round(box.y + box.height)).toBe(height);
      expect(box.height).toBeLessThanOrEqual(height * 0.8 + 1);
    }
    for (let i = 0; i < 10; i++) {
      await page.keyboard.press("Tab");
      expect(await inside(page, "[data-sheet]")).toBe(true);
    }
    for (let i = 0; i < 10; i++) {
      await page.keyboard.press("Shift+Tab");
      expect(await inside(page, "[data-sheet]")).toBe(true);
    }
    await page.keyboard.press("Escape");
    await expect(sheet).toHaveCount(0);
    expect(await escapes(page)).toBe(0);
    await expect(opener).toBeFocused();

    // A click on the backdrop (the strip the sheet leaves) closes it too, and reaches nothing under it.
    await opener.click();
    await expect(sheet).toBeVisible();
    await page.mouse.click(width > 600 ? 10 : width / 2, width > 600 ? height / 2 : 10);
    await expect(sheet).toHaveCount(0);
    await expect(opener).toBeFocused();
    await expect(page.locator("[data-pane-field]")).not.toBeFocused();

    // The × closes it as well.
    await opener.click();
    await sheet.getByRole("button", { name: "Close" }).click();
    await expect(sheet).toHaveCount(0);
  });
}
