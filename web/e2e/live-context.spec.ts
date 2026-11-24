// #450: what Claude Code says of its session from inside (the arugula-live
// mod, through `arugula session-signal`) shows as a slim strip in a
// terminal's header while Claude Code runs there, and wins over the
// transcript on an agent block, with Claude Code's own cost. Where the
// figures come from is in the strip's popover. The signals are posted here
// as the mod's CLI call would. Claude Code's own cost wins over the estimate
// whenever it reports one: the mod's, or claude-agent-acp's.

import { mkdtempSync, readFileSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { expect, test, type Page } from "@playwright/test";
import { menu, menuItem, paneEl, panes, reset, seedConversation, SLEEP } from "./helpers";

const VERSIONS = { claude_version: "2.1.316", mod_version: "0.2.0" };

/** Conversation `id` from Your work (its ⋯) as an agent block, beside `term`. */
async function openAsBlock(page: Page, term: number, id: string) {
  await menu(page, paneEl(page, term), "Your work…");
  const row = page.getByRole("dialog", { name: "Your work" }).locator(`[data-conversation="${id}"]`);
  await row.click();
  await row.locator("[data-conv-more]").click();
  await (await menuItem(page, "Open as agent block")).click();
}

async function signal(page: Page, pane: number, body: object) {
  await page.evaluate(([p, b]) => window.__arugula.client.api(`/api/panes/${p}/session-signal`, b), [pane, { ...VERSIONS, ...body }] as const);
}

test("a terminal running Claude Code with the mod gets a slim strip, live", async ({ page }) => {
  // `sleep` under Claude Code's name: the daemon sees Claude Code there.
  const bin = mkdtempSync(join(tmpdir(), "arugula-e2e-live-"));
  symlinkSync(SLEEP, join(bin, "claude"));
  try {
    await reset(page);
    const [pane] = await panes(page);
    await page.evaluate(([p, cmd]) => window.__arugula.client.api(`/api/panes/${p}/send`, { text: cmd, enter: true }), [pane, `${join(bin, "claude")} 120`] as const);
    await expect.poll(() => page.evaluate((p) => window.__arugula.client.info(p)?.kind, pane)).toBe("agent");
    await signal(page, pane, { event: "step", model: "claude-sonnet-5-5", effort: "high" });
    await signal(page, pane, { event: "usage", tokens: 52_000, window: 1_000_000, cost_usd: 1.25 });
    const strip = page.locator(`[data-pane-header="${pane}"] [data-context]`);
    await expect(strip).toHaveAttribute("data-source", "live");
    await expect(strip.locator("[data-fill]")).toHaveAttribute("data-fill", "5");
    await expect(strip.locator("[data-fill]")).toHaveAttribute("title", /^52k of 1M tokens\. 1M window, as Claude Code reported it/);
    // Slim: the model is in the popover, not the header.
    await expect(strip.locator(".agent-context-model")).toHaveCount(0);
    // A compaction, as it happens.
    await signal(page, pane, { event: "compact", trigger: "auto", tokens_before: 190_000, tokens_after: 13_000, window: 1_000_000 });
    await expect(strip.locator("[data-mark]")).toHaveAttribute("data-mark", "compacted");
    await strip.locator("button").click();
    const pop = page.locator("[data-context-pop]");
    await expect(pop).toContainText("Sonnet 5.5, high effort");
    await expect(pop).toContainText("$1.25 Claude Code's figure");
    await expect(pop.locator("[data-context-source]")).toHaveText("Live from Claude Code (Claude Code 2.1.316, arugula-live 0.2.0), as it runs.");
    await page.keyboard.press("Escape");
    await expect(pop).toHaveCount(0);
    // Claude Code gone, its strip goes too.
    await page.evaluate((p) => window.__arugula.client.api(`/api/panes/${p}/send`, { text: "\u0003" }), pane);
    await expect(strip).toHaveCount(0, { timeout: 15_000 });
  } finally {
    rmSync(bin, { recursive: true, force: true });
  }
});

test("an agent block's strip: the mod's figures win over its transcript's", async ({ page }) => {
  const claude = process.env.CLAUDE_CONFIG_DIR!;
  const title = "Live over the transcript";
  const s = seedConversation(claude, "liveover", title, (b) => [
    {
      ...b("assistant", "u-1", "a2"),
      message: { id: "m-1", role: "assistant", model: "claude-sonnet-4-6", content: [{ type: "text", text: "reply" }], usage: { input_tokens: 100_000, output_tokens: 1_000 } },
    },
  ]);
  try {
    await reset(page);
    const [term] = await panes(page);
    await openAsBlock(page, term, s.id);
    const block = (await (await page.waitForFunction(() => window.__arugula.client.state!.panes.find((p) => p.type === "agent")?.id ?? null)).jsonValue()) as number;
    const bar = paneEl(page, block).locator(".agent-bar");
    // From the transcript: Sonnet 4.6's window can't be told, and its cost
    // is the estimate at API prices.
    await expect(bar.locator("[data-context]")).toHaveAttribute("data-source", "transcript");
    await expect(bar.locator("[data-fill]")).toHaveAttribute("data-fill", "");
    await expect(bar.locator("[data-estimate] button")).not.toContainText("at API prices");
    await bar.locator("[data-context] button").click();
    await expect(page.locator("[data-context-source]")).toHaveText("Read from the transcript Claude Code writes, after each reply.");
    await page.keyboard.press("Escape");
    // From inside: the engine's window settles it, and its own cost wins.
    await signal(page, block, { event: "usage", model: "claude-sonnet-4-6", tokens: 101_000, window: 200_000, cost_usd: 0.42 });
    await expect(bar.locator("[data-context]")).toHaveAttribute("data-source", "live");
    await expect(bar.locator("[data-fill]")).toHaveAttribute("data-fill", "51");
    const cost = bar.locator("[data-estimate]");
    await expect(cost).toHaveAttribute("data-cost", "claude");
    await expect(cost.locator("button")).toHaveText("$0.42 Claude Code's figure");
    await cost.locator("button").click();
    const pop = page.locator("[data-estimate-pop]");
    await expect(pop).toContainText("Claude Code's own figure for this session");
    // The estimate is still there, below it.
    await expect(pop.locator('[data-estimate-row="session"]')).toBeVisible();
  } finally {
    rmSync(join(claude, "projects"), { recursive: true, force: true });
    rmSync(s.cwd, { recursive: true, force: true });
  }
});

// #453: keyed by Claude Code's session id, with the pane optional. A
// session with no pane (VS Code's Claude Code, the desktop app) keeps its
// figures; one from an app started in a pane (its pid isn't in the pane's
// foreground job) does too, and not on that pane. VS Code's sessions are on
// the default list. Your work's rows draw no strip or cost (Arugula #1396),
// so what's checked is what `conversations.list` carries for them.
test("a session with no pane keeps its live figures in the conversations list", async ({ page }) => {
  const claude = process.env.CLAUDE_CONFIG_DIR!;
  const title = "Live from no pane";
  const away = "Live from VS Code started in a pane";
  const s = seedConversation(claude, "livenopane", title);
  const t = seedConversation(claude, "liveapp", away);
  // As VS Code's Claude Code writes it.
  const tPath = join(claude, "projects", t.cwd.replace(/[/.]/g, "-"), `${t.id}.jsonl`);
  writeFileSync(tPath, readFileSync(tPath, "utf8").replaceAll('"entrypoint":"cli"', '"entrypoint":"claude-vscode"'));
  const post = (body: object) =>
    page.evaluate((b) => window.__arugula.client.api("/api/conversations/session-signal", b), { ...VERSIONS, ...body });
  type Row = { id: string; source: string; context?: { source?: string; tokens: number | null; window: number | null; effort?: string; cost_usd?: number | null; model: string | null; claude_version?: string | null; mod_version?: string | null } };
  const listed = () =>
    page.evaluate(async () => {
      const res = await window.__arugula.client.request("GET", "/api/conversations?limit=1000");
      return ((await res.json()) as { conversations: unknown[] }).conversations;
    }) as Promise<Row[]>;
  try {
    await reset(page);
    const [term] = await panes(page);
    await post({ session_id: s.id, event: "step", model: "claude-opus-5-5", effort: "high" });
    await post({ session_id: s.id, event: "usage", tokens: 300_000, window: 1_000_000, cost_usd: 2.5 });
    // Started from the terminal, running elsewhere: this test's own process
    // isn't in the pane's foreground job.
    await post({ session_id: t.id, event: "usage", tokens: 20_000, window: 200_000, pane: term, pid: process.pid });
    const rows = await listed();
    expect(rows.find((r) => r.id === s.id)?.context).toMatchObject({
      source: "live",
      tokens: 300_000,
      window: 1_000_000,
      effort: "high",
      cost_usd: 2.5,
      model: "Opus 5.5",
      claude_version: "2.1.316",
      mod_version: "0.2.0",
    });
    const app = rows.find((r) => r.id === t.id);
    expect(app?.source).toBe("vscode");
    expect(app?.context).toMatchObject({ source: "live", tokens: 20_000, window: 200_000 });
    // Listed in Your work, as Claude Code's.
    await menu(page, paneEl(page, term), "Your work…");
    const dialog = page.getByRole("dialog", { name: "Your work" });
    await expect(dialog.locator(`[data-conversation="${t.id}"] .yw-tool`)).toHaveText("Claude Code");
    await page.keyboard.press("Escape");
    await expect(dialog).toBeHidden();
    // The terminal it was started from shows none of it.
    await expect(page.locator(`[data-pane-header="${term}"] [data-context]`)).toHaveCount(0);
  } finally {
    rmSync(join(claude, "projects"), { recursive: true, force: true });
    rmSync(s.cwd, { recursive: true, force: true });
    rmSync(t.cwd, { recursive: true, force: true });
  }
});

test("claude-agent-acp's own cost wins over the estimate", async ({ page }) => {
  // The scripted fake, named as claude-agent-acp, writes a transcript the
  // daemon prices (the estimate) and reports $0.37 for the turn, as the
  // adapter passes on the SDK's figure.
  const fake = fileURLToPath(new URL("../../crates/daemon/tests/fake_acp.py", import.meta.url));
  const dir = mkdtempSync(join(tmpdir(), "arugula-e2e-acp-cost-"));
  const script = join(dir, "turns.json");
  writeFileSync(script, JSON.stringify({ turns: [[{ title: "Adapter cost" }, { usage: 60_000, model: "claude-sonnet-5-5" }, { say: "Done.", ms: 1 }]], then: [{ say: "Done." }] }));
  try {
    await reset(page);
    const config = {
      agent: "claude",
      command: ["env", "FAKE_ACP_NAME=@agentclientprotocol/claude-agent-acp", "python3", fake, "--demo", script],
      cwd: dir,
      prompt: "go",
    };
    expect(await page.evaluate((c) => window.__arugula.client.api("/api/blocks", { type: "agent", config: c }), config)).toBe(true);
    const block = (await (await page.waitForFunction(() => window.__arugula.client.state!.panes.find((p) => p.type === "agent")?.id ?? null)).jsonValue()) as number;
    const bar = paneEl(page, block).locator(".agent-bar");
    const cost = bar.locator("[data-estimate]");
    await expect(cost).toHaveAttribute("data-cost", "claude", { timeout: 20_000 });
    await expect(cost.locator("button")).toHaveText("$0.37 Claude Code's figure");
    // The estimate from its transcript is there too, below it.
    await expect(cost).not.toHaveAttribute("data-estimate", "");
    await cost.locator("button").click();
    const pop = page.locator("[data-estimate-pop]");
    await expect(pop).toContainText("Claude Code's own figure for this session");
    await expect(pop.locator('[data-estimate-row="session"]')).toBeVisible();
    // The window, as the adapter reported it.
    await expect(bar.locator("[data-fill]")).toHaveAttribute("title", /^60k of 1M tokens\. 1M window, as Claude Code reported it/);
    await expect(bar.locator(".agent-cost")).toHaveCount(0);
  } finally {
    rmSync(dir, { recursive: true, force: true });
    rmSync(join(process.env.CLAUDE_CONFIG_DIR!, "projects", dir.replace(/[/.]/g, "-")), { recursive: true, force: true });
  }
});
