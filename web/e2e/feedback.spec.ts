// Send feedback (shell/ui/feedback.tsx, Arugula #1217): the command
// palette's Help opens it; the first line is the title and the rest the
// body; Send goes to this daemon's `POST /api/feedback` and says the F- id,
// or the refusal's one line. The daemon's handler is #1216's, so these
// answer for it. On a daemon's own page nobody is signed in: no contact box.

import { devices, expect, test, type Page } from "@playwright/test";
import { paletteCommand, reset, sheetTab } from "./helpers";

const dialog = (page: Page) => page.getByRole("dialog", { name: "Send feedback" });

async function openFeedback(page: Page) {
  await reset(page);
  await paletteCommand(page, "Help/Send feedback…");
  await expect(dialog(page)).toBeVisible();
}

test("a send: its kind, title and body go to the daemon, and the F- id comes back", async ({ page }) => {
  const sent: unknown[] = [];
  await page.route("**/api/feedback", async (r) => {
    sent.push(r.request().postDataJSON());
    await r.fulfill({ status: 200, contentType: "application/json", body: JSON.stringify({ id: "fbk_1", shown: "F-7K2Q9X", queued: false }) });
  });
  await openFeedback(page);
  const d = dialog(page);
  await expect(d.getByText("Sends your Arugula version, OS and this page. Never your terminal.")).toBeVisible();
  // Nobody signed in here: nothing to reach.
  await expect(d.getByRole("checkbox", { name: "You can reach me about this" })).toHaveCount(0);
  const send = d.getByRole("button", { name: "Send", exact: true });
  await expect(send).toBeDisabled();

  // Words and a kind, both: the daemon takes no feedback without one.
  await d.getByRole("textbox", { name: "Your feedback" }).fill("Paste drops a newline\n\nPasting two lines runs the first.");
  await expect(send).toBeDisabled();
  await d.getByRole("button", { name: "Bug", exact: true }).click();
  await expect(d.getByRole("button", { name: "Bug", exact: true })).toHaveAttribute("aria-pressed", "true");
  await send.click();

  await expect(d.getByText("Thanks — sent as F-7K2Q9X.")).toBeVisible();
  expect(sent).toEqual([
    { kind: "bug", title: "Paste drops a newline", body: "Pasting two lines runs the first.", contact_ok: false, source: "person", surface: "web", route: "/" },
  ]);
  await d.getByRole("button", { name: "Done" }).click();
  await expect(dialog(page)).toHaveCount(0);
});

test("kept for later: it says it goes when the machine is back online", async ({ page }) => {
  await page.route("**/api/feedback", (r) =>
    r.fulfill({ status: 200, contentType: "application/json", body: JSON.stringify({ id: "", shown: "", queued: true }) }),
  );
  await openFeedback(page);
  const d = dialog(page);
  await d.getByRole("textbox", { name: "Your feedback" }).fill("An idea");
  await d.getByRole("button", { name: "Idea", exact: true }).click();
  await d.getByRole("button", { name: "Send", exact: true }).click();
  await expect(d.getByText("Thanks — saved. It goes when this machine is back online.")).toBeVisible();
});

test("a refusal is the daemon's one line, and what was written stays", async ({ page }) => {
  const said = "Arugula isn't taking feedback from machines that aren't joined to Arugula cloud just now. Join this one (`arugula join`), or send it while signed in at app.arugula.io. Thanks for writing.";
  await page.route("**/api/feedback", (r) => r.fulfill({ status: 403, contentType: "application/json", body: JSON.stringify({ error: said }) }));
  await openFeedback(page);
  const d = dialog(page);
  await d.getByRole("textbox", { name: "Your feedback" }).fill("An idea");
  await d.getByRole("button", { name: "Idea", exact: true }).click();
  await d.getByRole("button", { name: "Send", exact: true }).click();
  await expect(d.locator("[data-feedback-refused]")).toHaveText(said);
  await expect(d.getByRole("textbox", { name: "Your feedback" })).toHaveValue("An idea");
});

test.describe("on a phone", () => {
  const { viewport, userAgent, deviceScaleFactor, isMobile, hasTouch } = devices["Pixel 7"];
  test.use({ viewport, userAgent, deviceScaleFactor, isMobile, hasTouch });

  test("the sheet's Go to… reaches it, and it's a sheet along the bottom", async ({ page }) => {
    await reset(page);
    await page.locator(".sheet-button").click();
    await sheetTab(page, "More");
    await page.getByRole("button", { name: "Go to…", exact: true }).click();
    const palette = page.getByRole("dialog", { name: "Go to…" });
    await palette.getByRole("combobox", { name: "Go to" }).fill("Send feedback");
    await palette.locator('[data-command="Help/Send feedback…"]').click();
    const d = dialog(page);
    await expect(d).toHaveClass(/\bphone\b/);
    const vp = page.viewportSize()!;
    await expect.poll(async () => {
      const b = (await d.boundingBox())!;
      return [Math.round(b.x), Math.round(b.width), Math.round(b.y + b.height)];
    }).toEqual([0, vp.width, vp.height]);
  });
});
