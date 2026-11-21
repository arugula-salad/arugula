// Quiet hours as one week (Arugula #1109): the nights, one window, and the
// week they make, drawn, with an overnight window running into the next
// morning. Different times on some days wait behind a link, and "Same times
// every night" folds them back. What's saved keeps QuietHours' shape. On
// #971's fixture page (move-to-project-page.tsx), as settings-scopes.spec.ts;
// a part of Settings › You's Notifications (Arugula #1320).
import { expect, test, type Page, type Route } from "@playwright/test";
import { fixturePage } from "./fixture-page";

type Quiet = { days: number[]; from: number; to: number };

/** Quiet hours' old address, which lands on Notifications, with `quiet` saved; what the page saves, as it sends it. */
async function open(page: Page, quiet: Quiet[]) {
  const saved: { tz?: string; quiet?: Quiet[] }[] = [];
  let prefs = { channels: "mentions", dms: "all", threads: "all", snooze_until: null, tz: "America/New_York", quiet, all_notices: false, paused: false };
  const json = (r: Route, body: unknown, status = 200) => r.fulfill({ status, contentType: "application/json", body: JSON.stringify(body) });
  await page.route(/\/api\//, (r) => json(r, { error: "not here" }, 404));
  await page.route(/\/api\/projects$/, (r) => json(r, { me: { account: "acct_a", login: "sam", name: "Sam" }, projects: [{ id: "prj_aaaa0001", name: "Default", owner: { kind: "personal" }, team: null }], hosted: false }));
  await page.route(/\/api\/notify\/prefs$/, (r) => {
    if (r.request().method() === "POST") {
      const body = r.request().postDataJSON();
      saved.push(body);
      prefs = { ...prefs, ...body };
    }
    return json(r, prefs);
  });
  await page.route(/\/api\/linked-products$/, (r) => json(r, { links: [] }));
  await page.route(/\/api\/agent-tokens$/, (r) => json(r, { mcp_url: "https://example.test/mcp", tokens: [] }));
  await page.route(/\/api\/grants$/, (r) => json(r, { grants: [] }));
  const body = await fixturePage("./move-to-project-page.tsx");
  await page.route(/\/settings/, (r) => (r.request().resourceType() === "document" ? r.fulfill({ contentType: "text/html", body }) : r.fallback()));
  await page.goto("/settings/you/quiet-hours");
  await expect(page).toHaveURL(/\/settings\/you\/notifications$/);
  await expect(page.locator('[data-settings-section="notifications"] [data-settings-part="quiet-hours"]')).toBeVisible();
  return saved;
}

/** A day's bar: its quiet stretches, "from-to". */
const day = (page: Page, name: string) => page.locator("[data-notify-week]").getByRole("img", { name: new RegExp(`^${name}:`) });
const spans = (page: Page, d: number) =>
  page.locator(`[data-notify-week] [data-day="${d}"] i`).evaluateAll((els) => els.map((e) => `${(e as HTMLElement).dataset.from}-${(e as HTMLElement).dataset.to}`));

test.use({ viewport: { width: 1280, height: 900 } });

test("weeknights: one window, the week drawn with each night running into the next morning, and Sunday free", async ({ page }) => {
  await open(page, [{ days: [1, 2, 3, 4, 5], from: 22 * 60, to: 8 * 60 }]);
  await expect(page.locator("[data-notify-window]")).toHaveCount(1);
  await expect(day(page, "Monday")).toHaveAttribute("aria-label", "Monday: 22:00 to 24:00");
  await expect(day(page, "Tuesday")).toHaveAttribute("aria-label", "Tuesday: 00:00 to 08:00, 22:00 to 24:00");
  await expect(day(page, "Saturday")).toHaveAttribute("aria-label", "Saturday: 00:00 to 08:00");
  await expect(day(page, "Sunday")).toHaveAttribute("aria-label", "Sunday: none");
  // The split waits behind a link; there's nothing to fold back.
  await expect(page.locator("[data-notify-add-window]")).toHaveText("Different times on some days…");
  await expect(page.locator("[data-notify-same-times]")).toHaveCount(0);
  // The week follows what's being edited, before it's saved.
  await page.getByRole("group", { name: "Quiet days", exact: true }).getByRole("button", { name: "Sunday" }).click();
  expect(await spans(page, 7)).toEqual(["22:00-24:00"]);
  expect(await spans(page, 1)).toEqual(["00:00-08:00", "22:00-24:00"]);
  await page.getByLabel("Quiet hours to", { exact: true }).fill("06:30");
  expect(await spans(page, 1)).toEqual(["00:00-06:30", "22:00-24:00"]);
});

test("different times on some days: a window of their own, saved as QuietHours, and folded back by same times every night", async ({ page }) => {
  const saved = await open(page, [{ days: [1, 2, 3, 4, 5], from: 22 * 60, to: 8 * 60 }]);
  await page.locator("[data-notify-add-window]").click();
  await expect(page.locator("[data-notify-window]")).toHaveCount(2);
  // Friday moves to the new window, with Saturday, and they sleep in.
  const other = page.getByRole("group", { name: "Quiet days, other hours" });
  await other.getByRole("button", { name: "Friday" }).click();
  await other.getByRole("button", { name: "Saturday" }).click();
  await page.getByLabel("Other quiet hours from").fill("23:30");
  await page.getByLabel("Other quiet hours to").fill("10:00");
  await expect(page.getByRole("group", { name: "Quiet days", exact: true }).getByRole("button", { name: "Friday" })).toHaveAttribute("aria-pressed", "false");
  expect(await spans(page, 5)).toEqual(["00:00-08:00", "23:30-24:00"]);
  expect(await spans(page, 7)).toEqual(["00:00-10:00"]);
  await expect(page.locator("[data-notify-add-window]")).toHaveText("Different times on more days…");
  await page.locator("[data-notify-save]").click();
  await expect.poll(() => saved.length).toBe(1);
  expect(saved[0]).toEqual({
    tz: "America/New_York",
    quiet: [
      { days: [1, 2, 3, 4], from: 22 * 60, to: 8 * 60 },
      { days: [5, 6], from: 23 * 60 + 30, to: 10 * 60 },
    ],
  });
  await expect(page.locator('[data-settings-part="quiet-hours"]')).toHaveAttribute("data-settings-summary", "Mon–Sat, times vary · New York");
  // Back to one window: every night they covered, at the first one's times.
  await page.locator("[data-notify-same-times]").click();
  await expect(page.locator("[data-notify-window]")).toHaveCount(1);
  expect(await spans(page, 7)).toEqual(["00:00-08:00"]);
  await page.locator("[data-notify-save]").click();
  await expect.poll(() => saved.length).toBe(2);
  expect(saved[1].quiet).toEqual([{ days: [1, 2, 3, 4, 5, 6], from: 22 * 60, to: 8 * 60 }]);
  await expect(page.locator('[data-settings-part="quiet-hours"]')).toHaveAttribute("data-settings-summary", "Mon–Sat 10 PM – 8 AM · New York");
});

test("off: an empty week and no link until a night is picked", async ({ page }) => {
  await open(page, []);
  await expect(page.locator('[data-settings-part="quiet-hours"]')).toHaveAttribute("data-settings-summary", "Off");
  await expect(page.locator("[data-notify-week] i")).toHaveCount(0);
  await expect(page.locator("[data-notify-add-window]")).toHaveCount(0);
  await page.getByRole("group", { name: "Quiet days", exact: true }).getByRole("button", { name: "Wednesday" }).click();
  expect(await spans(page, 3)).toEqual(["22:00-24:00"]);
  expect(await spans(page, 4)).toEqual(["00:00-08:00"]);
  await expect(page.locator("[data-notify-add-window]")).toBeVisible();
});
