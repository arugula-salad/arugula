// M36: a pull request as a block, against a fake Forgejo served here (S23's
// recorded #84, changed as each test needs) and the stand-in `tea` the
// config puts on the daemon's PATH (this spec writes its logins).
//
// Opened from a pane's menu ("Open pull request…"), the block shows the PR;
// a review asked of you waits first and is approved there, reaching the
// forge as a review with your login's token. An agent's comment is a
// draft: a card on the block with the text in a box to edit, sent from
// here (the edited text is what's posted), and the block says who sent it.
// Nothing here reaches a real forge.

import { readFileSync, writeFileSync } from "node:fs";
import { createServer, type Server, type ServerResponse } from "node:http";
import { join } from "node:path";
import { expect, test, type Page } from "@playwright/test";
import { menu, paneEl, panes, reset } from "./helpers";
import { listen } from "./ports";

const TOKEN = "e2e-forge-token";
const REPO = "jhgaylor/illogical";
const fixture = (f: string) =>
  JSON.parse(readFileSync(new URL(`../../crates/daemon/tests/fixtures/forgejo/forgejo-illogical-84/${f}`, import.meta.url), "utf8"));

let origin = "";
let server: Server;
let item: Record<string, any>;
let reviews: Record<string, unknown>[] = [];
let timeline: Record<string, unknown>[] = [];
const writes: { route: string; body: any; auth: string }[] = [];
let n = 0;

function json(res: ServerResponse, status: number, v: unknown, headers: Record<string, string> = {}) {
  res.writeHead(status, { "content-type": "application/json", ...headers }).end(JSON.stringify(v));
}

function touch() {
  n += 1;
  item.updated_at = `2026-10-03T03:${String(Math.floor(n / 60) % 60).padStart(2, "0")}:${String(n % 60).padStart(2, "0")}Z`;
}

/** #84, open again, by someone else, a review asked of jhgaylor. */
function fresh() {
  item = fixture("item.json");
  Object.assign(item, { state: "open", merged: false, merged_at: null, html_url: `${origin}/${REPO}/pulls/84` });
  item.user.login = "sam";
  item.requested_reviewers = [{ login: "jhgaylor" }];
  reviews = [{ id: 1, state: "REQUEST_REVIEW", user: { login: "jhgaylor" }, submitted_at: "2026-10-02T23:40:00Z" }];
  timeline = fixture("timeline.json").slice(0, 1);
  writes.length = 0;
  touch();
}

test.describe.configure({ mode: "serial" });

test.beforeAll(async () => {
  server = createServer((req, res) => {
    const u = new URL(req.url!, "http://forge");
    if (req.headers.authorization !== `token ${TOKEN}`) return json(res, 401, { message: "token is required" });
    const p = u.pathname.replace(`/api/v1/repos/${REPO}`, "");
    let body = "";
    req.on("data", (d) => (body += d));
    req.on("end", () => {
      if (req.method === "GET") {
        if (u.pathname === "/api/v1/user") return json(res, 200, { id: 1, login: "jhgaylor" });
        if (u.pathname === "/api/v1/user/teams") return json(res, 200, []);
        if (p === "/pulls/84") return json(res, 200, item);
        if (p === "/pulls/84/reviews") return json(res, 200, reviews);
        if (p.startsWith("/commits/") && p.endsWith("/status")) return json(res, 200, { state: "success", total_count: 2, statuses: fixture("statuses.json").statuses });
        if (p === "/issues/84/timeline") return json(res, 200, timeline, { "x-total-count": String(timeline.length) });
        return json(res, 404, { message: "not here" });
      }
      const b = JSON.parse(body || "{}");
      writes.push({ route: p, body: b, auth: req.headers.authorization! });
      n += 1;
      if (p === "/pulls/84/reviews") {
        reviews.push({ id: 100 + n, state: b.event, user: { login: "jhgaylor" }, body: b.body, submitted_at: "2026-10-03T02:00:00Z" });
        touch();
        return json(res, 200, { id: 100 + n, html_url: `${origin}/${REPO}/pulls/84#issuecomment-${100 + n}` });
      }
      if (p === "/issues/84/comments") {
        timeline.push({ id: 7000 + n, type: "comment", created_at: "2026-10-03T02:00:00Z", user: { login: "jhgaylor" }, body: b.body });
        item.comments += 1;
        touch();
        return json(res, 201, { id: 7000 + n, html_url: `${origin}/${REPO}/pulls/84#issuecomment-${7000 + n}` });
      }
      json(res, 404, { message: "not here" });
    });
  });
  origin = `http://127.0.0.1:${await listen(server)}`;
  const tea = process.env.ARUGULA_E2E_TEA_DIR!;
  writeFileSync(join(tea, "logins.json"), JSON.stringify([{ name: "e2e", url: origin, ssh_host: "", user: "jhgaylor", default: "false" }]));
});

