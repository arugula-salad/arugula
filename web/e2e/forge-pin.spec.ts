// M172: a PR or issue block on a repo linked in Arugula control is a pin:
// the daemon reads the platform's mirror, not GitHub, and keeps the item's
// id (`pin`). The daemon's side, and that it makes no GitHub call, is
// crates/daemon/tests/integration/forge_pins.rs. This spec's daemon isn't
// in control, so the page's socket hands the block the state a pinned
// daemon sends: it's drawn as GitHub's is (the same bar, chips, checks,
// reviews and timeline), says it's read through control, and says the text
// stays on GitHub. A block the reader reads is forge-github.spec.ts's.

import { expect, test, type Page } from "@playwright/test";
import { reset } from "./helpers";

const ITEM = "fi_000000000000000000000000000000a1";
const now = Date.now();

/** What the daemon makes of the mirror's rows (crates/arugulad-forge/src/pin.rs). */
function item(kind: "pr" | "issue", number: number, title: string) {
  return {
    kind, number, title, body: "", author: "jake-gh", state: "open", draft: false, labels: ["bug"], assignees: kind === "issue" ? ["jake-gh"] : [],
    url: `https://github.com/acme/app/${kind === "pr" ? "pull" : "issues"}/${number}`,
    base: { repo: null, branch: kind === "pr" ? "main" : "", sha: "" }, head: { repo: null, branch: kind === "pr" ? "fix-parser" : "", sha: kind === "pr" ? "abc123" : "" },
    head_ref: kind === "pr" ? `refs/pull/${number}/head` : "", merge_base: null, mergeable: kind === "pr" ? true : null, blocked: false,
    requested: kind === "pr" ? [{ user: "dan-gh" }] : [], comments: 1, updated_at: now - 120_000, merged_at: null, merged_by: null,
  };
}

const events = [
  { id: "pf-1", at: now - 600_000, actor: "jake-gh", kind: "other", what: "opened it" },
  { id: "pf-2", at: now - 300_000, actor: "dan-gh", kind: "reviewed", what: "approved" },
  { id: "pf-3", at: now - 120_000, actor: "dan-gh", kind: "commented" },
];

const check = (name: string, state: string) => ({ name, source: "check_run", state, allow_failure: false, url: `https://github.com/acme/app/runs/${name}`, description: null, run: null });

/** The block's state as a pinned daemon sends it, on what this daemon sent. */
function pinned(s: Record<string, unknown>, stale?: string) {
  const kind = s.kind === "issue" ? "issue" : "pr";
  const it = item(kind, s.number as number, kind === "pr" ? "Fix the parser" : "The parser drops tabs");
  return {
    ...s, loading: false, error: null, read_only: null, rate: null, logins: [], me: "jake-gh", polls: 4, reads: 4, updated_ms: now - 30_000,
    live: "webhook", live_via: "github-app", wants: [], rerun: null,
    pr: kind === "pr" ? { item: it, reviews: [{ id: "pf-2", author: "dan-gh", state: "approved", commit: null, stale: false, at: now - 300_000, body: null, url: null }], checks: [check("build", "success"), check("lint", "success")], rollup: "success", events } : null,
    issue: kind === "issue" ? { item: it, events, linked: [] } : null,
    pin: { kind, global_id: ITEM, ...(stale ? { stale } : {}) },
  };
}

/** Rewrite the daemon's word on acme/app's blocks into a pin's. */
async function pin(page: Page, stale?: () => string | undefined) {
  await page.routeWebSocket(/\/ws(\?|$)/, (ws) => {
    const server = ws.connectToServer();
    server.onMessage((m) => {
      if (typeof m === "string" && m.includes('"type":"block"') && m.includes('"repo":"acme/app"')) {
        const msg = JSON.parse(m) as { state: Record<string, unknown> };
        msg.state = pinned(msg.state, stale?.());
        return ws.send(JSON.stringify(msg));
      }
      ws.send(m);
    });
  });
}

