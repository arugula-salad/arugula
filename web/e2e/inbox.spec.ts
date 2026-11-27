// The Inbox (Lift 6d, Arugula #1486; v6 shot 7): every project's items,
// grouped by project, the one you're in first; a teammate's asks folded;
// each row answered in place; `↵` goes to an item's place in another
// project, and `esc` comes back. The rail's Inbox badge is the server's
// `count`, and each project's mark its `by_project`. Beside it, the agent
// panel peeks the asker of the row the keys are on (#1528). On the app
// frame's fixture page (app-frame-page.tsx), whose `/api/inbox` is the spec's.
import { expect, test, type Page } from "@playwright/test";
import type { InboxItem } from "../src/shared/proto.gen";
import { overview, paneItem, say } from "./overview-fixture";
import { away, openApp, P as ARUGULA, SITE } from "./stack-steps";

test.use({ permissions: ["local-network-access"], viewport: { width: 1440, height: 900 } });

const NOW = Date.now();
const item = (id: string, kind: string, group: string, title: string, subject: Partial<InboxItem["subject"]>, more: Partial<InboxItem> = {}): InboxItem =>
  ({ id, kind, group, counts: true, title, from: {}, subject: { type: "issue", id, url: "", ...subject }, since: NOW - 60_000, ...more }) as InboxItem;

/** Arugula: an approval you answer here, and Jo's ask; Site: a mention and a failed check (an alert, not counted); a DM in no project. */
const ITEMS: InboxItem[] = [
  item("prp_1", "approval", "decide", "Start a worker on #12?", { project: ARUGULA, url: `/p/${ARUGULA}/boards` }, { answer: { via: "card", options: ["Start", "Not now"] }, lines: ["Why: it's ready"] }),
  item("prp_2", "agent_ask", "decide", "Ship it on Friday?", { project: ARUGULA }, { for: "acct_b", counts: false }),
  item("iss_7:mentioned", "mention", "named", "#7 Copy for the launch", { project: SITE, url: `/p/${SITE}/boards` }, { from: { name: "Jo" } }),
  item("run_3:checks_failed", "checks_failed", "named", "pr-gate failed on your PR #71", { project: SITE }, { counts: false }),
  item("room:dm_1", "dm", "chat", "Jo", { type: "room", url: "/chat" }, { snippet: "lunch?" }),
];

async function openInbox(page: Page) {
  await page.addInitScript((items) => void ((window as unknown as { __inbox: unknown }).__inbox = items), ITEMS);
  await openApp(page, `/p/${ARUGULA}`);
  await page.locator("[data-rail] [data-open-inbox]").click();
  await expect(page).toHaveURL(/\/inbox$/);
  await expect(page.locator("[data-platform-inbox]")).toBeVisible();
}

const group = (page: Page, project: string) => page.locator(`[data-inbox-project="${project}"]`);
const row = (page: Page, id: string) => page.locator(`[data-need-item="${id}"]`);

test("every project's items, grouped by project: the one you're in first, a teammate's asks folded, no project last", async ({ page }) => {
  await openInbox(page);
  const order = await page.locator("[data-inbox-project]").evaluateAll((els) => els.map((e) => e.getAttribute("data-inbox-project")));
  expect(order[0]).toBe(ARUGULA);
  expect(order.at(-1)).toBe("none");
  expect(order).toEqual(expect.arrayContaining([SITE, "prj_aaaa0001"]));
  await expect(group(page, ARUGULA).locator("[data-need-item]")).toHaveCount(1);
  await expect(row(page, "prp_1")).toHaveAttribute("data-need", "approval");
  // Jo's ask is folded, and opens on a click.
  const fold = group(page, ARUGULA).locator("[data-inbox-for=acct_b]");
  await expect(fold).toContainText("1 ask for Jo ›");
  await expect(row(page, "prp_2")).toHaveCount(0);
  await fold.locator("button[aria-expanded]").click();
  await expect(row(page, "prp_2")).toBeVisible();
  // Site: what names you, then its alert, which isn't counted.
  await expect(group(page, SITE).locator("[data-need-item]")).toHaveCount(2);
  await expect(group(page, SITE).locator("[data-need-item]").first()).toHaveAttribute("data-need-item", "iss_7:mentioned");
  await expect(group(page, SITE).locator("[data-inbox-project-count]")).toHaveAttribute("data-inbox-project-count", "1");
  await expect(group(page, "prj_aaaa0001")).toContainText("nothing for you");
  await expect(group(page, "none").locator(`[data-need-item="room:dm_1"]`)).toBeVisible();
});

