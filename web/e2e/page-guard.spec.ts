// A page that throws while drawing says so in its place, with Try again
// (Arugula #615), instead of staying on its last frame. Here an issue page
// is given its events without the list it reads, as an older server's
// answer might be: the rail goes on working, the error reaches the console,
// and Try again, once the answer is whole, draws the page. On #971's fixture page
// (move-to-project-page.tsx): the app's own pages, with this spec's answers.
import { expect, test, type Page, type Route } from "@playwright/test";
import { fixturePage } from "./fixture-page";

const P = "prj_aaaa0001";
const ISSUE = "iss_0000c0de";
/** Where `GUARD_SHOTS=dir` puts the pictures (the PR's Before | After). */
const SHOTS = process.env.GUARD_SHOTS;

const projects = [{ id: P, name: "Default", owner: { kind: "personal" }, team: null }];
const issue = {
  id: ISSUE,
  project: P,
  number: 615,
  title: "A render error freezes a page with no message",
  body: "",
  state: "open",
  close_reason: null,
  author: "acct_a",
  agent: null,
  labels: [],
  assignees: [],
  github: null,
  created_at: 1,
  updated_at: 1,
  closed_at: null,
};

/** The fixture page at the issue; `whole()` says which answer it gets. */
async function open(page: Page, whole: () => boolean) {
  const json = (r: Route, body: unknown, status = 200) => r.fulfill({ status, contentType: "application/json", body: JSON.stringify(body) });
  // What this spec doesn't answer, nobody does.
  await page.route(/\/api\//, (r) => json(r, { error: "not here" }, 404));
  await page.route(/\/api\/projects$/, (r) => json(r, { me: { account: "acct_a", login: "sam", name: "Sam" }, projects, hosted: false }));
  await page.route(new RegExp(`/api/issues/${ISSUE}$`), (r) => json(r, issue));
  await page.route(new RegExp(`/api/issues/${ISSUE}/events$`), (r) => json(r, whole() ? { events: [] } : {}));
  const body = await fixturePage("./move-to-project-page.tsx");
  await page.route(/\/p\/prj_/, (r) => (r.request().resourceType() === "document" ? r.fulfill({ contentType: "text/html", body }) : r.fallback()));
  await page.goto(`/p/${P}/issues/${ISSUE}`);
}

test.use({ viewport: { width: 1280, height: 720 } });

test("a page that throws says so with Try again, and the rest of the app works", async ({ page }) => {
  const errors: string[] = [];
  page.on("console", (m) => void (m.type() === "error" && errors.push(m.text())));
  let whole = false;
  await open(page, () => whole);

  const failed = page.locator("main [data-page-failed]");
  await expect(failed).toBeVisible();
  await expect(failed).toHaveAttribute("role", "alert");
  await expect(failed.locator(".block-error-title")).toHaveText("Can't show this page");
  await expect(failed.locator(".block-error-detail")).toHaveText("Something in it went wrong while drawing it. The rest of Arugula still works.");
  expect(errors.some((e) => e.startsWith("A page failed to draw"))).toBe(true);
  // The frame is the page's: the rail and the bar are there.
  await expect(page.locator("[data-platform-bar]")).toBeVisible();
  if (SHOTS) await page.screenshot({ path: `${SHOTS}/after-failed.png` });

  // Try again, once the answer is whole: the page draws.
  whole = true;
  await failed.getByRole("button", { name: "Try again" }).click();
  await expect(failed).toHaveCount(0);
  await expect(page.getByText(issue.title).first()).toBeVisible();
  if (SHOTS) await page.screenshot({ path: `${SHOTS}/after-retried.png` });
});

test("another page draws: only the failed page was replaced", async ({ page }) => {
  await open(page, () => false);
  await expect(page.locator("main [data-page-failed]")).toBeVisible();
  // The rail still goes places, and the next page isn't the failed one's note.
  await page.locator(`a[href^="/p/${P}"][href$="/docs"]`).first().click();
  await expect(page).toHaveURL(/\/docs$/);
  await expect(page.locator("main [data-page-failed]")).toHaveCount(0);
});