/** A block on acme/app, read from nowhere: the socket says what it is. */
const open = (page: Page, kind: "pr" | "issue", number: number) =>
  page.evaluate(
    ([kind, number]) =>
      window.__arugula.client
        .request("POST", "/api/blocks", { type: "forge", config: { provider: "github", api: "http://127.0.0.1:9/api/v3", host: "github.example", repo: "acme/app", kind, number }, local: true })
        .then((r) => r.json<{ block: number }>())
        .then((r) => r.block),
    [kind, number] as const,
  );

test("a pinned PR looks as GitHub's does, read through Arugula control", async ({ page }) => {
  await pin(page);
  await reset(page);
  const block = await open(page, "pr", 1);
  const el = page.locator(`[data-forge-block="${block}"]`);
  await expect(el.locator("[data-item-head] .item-number")).toHaveText("#1");
  await expect(el.locator("[data-item-head] .item-title")).toHaveText("Fix the parser");
  await expect(el.locator("[data-item-pill]")).toHaveText("Open");
  await expect(el.locator("[data-item-fact=branch]")).toHaveText("fix-parser → main");
  await expect(el.locator('[data-item-fact="checks"]')).toContainText("✓ 2");
  await expect(el.locator('[data-item-fact="reviewers"] [data-item-avatar]')).toHaveCount(1);
  await expect(el.locator('[data-item-avatar][data-name="dan-gh"]')).toHaveAttribute("title", "dan-gh approved");
  await expect(el.locator('[data-item-fact="mergeable"]')).toHaveAttribute("data-mergeable", "clean");
  // Green, settled and the person's to merge: the one strip.
  await expect(el.locator('[data-item-next="ok"]')).toContainText("Ready to merge");
  await expect(el.locator("[data-check]")).toHaveCount(2);
  await el.locator('[data-item-tab="reviews"]').click();
  await expect(el.locator('[data-review="approved"]')).toContainText("dan-gh");
  await el.locator('[data-item-tab="activity"]').click();
  await expect(el.locator("[data-forge-timeline] .forge-event")).toHaveCount(3);
  await expect(el.locator("[data-forge-timeline]")).toContainText("dan-gh commented");
  // The mirror keeps no description: the part isn't drawn, nor a line saying so.
  await expect(el.locator("[data-item-body]")).toHaveCount(0);
  await expect(el).not.toContainText("No description");
  // Where it's read is the source icon's tooltip, not a line.
  const source = el.locator("[data-item-source]");
  await expect(source).toHaveAttribute("title", /read through Arugula control/);
  await expect(source).toHaveAttribute("title", /live \(GitHub App\)/);
  await expect(source).not.toHaveAttribute("title", /polls/);
  await expect(el).not.toContainText("read through");
  // The person's own buttons stay.
  await expect(el.getByRole("button", { name: "Comment", exact: true })).toBeVisible();
  await expect(el.locator("[data-forge-checkout]")).toBeVisible();
});

test("a pinned issue, and a pin whose platform can't be reached shows what it said last", async ({ page }) => {
  let stale: string | undefined;
  await pin(page, () => stale);
  await reset(page);
  const block = await open(page, "issue", 2);
  const el = page.locator(`[data-forge-block="${block}"]`);
  await expect(el).toHaveAttribute("data-forge-kind", "issue");
  await expect(el.locator(".review-path")).toContainText("The parser drops tabs #2 acme/app");
  await expect(el.locator("[data-forge-pin]")).toBeVisible();
  await expect(el.locator(".forge-meta")).toContainText("assigned to jake-gh");
  await expect(el.locator("[data-forge-pin-body]")).toBeVisible();
  await expect(el.locator("[data-forge-pin-read]")).toContainText("read through Arugula control");

  stale = "control answered 502";
  await page.evaluate((b) => window.__arugula.client.api(`/api/blocks/${b}/call/refresh`, {}, ""), block);
  await expect(el.locator("[data-forge-pin-read]")).toContainText("can't reach it now: as it said last");
  await expect(el.locator("[data-forge-pin-read]")).toHaveAttribute("title", "control answered 502");
  await expect(el.locator(".review-path")).toContainText("The parser drops tabs");
});