test("the rail's badge is the server's count, and each project's mark its share", async ({ page }) => {
  await openInbox(page);
  const count = await page.locator("[data-platform-inbox]").getAttribute("data-inbox-count");
  // The approval, the mention and the DM: not Jo's ask, not the alert.
  expect(count).toBe("3");
  await expect(page.locator("[data-rail] [data-open-inbox] [data-count]")).toHaveText(count!);
  await expect(page.locator(`[data-rail] [data-project-mark="${ARUGULA}"] [data-count]`)).toHaveText("1");
  await expect(page.locator(`[data-rail] [data-project-mark="${SITE}"] [data-count]`)).toHaveText("1");
  // The stream says it changed: the badge follows the server, not the rows.
  await page.evaluate((items) => (window as unknown as { __inboxSay(i: unknown[]): void }).__inboxSay(items), ITEMS.slice(2));
  await expect(page.locator("[data-rail] [data-open-inbox] [data-count]")).toHaveText("2");
  await expect(page.locator(`[data-rail] [data-project-mark="${ARUGULA}"] [data-count]`)).toHaveCount(0);
});

test("a row answers in place, and stays as its receipt", async ({ page }) => {
  await openInbox(page);
  await row(page, "prp_1").locator("button[aria-expanded]").first().click();
  await row(page, "prp_1").getByRole("button", { name: /^Start/ }).click();
  await expect.poll(() => page.evaluate(() => (window as unknown as { __inboxPosted?: unknown[] }).__inboxPosted)).toEqual([{ id: "prp_1", choice: "Start" }]);
  await expect(row(page, "prp_1")).toContainText("Start by you");
});

test("↵ goes to an item's place in another project, and esc comes back", async ({ page }) => {
  await openInbox(page);
  await away(page);
  // j to the mention: past Arugula's approval.
  await page.keyboard.press("j");
  await expect(row(page, "prp_1")).toHaveAttribute("data-selected", "true");
  await page.keyboard.press("j");
  await expect(row(page, "iss_7:mentioned")).toHaveAttribute("data-selected", "true");
  await page.keyboard.press("Enter");
  await expect(page).toHaveURL(new RegExp(`/p/${SITE}[^/]*/boards`));
  await expect(page.locator(`[data-rail] [data-project-mark="${SITE}"]`)).toHaveAttribute("aria-current", "true");
  // Boards' filter takes the keys as it draws: the page's, then Escape.
  await away(page);
  await page.keyboard.press("Escape");
  await expect(page).toHaveURL(/\/inbox$/);
  await expect(page.locator("[data-platform-inbox]")).toBeVisible();
});

test("moving through the rows peeks each asker in the panel beside it: an agent's session, then the chief of staff", async ({ page }) => {
  const { term, agent } = await overview(page);
  const host = await page.evaluate((t) => (window as unknown as { __app: { fleet: { panes: { id: number; host: string }[] } } }).__app.fleet.panes.find((p) => p.id === t)!.host, term);
  const chief = { ...ITEMS[0], id: "prp_cos", title: "Start a worker on #12?", from: { name: "Chief of staff · Arugula" }, since: NOW };
  // The agent's block on #12 asks, and so does the chief of staff.
  await say(page, [{ ...paneItem(host, agent), title: "Which colour?" }, chief]);
  await page.locator("[data-rail] [data-open-inbox]").click();
  await expect(row(page, `${host}.${agent}:1`)).toBeVisible();
  const panel = page.locator("[data-inbox-side] [data-kit=panel]");
  await expect(panel.locator('[data-tab="chief"]')).toHaveAttribute("aria-selected", "true");
  await away(page);
  await page.keyboard.press("j");
  await expect(panel.locator('[data-tab^="agent:"]')).toHaveAttribute("aria-selected", "true");
  await expect(panel.locator("[data-panel-transcript]")).toBeVisible();
  await page.keyboard.press("j");
  await expect(row(page, "prp_cos")).toHaveAttribute("data-selected", "true");
  await expect(panel.locator('[data-tab="chief"]')).toHaveAttribute("aria-selected", "true");
  // Answered in the row, it's gone from the list once the server says so, and the panel stays.
  await row(page, "prp_cos").getByRole("button", { name: /^Start/ }).click();
  await expect(row(page, "prp_cos")).toContainText("Start by you");
  await expect(panel).toBeVisible();
});
