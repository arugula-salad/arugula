// Send feedback from the platform pages (Arugula #1240): the account
// menu's Send feedback… opens the shared sheet, which sends straight to
// control's `POST /api/feedback` with the page's session: control-wire's
// `FeedbackSubmit` as a signed-in person on the web, with the page's route,
// and no install id or stamp. Control's answers are this spec's, on #971's
// fixture page (move-to-project-page.tsx).

import { expect, test, type Page, type Route } from "@playwright/test";
import { fixturePage } from "./fixture-page";

const json = (r: Route, body: unknown, status = 200) => r.fulfill({ status, contentType: "application/json", body: JSON.stringify(body) });

async function open(page: Page, feedback: (r: Route) => Promise<void>) {
  await page.route(/\/api\//, (r) => json(r, { error: "not here" }, 404));
  await page.route(/\/api\/projects$/, (r) =>
    json(r, { me: { account: "acct_a", login: "sam", name: "Sam" }, projects: [{ id: "prj_aaaa0001", name: "Default", owner: { kind: "personal" }, team: null }], hosted: false }),
  );
  await page.route(/\/api\/feedback$/, feedback);
  const body = await fixturePage("./move-to-project-page.tsx");
  await page.route(/\/settings/, (r) => (r.request().resourceType() === "document" ? r.fulfill({ contentType: "text/html", body }) : r.fallback()));
  await page.goto("/settings");
  await page.locator("[data-rail] [data-signed-in-as]").click();
  await page.getByRole("menuitem", { name: "Send feedback…" }).click();
  const d = page.getByRole("dialog", { name: "Send feedback" });
  await expect(d).toBeVisible();
  return d;
}

test.use({ viewport: { width: 1280, height: 800 } });

test("Send feedback… in the account menu sends to control as you, and says its F- id", async ({ page }) => {
  const sent: Record<string, unknown>[] = [];
  const d = await open(page, async (r) => {
    sent.push(r.request().postDataJSON());
    await json(r, { id: "fbk_1", shown: "F-7K2Q9X" });
  });
  // Signed in: the contact box, on.
  const contact = d.getByRole("checkbox", { name: "You can reach me about this" });
  await expect(contact).toBeChecked();
  await d.getByRole("textbox", { name: "Your feedback" }).fill("Boards by owner\n\nA filter for whose cards they are.");
  await d.getByRole("button", { name: "Idea", exact: true }).click();
  await d.getByRole("button", { name: "Send", exact: true }).click();
  await expect(d.getByText("Thanks — sent as F-7K2Q9X.")).toBeVisible();

  expect(sent).toHaveLength(1);
  const [s] = sent;
  const context = s.context as Record<string, unknown>;
  expect({ ...s, context: undefined }).toEqual({
    kind: "idea",
    title: "Boards by owner",
    body: "A filter for whose cards they are.",
    contact_ok: true,
    source: "person",
    surface: "web",
    agent: null,
    context: undefined,
    who: { install: null, stamp: null },
  });
  expect(context.route).toBe("/settings");
  expect(context.onboarding).toBeNull();
  expect(context.failed).toBeNull();
  expect(["linux", "macos", "windows", null]).toContain(context.os);
});

test("a refusal's code is said as one polite line", async ({ page }) => {
  const d = await open(page, (r) => json(r, { error: "feedback_rate_limited", difficulty: null, retry_after: 120 }, 429));
  await d.getByRole("textbox", { name: "Your feedback" }).fill("It broke");
  await d.getByRole("button", { name: "Bug", exact: true }).click();
  await d.getByRole("button", { name: "Send", exact: true }).click();
  await expect(d.locator("[data-feedback-refused]")).toHaveText("That's a lot of feedback at once. Try again in 2 minutes.");
  await expect(d.getByRole("textbox", { name: "Your feedback" })).toHaveValue("It broke");
});
