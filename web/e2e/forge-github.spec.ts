// M38: a GitHub pull request as a block, against a fake GitHub served here
// (S23's recorded cli/cli#13788: three builds red, now by jhgaylor) and the
// stand-in `gh` the config puts on the daemon's PATH.
//
// The block shows the check runs and that they failed, with *Rerun checks*
// (GitHub can; Forgejo can't); clicking it posts `rerun-failed-jobs` for
// the red workflow run with gh's token, and the jobs queue again. Polls
// are conditional: the fake sees `If-None-Match` and answers 304. Nothing
// here reaches GitHub.

import { readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { createHash } from "node:crypto";
import { createServer, type Server, type ServerResponse } from "node:http";
import { expect, test } from "@playwright/test";
import { example } from "./doc-examples";
import { reset } from "./helpers";
import { listen } from "./ports";

const TOKEN = "e2e-github-token";
const REPO = "cli/cli";
const N = 13788;
const fixture = (f: string) =>
  JSON.parse(readFileSync(new URL(`../../crates/daemon/tests/fixtures/github/github-cli-cli-13788/${f}`, import.meta.url), "utf8"));

let origin = "";
let server: Server;
let item: Record<string, any>;
let runs: Record<string, any>[] = [];
const writes: { route: string; auth: string }[] = [];
let notModified = 0;

function answer(res: ServerResponse, inm: string | undefined, v: unknown) {
  const body = JSON.stringify(v);
  const etag = `"${createHash("sha1").update(body).digest("hex")}"`;
  const h = { etag, "x-ratelimit-limit": "5000", "x-ratelimit-remaining": "4900", "x-ratelimit-resource": "core" };
  if (inm === etag) {
    notModified += 1;
    return res.writeHead(304, h).end();
  }
  res.writeHead(200, { "content-type": "application/json", ...h }).end(body);
}

test.beforeAll(async () => {
  item = fixture("item.json");
  item.user.login = "jhgaylor";
  item.requested_reviewers = [];
  runs = fixture("check_runs.json").check_runs;
  server = createServer((req, res) => {
    const u = new URL(req.url!, "http://github");
    if (req.headers.authorization !== `Bearer ${TOKEN}`) return res.writeHead(401).end('{"message":"Bad credentials"}');
    const inm = req.headers["if-none-match"] as string | undefined;
    const p = u.pathname.replace(`/repos/${REPO}`, "");
    req.resume();
    req.on("end", () => {
      if (req.method === "POST") {
        writes.push({ route: p, auth: req.headers.authorization! });
        for (const c of runs) if (c.conclusion === "failure") Object.assign(c, { status: "queued", conclusion: null });
        return res.writeHead(201).end();
      }
      if (u.pathname === "/user") return answer(res, inm, { login: "jhgaylor" });
      if (u.pathname === "/user/teams") return answer(res, inm, []);
      // Squash only, as GitHub tells a login that can push (UI audit #196).
      if (p === "") return answer(res, inm, { full_name: REPO, allow_merge_commit: false, allow_squash_merge: true, allow_rebase_merge: false });
      if (p === `/pulls/${N}`) return answer(res, inm, item);
      if (p.endsWith("/check-runs")) return answer(res, inm, { total_count: runs.length, check_runs: runs });
      if (p.endsWith("/status")) return answer(res, inm, { state: "pending", total_count: 0, statuses: [] });
      if (p === `/pulls/${N}/reviews`) return answer(res, inm, fixture("reviews.json"));
      if (p === `/issues/${N}/timeline`) return answer(res, inm, fixture("timeline.json"));
      res.writeHead(404).end('{"message":"Not Found"}');
    });
  });
  origin = `http://127.0.0.1:${await listen(server)}`;
  // The stand-in gh has a login for it (and no other host).
  writeFileSync(join(process.env.ARUGULA_E2E_TEA_DIR!, "gh-hosts"), `${origin.replace("http://", "")}\n`);
});

test.afterAll(() => {
  server.close();
  writeFileSync(join(process.env.ARUGULA_E2E_TEA_DIR!, "gh-hosts"), "");
});

test("red check runs on your GitHub PR rerun from the block", async ({ page }) => {
  await reset(page);
  const block = await page.evaluate(
    ([api, repo, number]) =>
      window.__arugula.client
        .request("POST", "/api/blocks", { type: "forge", config: { provider: "github", api, repo, number }, local: true })
        .then((r) => r.json<{ block: number }>())
        .then((r) => r.block),
    [origin, REPO, N] as const,
  );
  const el = page.locator(`[data-forge-block="${block}"]`);
  await expect(el.locator("[data-item-head] .item-number")).toHaveText(`#${N}`);
  await expect(el.locator("[data-item-head] .item-title")).toHaveText("fix: preserve percent-encoded path in DisplayURL");
  await expect(el.locator("[data-check]")).toHaveCount(11);
  // #408: one red strip, with its buttons; the sentences are tooltips and icons.
  const strip = el.locator("[data-item-next]");
  await expect(strip).toHaveAttribute("data-item-next", "danger");
  await expect(strip).toContainText("build (ubuntu-latest) and 2 more failed");
  await expect(el.locator('[data-item-tab="checks"]')).toHaveAttribute("data-tone", "danger");
  await expect(el.locator('[data-item-fact="checks"]')).toContainText("✕ 3");
  for (const gone of ["Waiting on you", "wants to merge", "checks failed", "polls", "no reviews"]) await expect(el).not.toContainText(gone);
  // Who you act as, and how it's read, are the source icon's tooltip.
  // M172: the reader's, not a pin (forge-pin.spec.ts): it says how it polls.
  const source = el.locator("[data-item-source]");
  await expect(source).toHaveAttribute("title", /as jhgaylor \(gh on /);
  await expect(source).toHaveAttribute("title", /polls/);
  await expect(source).not.toHaveAttribute("title", /Arugula control/);
  // On the rail too, with a rerun.
  await expect
    .poll(() => page.evaluate((b) => window.__arugula.client.info(b)?.reason?.actions ?? [], block))
    .toEqual(["rerun", "dismiss"]);
  // Polls are conditional: unchanged, they're 304s.
  await expect.poll(() => notModified).toBeGreaterThan(3);

  await el.locator("[data-rerun]").click();
  await expect.poll(() => writes.length).toBe(1);
  expect(writes[0]).toEqual({ route: "/actions/runs/28656994029/rerun-failed-jobs", auth: `Bearer ${TOKEN}` });
  // The jobs queue again: no longer failed.
  await expect(el.locator("[data-item-next]")).toHaveCount(0);
  await expect(el.locator('[data-check="queued"]')).toHaveCount(3);

  // #379: running and queued check dots use the warn colour, not accent.
  const colors = await el.locator(".forge-dot.queued").first().evaluate(() => {
    const dot = document.querySelector(".forge-dot.queued") as HTMLElement;
    const dotBg = getComputedStyle(dot).backgroundColor;
    const warnEl = document.createElement("div");
    warnEl.style.color = "var(--warn)";
    document.body.appendChild(warnEl);
    const warnColor = getComputedStyle(warnEl).color;
    document.body.removeChild(warnEl);
    const accentEl = document.createElement("div");
    accentEl.style.color = "var(--accent)";
    document.body.appendChild(accentEl);
    const accentColor = getComputedStyle(accentEl).color;
    document.body.removeChild(accentEl);
    return { dotBg, warnColor, accentColor };
  });
  expect(colors.dotBg).toBe(colors.warnColor);
  expect(colors.dotBg).not.toBe(colors.accentColor);
});

// #964: a Doc draws a chart fence and shows an svg one; a forge's body is
// written by anyone who can open a PR, so there they stay code.
test("a chart or svg fence in a PR's description is code, not a chart or a picture", async ({ page }) => {
  const was = item.body;
  item.body = `${example("bar-chart.md")}\n${example("drawing.md")}`;
  try {
    await reset(page);
    const block = await page.evaluate(
      ([api, repo, number]) =>
        window.__arugula.client
          .request("POST", "/api/blocks", { type: "forge", config: { provider: "github", api, repo, number }, local: true })
          .then((r) => r.json<{ block: number }>())
          .then((r) => r.block),
      [origin, REPO, N] as const,
    );
    const body = page.locator(`[data-forge-block="${block}"] .forge-desc`);
    await expect(body.locator("pre")).toHaveCount(2);
    await expect(body.locator("pre").first()).toContainText('"type": "bar"');
    await expect(body.locator("pre").nth(1)).toContainText('<svg xmlns="http://www.w3.org/2000/svg"');
    await expect(body.locator("svg, img, .chart-box")).toHaveCount(0);
  } finally {
    item.body = was;
  }
});

// UI audit #196, kept through #408: Merge ▾ offers only the methods the
// repository allows, and a squash-only repository offers Squash alone.
test("a squash-only repository's Merge ▾ offers only Squash", async ({ page }) => {
  await reset(page);
  const block = await page.evaluate(
    ([api, repo, number]) =>
      window.__arugula.client
        .request("POST", "/api/blocks", { type: "forge", config: { provider: "github", api, repo, number }, local: true })
        .then((r) => r.json<{ block: number }>())
        .then((r) => r.block),
    [origin, REPO, N] as const,
  );
  const el = page.locator(`[data-forge-block="${block}"]`);
  await el.locator("[data-item-foot]").getByRole("button", { name: "Merge ▾" }).click();
  const choices = page.locator(".prompt-choices button");
  await expect(choices).toHaveText(["Squash"]);
});
