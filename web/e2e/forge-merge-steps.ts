// #427: a PR block's Merge ▾ against a fake GitHub that is squash-only (the
// shape of arugula-salad/arugula), shared by the Chromium and WebKit specs.
// Two things: Merge offers only what the repository allows, including when
// the first ask for it failed and the PR has not moved since (a 304 read
// is not "moved"); and picking a method doesn't merge, the Confirm button does.

import { readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { createHash } from "node:crypto";
import { createServer, type Server, type ServerResponse } from "node:http";
import { expect, test, type Page } from "@playwright/test";
import { reset } from "./helpers";
import { listen } from "./ports";

const TOKEN = "e2e-github-token";
const REPO = "cli/cli";
const N = 13788;
const fixture = (f: string) =>
  JSON.parse(readFileSync(new URL(`../../crates/daemon/tests/fixtures/github/github-cli-cli-13788/${f}`, import.meta.url), "utf8"));

export function mergeSpecs(tag: string) {
  let origin = "";
  let server: Server;
  let item: Record<string, any>;
  // How the fake has been asked: the repository's methods, the PR (304s), and merges.
  let repoFails = 0;
  let repoReads = 0;
  let pullNotModified = 0;
  const merges: { style: string; auth: string }[] = [];

  function answer(res: ServerResponse, inm: string | undefined, v: unknown, pull = false) {
    const body = JSON.stringify(v);
    const etag = `"${createHash("sha1").update(body).digest("hex")}"`;
    const h = { etag, "x-ratelimit-limit": "5000", "x-ratelimit-remaining": "4900", "x-ratelimit-resource": "core" };
    if (inm === etag) {
      if (pull) pullNotModified += 1;
      return res.writeHead(304, h).end();
    }
    res.writeHead(200, { "content-type": "application/json", ...h }).end(body);
  }

  test.beforeAll(async () => {
    item = fixture("item.json");
    item.user.login = "jhgaylor";
    item.requested_reviewers = [];
    // Green and settled: the strip's own Merge ▾ shows too.
    const runs = fixture("check_runs.json").check_runs.map((c: any) => ({ ...c, conclusion: "success", status: "completed" }));
    server = createServer((req, res) => {
      const u = new URL(req.url!, "http://github");
      if (req.headers.authorization !== `Bearer ${TOKEN}`) return res.writeHead(401).end('{"message":"Bad credentials"}');
      const inm = req.headers["if-none-match"] as string | undefined;
      const p = u.pathname.replace(`/repos/${REPO}`, "");
      let raw = "";
      req.on("data", (c) => (raw += c));
      req.on("end", () => {
        if (req.method === "PUT" && p === `/pulls/${N}/merge`) {
          merges.push({ style: JSON.parse(raw).merge_method, auth: req.headers.authorization! });
          Object.assign(item, { state: "closed", merged: true, merged_at: "2026-10-03T02:00:00Z", merged_by: { login: "jhgaylor" } });
          return res.writeHead(200, { "content-type": "application/json" }).end('{"merged":true}');
        }
        if (u.pathname === "/user") return answer(res, inm, { login: "jhgaylor" });
        if (u.pathname === "/user/teams") return answer(res, inm, []);
        if (p === "") {
          repoReads += 1;
          if (repoFails > 0) {
            repoFails -= 1;
            return res.writeHead(500, { "content-type": "application/json" }).end('{"message":"try later"}');
          }
          return answer(res, inm, { full_name: REPO, allow_merge_commit: false, allow_squash_merge: true, allow_rebase_merge: false });
        }
        if (p === `/pulls/${N}`) return answer(res, inm, item, true);
        if (p.endsWith("/check-runs")) return answer(res, inm, { total_count: runs.length, check_runs: runs });
        if (p.endsWith("/status")) return answer(res, inm, { state: "success", total_count: 0, statuses: [] });
        if (p === `/pulls/${N}/reviews`) return answer(res, inm, fixture("reviews.json"));
        if (p === `/issues/${N}/timeline`) return answer(res, inm, fixture("timeline.json"));
        res.writeHead(404).end('{"message":"Not Found"}');
      });
    });
    origin = `http://127.0.0.1:${await listen(server)}`;
    writeFileSync(join(process.env.ARUGULA_E2E_TEA_DIR!, "gh-hosts"), `${origin.replace("http://", "")}\n`);
  });

  test.afterAll(() => {
    server.close();
    writeFileSync(join(process.env.ARUGULA_E2E_TEA_DIR!, "gh-hosts"), "");
  });

  test.beforeEach(() => {
    repoFails = 0;
    repoReads = 0;
    pullNotModified = 0;
    merges.length = 0;
    Object.assign(item, { state: "open", merged: false, merged_at: null, merged_by: null });
  });

  const open = (page: Page) =>
    page.evaluate(
      ([api, repo, number]) =>
        window.__arugula.client
          .request("POST", "/api/blocks", { type: "forge", config: { provider: "github", api, repo, number }, local: true })
          .then((r) => r.json<{ block: number }>())
          .then((r) => r.block),
      [origin, REPO, N] as const,
    );

  test(`${tag}: after a failed first ask and an unchanged PR, Merge ▾ still offers only Squash`, async ({ page }) => {
    await reset(page);
    // The first ask for the repository's methods fails; the PR doesn't move
    // afterwards, so what follows are 304 reads, not "moved" ones.
    repoFails = 1;
    const block = await open(page);
    const el = page.locator(`[data-forge-block="${block}"]`);
    await expect(el.locator("[data-item-head] .item-number")).toHaveText(`#${N}`);
    await expect.poll(() => repoReads).toBeGreaterThan(1);
    await expect.poll(() => pullNotModified).toBeGreaterThan(1);
    await el.locator("[data-item-foot]").getByRole("button", { name: "Merge ▾" }).click();
    await expect(page.locator(".prompt-choices button")).toHaveText(["Squash"]);
    await page.keyboard.press("Escape");
    // After a reload, with the PR still unchanged: the same.
    await page.reload();
    await expect(el.locator("[data-item-head] .item-number")).toHaveText(`#${N}`);
    await el.locator("[data-item-foot]").getByRole("button", { name: "Merge ▾" }).click();
    await expect(page.locator(".prompt-choices button")).toHaveText(["Squash"]);
  });

  test(`${tag}: choosing a method asks for a confirm click; only that merges`, async ({ page }) => {
    await reset(page);
    const block = await open(page);
    const el = page.locator(`[data-forge-block="${block}"]`);
    const foot = el.locator("[data-item-foot]");
    await expect.poll(() => repoReads).toBeGreaterThan(0);
    await foot.getByRole("button", { name: "Merge ▾" }).click();
    await page.locator(".prompt-choices button", { hasText: "Squash" }).click();
    // ONE click: the button asks, the PR is still open and nothing reached the fake.
    const confirm = foot.locator("[data-merge-confirm]");
    await expect(confirm).toHaveText("Confirm squash");
    await expect(confirm).toHaveClass(/danger/);
    await expect(el.locator("[data-item-pill]")).not.toHaveText("Merged");
    expect(merges).toEqual([]);
    await confirm.click();
    await expect.poll(() => merges.length).toBe(1);
    expect(merges[0]).toEqual({ style: "squash", auth: `Bearer ${TOKEN}` });
    await expect(el.locator("[data-item-pill]")).toHaveText("Merged");
  });

  test(`${tag}: Escape, a click elsewhere and the clock all take the confirm back`, async ({ page }) => {
    await reset(page);
    const block = await open(page);
    const el = page.locator(`[data-forge-block="${block}"]`);
    const foot = el.locator("[data-item-foot]");
    await expect.poll(() => repoReads).toBeGreaterThan(0);
    const ask = async () => {
      await foot.getByRole("button", { name: "Merge ▾" }).click();
      await page.locator(".prompt-choices button", { hasText: "Squash" }).click();
      await expect(foot.locator("[data-merge-confirm]")).toBeVisible();
    };
    await ask();
    await page.keyboard.press("Escape");
    await expect(foot.locator("[data-merge-confirm]")).toHaveCount(0);
    await ask();
    await el.locator("[data-item-head] .item-title").click();
    await expect(foot.locator("[data-merge-confirm]")).toHaveCount(0);
    await ask();
    await expect(foot.locator("[data-merge-confirm]")).toHaveCount(0, { timeout: 8000 });
    await expect(foot.getByRole("button", { name: "Merge ▾" })).toBeVisible();
    expect(merges).toEqual([]);
  });
}