test.afterAll(() => {
  server.close();
  writeFileSync(join(process.env.ARUGULA_E2E_TEA_DIR!, "logins.json"), "[]");
});

async function openPr(page: Page): Promise<number> {
  await reset(page);
  const term = (await panes(page))[0];
  await menu(page, paneEl(page, term), "Open pull request…");
  await page.locator(".prompt input").fill(`${origin}/${REPO}/pulls/84`);
  await page.keyboard.press("Enter");
  await expect.poll(async () => (await panes(page)).length).toBe(2);
  return page.evaluate(() => window.__arugula.client.state!.panes.find((p) => p.type === "forge")!.id);
}

test("a pull request opens from the menu; the review asked of you is approved there", async ({ page }) => {
  fresh();
  const block = await openPr(page);
  const el = page.locator(`[data-forge-block="${block}"]`);
  // #317: the title first, then the number and the repo.
  await expect(el.locator("[data-item-head] .item-number")).toHaveText("#84");
  await expect(el.locator("[data-item-head] .item-title")).toHaveText("CI: delete the kept build once it passes 30 GB");
  await expect.poll(() => page.evaluate((b) => window.__arugula.client.title(b), block)).toMatch(/^#84 CI: delete the kept build/);
  await expect(el.locator("[data-item-pill]")).toHaveText("Open");
  await expect(el.locator("[data-check]")).toHaveCount(2);
  // What waits on you is the one strip, and it's on the rail too.
  await expect(el.locator('[data-item-next="warn"]')).toContainText("Your review is asked for");
  await expect.poll(() => page.evaluate((b) => window.__arugula.client.info(b)?.reason?.kind, block)).toBe("gate");
  await el.locator("[data-approve-review]").click();
  await expect.poll(() => writes.length).toBe(1);
  expect(writes[0]).toEqual({ route: "/pulls/84/reviews", body: { event: "APPROVED", body: "" }, auth: `token ${TOKEN}` });
  // Read back: approved, and no longer asked.
  await expect(el.locator('[data-item-avatar][data-name="jhgaylor"]')).toHaveAttribute("title", "jhgaylor approved");
  await expect(el.locator('[data-item-avatar][data-ring]')).toHaveCount(0);
  await expect(el.locator("[data-item-next=warn]")).toHaveCount(0);
  await el.locator('[data-item-tab="reviews"]').click();
  await expect(el.locator('[data-review="approved"]')).toBeVisible();
  await expect.poll(() => page.evaluate((b) => window.__arugula.client.info(b)?.reason ?? null, block)).toBeNull();
});

test("a description is drawn as Markdown, with the status chips and the actions held at the foot", async ({ page }) => {
  fresh();
  item.body = [
    "Does **a lot** with `inline code`, see #5.",
    "",
    "- one",
    "- two",
    "",
    "[x](javascript:alert(1)) <img src=x onerror=alert(1)>",
    "",
    "<details><summary>The *old* plan</summary>",
    "",
    "- [x] folded",
    "</details>",
    "",
    "<details onclick=alert(1)>",
    "",
    ...Array.from({ length: 80 }, (_, i) => `line ${i}`).join("\n\n").split("\n"),
  ].join("\n");
  const block = await openPr(page);
  const el = page.locator(`[data-forge-block="${block}"]`);
  const desc = el.locator(".forge-desc");
  await expect(desc.locator("li")).toHaveCount(3);
  await expect(desc.locator("strong")).toHaveText("a lot");
  await expect(desc.locator("code")).toHaveText("inline code");
  await expect(desc).not.toContainText("**");
  await expect(desc.locator('a[href$="/issues/5"]')).toHaveText("#5");
  // Whoever opened the PR wrote it: nothing in it runs or loads.
  await expect(desc).toContainText("<img src=x onerror=alert(1)>");
  await expect(el.locator('a[href^="javascript:"]')).toHaveCount(0);
  await expect(el.locator("img")).toHaveCount(0);
  // GitHub's fold folds, its summary drawn as Markdown; one with more on
  // its line than `open` is text.
  await expect(desc.locator("details > summary em")).toHaveText("old");
  await expect(desc.locator("details li.md-task input")).toBeChecked();
  await expect(desc.locator("details")).toHaveCount(1);
  await expect(desc).toContainText("<details onclick=alert(1)>");
  // Where it stands, in the facts row: icons and counts, no words.
  await expect(el.locator('[data-item-fact="checks"]')).toContainText("✓ 2");
  await expect(el.locator('[data-item-fact="reviewers"] [data-item-avatar][data-ring="asked"]')).toHaveCount(1);
  await expect(el.locator('[data-item-fact="mergeable"]')).toHaveAttribute("data-mergeable", "clean");
  await expect(el.locator("[data-item-source]")).toHaveAttribute("title", /tea login e2e/);
  // #408: the description folds to a few lines with More, so the long one
  // doesn't push the actions away; they're under it.
  await expect(el.locator("[data-item-more]")).toHaveText("More");
  const foot = el.locator(".item-foot");
  await expect(foot.locator("button", { hasText: "Comment" })).toBeVisible();
  await expect(el.locator("[data-item-source]")).toHaveAttribute("title", /polls/);
});

test("an agent's comment waits as a draft until a person edits and sends it", async ({ page }) => {
  fresh();
  item.requested_reviewers = [];
  reviews = [];
  const block = await openPr(page);
  const el = page.locator(`[data-forge-block="${block}"]`);
  await expect(el.locator("[data-item-head] .item-number")).toHaveText("#84");
  // An agent's write (the CLI under Claude Code sends agent: true).
  const r = await page.evaluate(
    (b) => window.__arugula.client.request("POST", `/api/blocks/${b}/call/comment`, { body: "LGTM, one nit", agent: true }).then((r) => r.json<{ draft: string }>()),
    block,
  );
  expect(r.draft).toBeTruthy();
  expect(writes).toEqual([]);
  // The card: who drafted what, the text in a box to edit.
  const card = page.locator(`.pane-ask .ask[data-ask="${r.draft}"]`);
  await expect(card).toContainText("an agent drafted a comment on jhgaylor/illogical#84");
  // #317: the card sits above the block's actions at the foot, not over them.
  const foot = el.locator(".item-foot");
  await expect(foot).toBeVisible();
  await expect.poll(async () => {
    const [c, f] = await Promise.all([page.locator(".pane-ask").boundingBox(), foot.boundingBox()]);
    return !!c && !!f && c.y + c.height <= f.y + 0.5;
  }).toBe(true);
  await expect(foot.locator("button, a").first()).toBeVisible();
  const box = card.locator('textarea[name="body"]');
  await expect(box).toHaveValue("LGTM, one nit");
  // The strip says the draft is yours, no sentence; its card has Send.
  await expect(el.locator('[data-item-next="warn"]')).toContainText("Your comment draft");
  await expect(el.locator("[data-send-draft]")).toHaveCount(0);
  await expect(el).not.toContainText("A draft waits");
  await box.fill("LGTM, one small nit.\n\nThanks!");
  await card.locator("[data-ask-submit]").click();
  await expect.poll(() => writes.length).toBe(1);
  expect(writes[0].route).toBe("/issues/84/comments");
  expect(writes[0].body).toEqual({ body: "LGTM, one small nit.\n\nThanks!" });
  expect(writes[0].auth).toBe(`token ${TOKEN}`);
  // The block says who sent it, and the comment's in its timeline.
  await el.locator('[data-item-tab="activity"]').click();
  await expect(el.locator(`[data-draft="${r.draft}"]`)).toHaveAttribute("data-draft-status", "sent");
  await expect(el.locator("[data-send-draft]")).toHaveCount(0);
  await expect(el.locator(`[data-draft="${r.draft}"]`)).toContainText("sent by");
  await expect(el.locator("[data-forge-timeline]")).toContainText("LGTM, one small nit.");
  await expect(card).toHaveCount(0);

  // Another, dropped: nothing goes.
  const d = await page.evaluate(
    (b) => window.__arugula.client.request("POST", `/api/blocks/${b}/call/merge`, { agent: true }).then((r) => r.json<{ draft: string }>()),
    block,
  );
  const merge = page.locator(`.pane-ask .ask[data-ask="${d.draft}"]`);
  await expect(merge).toContainText("drafted a merge");
  await merge.locator("[data-ask-decline]").click();
  await expect(el.locator(`[data-draft="${d.draft}"]`)).toHaveAttribute("data-draft-status", "dropped");
  expect(writes.length).toBe(1);
});

test("a waiting draft is the strip's one item, sent from its card, and goes once the PR is merged", async ({ page }) => {
  fresh();
  item.requested_reviewers = [];
  reviews = [];
  const block = await openPr(page);
  const el = page.locator(`[data-forge-block="${block}"]`);
  const r = await page.evaluate(
    (b) => window.__arugula.client.request("POST", `/api/blocks/${b}/call/comment`, { body: "Ship it", agent: true }).then((r) => r.json<{ draft: string }>()),
    block,
  );
  expect(writes).toEqual([]);
  await expect(el.locator('[data-item-next="warn"]')).toContainText("Your comment draft");
  await page.locator(`.pane-ask .ask[data-ask="${r.draft}"] [data-ask-submit]`).click();
  await expect.poll(() => writes.length).toBe(1);
  expect(writes[0].route).toBe("/issues/84/comments");
  expect(writes[0].body).toEqual({ body: "Ship it" });
  await expect(page.locator(`.pane-ask .ask[data-ask="${r.draft}"]`)).toHaveCount(0);
  // Merged since: a draft still waiting isn't a want, and isn't drawn.
  const d = await page.evaluate(
    (b) => window.__arugula.client.request("POST", `/api/blocks/${b}/call/comment`, { body: "Too late", agent: true }).then((r) => r.json<{ draft: string }>()),
    block,
  );
  Object.assign(item, { state: "closed", merged: true, merged_at: "2026-10-03T02:00:00Z", merged_by: { login: "jhgaylor" } });
  touch();
  await page.evaluate((b) => window.__arugula.client.api(`/api/blocks/${b}/call/refresh`, {}, ""), block);
  await expect(el.locator("[data-item-pill]")).toHaveText("Merged");
  await expect(el.locator("[data-item-next], [data-send-draft], [data-forge-drafts]")).toHaveCount(0);
  expect(d.draft).toBeTruthy();
});

test("a merged pull request isn't waiting on you: the tag says merged once", async ({ page }) => {
  fresh();
  Object.assign(item, { state: "closed", merged: true, merged_at: "2026-10-03T02:00:00Z", merged_by: { login: "jhgaylor" } });
  item.requested_reviewers = [];
  item.user.login = "jhgaylor";
  touch();
  const block = await openPr(page);
  const el = page.locator(`[data-forge-block="${block}"]`);
  await expect(el.locator("[data-item-pill]")).toHaveText("Merged");
  // Nothing left to act on: it opens on Activity, not a red Checks.
  await expect(el.locator('[data-item-tab][aria-selected="true"]')).toHaveAttribute("data-item-tab", "activity");
  // Never a want after the end: no strip, no waiting draft, no finished news.
  await expect(el.locator("[data-item-next]")).toHaveCount(0);
  await expect(el.locator("[data-forge-wants], [data-forge-done], [data-send-draft], [data-forge-drafts]")).toHaveCount(0);
  await expect(el).not.toContainText("Waiting on you");
  await expect(el).not.toContainText("waits for you");
  // Open and green, the news is one quiet line, not a request.
  fresh();
  item.requested_reviewers = [];
  item.user.login = "jhgaylor";
  item.mergeable = true;
  reviews = [];
  touch();
  const open = await openPr(page);
  const el2 = page.locator(`[data-forge-block="${open}"]`);
  // Open and green, nothing but the ready strip, and no finished news.
  await expect(el2.locator('[data-item-next="ok"]')).toContainText("Ready to merge");
  await expect(el2.locator("[data-forge-done], [data-forge-wants]")).toHaveCount(0);
  // Phone width: the long title wraps or ends in an ellipsis, nothing scrolls sideways.
  await page.setViewportSize({ width: 390, height: 800 });
  await expect.poll(() => el2.evaluate((e) => [...e.querySelectorAll<HTMLElement>(".review-body")].every((x) => x.scrollWidth <= x.clientWidth))).toBe(true);
});

test("with labs on, Forgejo's and GitLab's links in terminal output open as blocks", async ({ page }) => {
  await reset(page);
  const kinds = page.evaluate(() =>
    ["https://git.example.test/o/r/pulls/3", "https://gitlab.com/g/p/-/merge_requests/2", "https://github.com/o/r/pull/3"].map((u) =>
      (window.__arugula.client as unknown as { forgeLink(u: string): string | null }).forgeLink(u),
    ),
  );
  expect(await kinds).toEqual(["pr", "pr", "pr"]);
});
