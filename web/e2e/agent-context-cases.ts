// #397 (shared by agent-context.spec.ts and its WebKit twin): an agent block shows its session's model, how full its context
// window is, and a mark when Claude Code compacted the conversation. The
// numbers come from the transcript the daemon reads, so each state here is
// a seeded Claude Code conversation, opened from the pane menu.

import { rmSync } from "node:fs";
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
const cases: { name: string; more: (b: Base) => object[]; pct: number; tone: string; mark: string | null; fill: string }[] = [
  { name: "normal fill", more: (b) => [used(b, "1", OPUS, 60_000)], pct: 30, tone: "ok", mark: null, fill: "60k of 200k" },
  { name: "near full", more: (b) => [used(b, "1", OPUS, 180_000)], pct: 90, tone: "danger", mark: null, fill: "180k of 200k" },
  { name: "warm", more: (b) => [used(b, "1", OPUS, 140_000)], pct: 70, tone: "warn", mark: null, fill: "140k of 200k" },
  {
    name: "auto-compacted at low fill",
    more: (b) => [used(b, "1", `${OPUS}[1m]`, 250_000), boundary(b, "1", "auto", 250_000, 20_000), used(b, "2", `${OPUS}[1m]`, 30_000)],
    pct: 3, tone: "ok", mark: "compacted", fill: "30k of 1M",
  },
  {
    name: "auto-compacted at 90%",
    more: (b) => [used(b, "1", OPUS, 180_000), boundary(b, "1", "auto", 180_000, 20_000), used(b, "2", OPUS, 25_000)],
    pct: 13, tone: "ok", mark: "overflow", fill: "25k of 200k",
  },
  { name: "window exceeded", more: (b) => [used(b, "1", OPUS, 150_000), tooLong(b, "1")], pct: 75, tone: "warn", mark: "overflow", fill: "150k of 200k" },
  { name: "a 1M model", more: (b) => [used(b, "1", `${OPUS}[1m]`, 500_000)], pct: 50, tone: "ok", mark: null, fill: "500k of 1M" },
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
      await expect(strip.locator(".agent-context-model")).toHaveText("Opus 5.5");
      const fill = strip.locator("[data-fill]");
      await expect(fill).toHaveAttribute("data-fill", `${c.pct}`);
      await expect(fill).toHaveAttribute("data-tone", c.tone);
      await expect(fill).toHaveAttribute("title", `${c.fill} tokens`);
      await expect(fill).toContainText(`${c.pct}%`);
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
