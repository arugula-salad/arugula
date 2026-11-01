// #397 (shared by agent-context.spec.ts and its WebKit twin): an agent block shows its session's model, how full its context
// window is, and a mark when Claude Code compacted the conversation. The
// numbers come from the transcript the daemon reads, so each state here is
// a seeded Claude Code conversation, opened from the pane menu.

import { mkdirSync, rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { expect, test, type Page } from "@playwright/test";
import { menu, paneEl, panes, reset, seedConversation, type Base } from "./helpers";

const claude = process.env.CLAUDE_CONFIG_DIR!;
const seeded: string[] = [];

const used = (base: Base, n: string, model: string, tokens: number) => ({
  ...base("assistant", `u-${n}`, "a2"),
  message: {
    id: `m-${n}`, role: "assistant", model, content: [{ type: "text", text: `reply ${n}` }],
    usage: { input_tokens: 3, cache_read_input_tokens: tokens - 13, cache_creation_input_tokens: 10, output_tokens: 40 },
  },
});

const boundary = (base: Base, n: string, trigger: string, pre: number, post: number) => ({
  ...base("system", `b-${n}`, null), subtype: "compact_boundary", content: "Conversation compacted",
  compactMetadata: { trigger, preTokens: pre, postTokens: post },
});

const tooLong = (base: Base, n: string) => ({
  ...base("assistant", `e-${n}`, "a2"), isApiErrorMessage: true,
  message: { id: `e-${n}`, role: "assistant", model: "<synthetic>", content: [{ type: "text", text: "Prompt is too long" }], usage: { input_tokens: 0, output_tokens: 0 } },
});

const OPUS = "claude-opus-5-5";
/** A model with 200k and no long variant, for the tones. */
const OPUS_45 = "claude-opus-4-5";
const cases: { name: string; more: (b: Base) => object[]; model: string; pct: number | null; tone: string; mark: string | null; fill: string; shows?: string }[] = [
  { name: "normal fill", more: (b) => [used(b, "1", OPUS_45, 60_000)], model: "Opus 4.5", pct: 30, tone: "ok", mark: null, fill: "60k of 200k" },
  { name: "near full", more: (b) => [used(b, "1", OPUS_45, 180_000)], model: "Opus 4.5", pct: 90, tone: "danger", mark: null, fill: "180k of 200k" },
  { name: "warm", more: (b) => [used(b, "1", OPUS_45, 140_000)], model: "Opus 4.5", pct: 70, tone: "warn", mark: null, fill: "140k of 200k" },
  {
    name: "auto-compacted at low fill",
    more: (b) => [used(b, "1", `${OPUS}[1m]`, 250_000), boundary(b, "1", "auto", 250_000, 20_000), used(b, "2", `${OPUS}[1m]`, 30_000)],
    model: "Opus 5.5", pct: 3, tone: "ok", mark: "compacted", fill: "30k of 1M",
  },
  {
    name: "auto-compacted at 90%",
    more: (b) => [used(b, "1", OPUS_45, 180_000), boundary(b, "1", "auto", 180_000, 20_000), used(b, "2", OPUS_45, 25_000)],
    model: "Opus 4.5", pct: 13, tone: "ok", mark: "overflow", fill: "25k of 200k",
  },
  { name: "window exceeded", more: (b) => [used(b, "1", OPUS_45, 150_000), tooLong(b, "1")], model: "Opus 4.5", pct: 75, tone: "warn", mark: "overflow", fill: "150k of 200k" },
  { name: "a 1M model", more: (b) => [used(b, "1", `${OPUS}[1m]`, 500_000)], model: "Opus 5.5", pct: 50, tone: "ok", mark: null, fill: "500k of 1M" },
  // #444: Sonnet 5.5 runs with 1M in Claude Code, though its transcript
  // never says so; a compaction at 190k is just a compaction.
  {
    name: "a 1M Sonnet 5.5 session",
    more: (b) => [used(b, "1", "claude-sonnet-5-5", 40_000), used(b, "2", "claude-sonnet-5-5", 189_000), boundary(b, "1", "auto", 190_000, 13_000), used(b, "3", "claude-sonnet-5-5", 52_000)],
    model: "Sonnet 5.5", pct: 5, tone: "ok", mark: "compacted", fill: "52k of 1M",
  },
  // #444: Sonnet 4.6 has 1M only as its [1m] variant, which the transcript
  // doesn't record: tokens, no share, and no overflow.
  {
    name: "an unknown window",
    more: (b) => [used(b, "1", "claude-sonnet-4-6", 170_000), boundary(b, "1", "auto", 170_000, 13_000), used(b, "2", "claude-sonnet-4-6", 120_000)],
    model: "Sonnet 4.6", pct: null, tone: "unknown", mark: "compacted", fill: "120k", shows: "120k",
  },
  // #444: right after a compaction, what it kept and what every request
  // carries (the first turn's 45k), not the kept 13k alone.
  {
    name: "right after a compaction",
    more: (b) => [used(b, "1", OPUS, 45_000), used(b, "2", OPUS, 330_000), boundary(b, "1", "auto", 330_000, 13_000)],
    model: "Opus 5.5", pct: 6, tone: "ok", mark: "compacted", fill: "About 58k of 1M",
  },
];

async function open(page: Page, title: string) {
  await reset(page);
  const [term] = await panes(page);
  await menu(page, paneEl(page, term), "Claude Code conversations…");
  const dialog = page.getByRole("dialog", { name: "Claude Code conversations" });
  await dialog.locator(".picker-filter").fill(title);
  await dialog.locator("[data-conversation]", { hasText: title }).click();
  await expect(dialog).toBeHidden();
  const block = await page.waitForFunction(() => window.__arugula.client.state!.panes.find((p) => p.type === "agent")?.id ?? null);
  return paneEl(page, (await block.jsonValue()) as number);
}

export function contextStripTests() {
  test.afterAll(() => {
    rmSync(join(claude, "projects"), { recursive: true, force: true });
    for (const cwd of seeded) rmSync(cwd, { recursive: true, force: true });
  });

  for (const c of cases) {
    test(`the context strip, ${c.name}`, async ({ page }) => {
      const title = `Context ${c.name}`;
      seeded.push(seedConversation(claude, c.name.replace(/\W/g, ""), title, c.more).cwd);
      const el = await open(page, title);
      const strip = el.locator(".agent-bar [data-context]");
      await expect(strip.locator(".agent-context-model")).toHaveText(c.model);
      const fill = strip.locator("[data-fill]");
      await expect(fill).toHaveAttribute("data-fill", `${c.pct ?? ""}`);
      await expect(fill).toHaveAttribute("data-tone", c.tone);
      await expect(fill).toHaveAttribute("title", new RegExp(`^${c.fill} tokens`));
      await expect(fill).toContainText(c.shows ?? `${c.pct}%`);
      if (c.pct == null) {
        await expect(fill).toHaveAttribute("title", /The window isn't known/);
        await expect(fill.locator(".agent-context-bar > span")).toHaveCount(0);
      }
      if (c.mark) {
        await expect(strip.locator("[data-mark]")).toHaveAttribute("data-mark", c.mark);
        await expect(strip.locator("[data-mark]")).toHaveText(c.mark === "overflow" ? "Overflow" : "Compacted");
        await expect(strip.locator("[data-mark]")).toHaveAttribute("title", /before|Window exceeded/);
      } else {
        await expect(strip.locator("[data-mark]")).toHaveCount(0);
      }
    });
  }
}

// #398: the estimate beside the strip, from the same transcripts and the
// subagents' beside them, at Anthropic's API list prices.

const reply = (base: Base, n: string, model: string, usage: Record<string, unknown>) => ({
  ...base("assistant", `c-${n}`, "a2"),
  message: { id: `cm-${n}`, role: "assistant", model, content: [{ type: "text", text: `reply ${n}` }], usage },
});

const costCases: { name: string; more: (b: Base) => object[]; label: string; usd: string; subagent?: boolean }[] = [
  // 100k in at $4, 50k out at $20.
  { name: "a plain session", more: (b) => [reply(b, "1", OPUS, { input_tokens: 100_000, output_tokens: 50_000 })], label: "≈ $1.40", usd: "1.4" },
  {
    // 2M cache reads at $0.10, 500k five-minute writes at $2.50, 10k out at $10.
    name: "a cache-heavy session",
    more: (b) => [reply(b, "1", "claude-sonnet-5-5", { input_tokens: 2, cache_read_input_tokens: 2_000_000, cache_creation_input_tokens: 500_000, output_tokens: 10_000 })],
    label: "≈ $1.55",
    usd: "1.55",
  },
  {
    // A 1M window costs the same per token: 800k reads at $0.20, 20k out at $20.
    name: "a 1M session",
    more: (b) => [reply(b, "1", `${OPUS}[1m]`, { input_tokens: 0, cache_read_input_tokens: 800_000, output_tokens: 20_000 })],
    label: "≈ $0.56",
    usd: "0.56",
  },
  { name: "an unknown model", more: (b) => [reply(b, "1", "claude-next-9", { input_tokens: 1000, output_tokens: 500 })], label: "2k tokens", usd: "" },
  // 100k in at $4, and a subagent's 100k out at Haiku 4.5's $5.
  { name: "a session with a subagent", more: (b) => [reply(b, "1", OPUS, { input_tokens: 100_000, output_tokens: 0 })], label: "≈ $0.90", usd: "0.9", subagent: true },
];

/** A subagent's transcript beside the session's, as Claude Code writes it. */
function seedSubagent(cwd: string, id: string) {
  const dir = join(claude, "projects", cwd.replace(/[/.]/g, "-"), id, "subagents");
  mkdirSync(dir, { recursive: true });
  const line = {
    type: "assistant", uuid: "s1", parentUuid: null, isSidechain: true, agentId: "e2e1", sessionId: id, timestamp: new Date().toISOString(),
    message: { id: "sm-1", role: "assistant", model: "claude-haiku-4-5-20251001", content: [{ type: "text", text: "found" }], usage: { input_tokens: 0, output_tokens: 100_000 } },
  };
  writeFileSync(join(dir, "agent-e2e1.jsonl"), JSON.stringify(line) + "\n");
  writeFileSync(join(dir, "agent-e2e1.meta.json"), JSON.stringify({ agentType: "Explore", description: "Find the price table" }));
}

export function costTests() {
  test.afterAll(() => {
    rmSync(join(claude, "projects"), { recursive: true, force: true });
    for (const cwd of seeded) rmSync(cwd, { recursive: true, force: true });
  });

  for (const c of costCases) {
    test(`the cost estimate, ${c.name}`, async ({ page }) => {
      const title = `Cost ${c.name}`;
      const s = seedConversation(claude, c.name.replace(/\W/g, ""), title, c.more);
      seeded.push(s.cwd);
      if (c.subagent) seedSubagent(s.cwd, s.id);
      const el = await open(page, title);
      const est = el.locator(".agent-bar [data-estimate]");
      await expect(est).toHaveAttribute("data-estimate", c.usd);
      const btn = est.locator("button");
      await expect(btn).toContainText(c.label);
      await expect(btn).toContainText(c.usd ? "at API prices" : "no price");
      await expect(btn).toHaveAttribute("title", /token counts at Anthropic's API list prices\. Not a bill/);
      await btn.click();
      const pop = el.locator("[data-estimate-pop]");
      await expect(pop).toContainText("Not a bill");
      await expect(pop.locator('[data-estimate-row="session"]')).toContainText(c.subagent ? "≈ $0.40" : c.label);
      if (c.subagent) {
        await expect(pop.locator('[data-estimate-row="e2e1"]')).toContainText("Find the price table");
        await expect(pop.locator('[data-estimate-row="e2e1"]')).toContainText("Explore");
        await expect(pop.locator('[data-estimate-row="e2e1"]')).toContainText("≈ $0.50");
        await expect(pop.locator('[data-estimate-row="total"]')).toContainText("≈ $0.90");
      } else {
        await expect(pop.locator('[data-estimate-row="total"]')).toHaveCount(0);
      }
      if (!c.usd) await expect(pop).toContainText("No list price for claude-next-9");
      await page.keyboard.press("Escape");
      await expect(pop).toHaveCount(0);
    });
  }

  test("the conversations list shows a conversation's estimate once it's read", async ({ page }) => {
    const title = "Cost in the list";
    const s = seedConversation(claude, "costlist", title, (b) => [reply(b, "1", OPUS, { input_tokens: 100_000, output_tokens: 0 })]);
    seeded.push(s.cwd);
    seedSubagent(s.cwd, s.id);
    await reset(page);
    const [term] = await panes(page);
    const dialog = page.getByRole("dialog", { name: "Claude Code conversations" });
    // The daemon prices it off the listing's path: a later listing has it.
    await expect(async () => {
      if (await dialog.isVisible()) await dialog.getByRole("button", { name: "Cancel" }).click();
      await menu(page, paneEl(page, term), "Claude Code conversations…");
      await dialog.locator(".picker-filter").fill(title);
      await expect(dialog.locator("[data-conversation] .conv-cost")).toHaveText("≈ $0.90", { timeout: 1000 });
    }).toPass({ timeout: 20_000 });
    await expect(dialog.locator("[data-conversation] .conv-cost")).toHaveAttribute("title", /API list prices\. Not a bill.* With its subagents/);
  });
}
