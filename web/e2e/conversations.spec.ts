// M33 in the client: Claude Code conversations from a terminal or the
// desktop app, picked from a pane's menu (desktop) or the sheet (phone).
// Continue here resumes one in a terminal tab named after it (`claude
// --resume`, a stand-in here: the tab and its pane are what's checked; the
// daemon's test checks the command and folder), and offers home when its
// folder is gone; a Codex one too (`codex resume`, a stand-in as well). Open as agent block shows it as a block, continued
// there; one still open elsewhere forked instead, or moved here: a stand-in
// Claude Code process stopped (SIGINT) and the conversation continued. The Claude directory is
// the run's own (playwright.config.ts) and Claude Code's adapter is the
// fake ACP agent, so nothing here costs anything.

import { execFileSync, spawn } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { devices, expect, test, type Page } from "@playwright/test";
import { active, menu, paneEl, panes, reset, seedConversation, tab, type Base } from "./helpers";

const claude = process.env.CLAUDE_CONFIG_DIR!;
const codexHome = process.env.CODEX_HOME!;

const seeded: string[] = [];
const seed = (word: string, title: string, more?: (base: Base) => object[]) => {
  const made = seedConversation(claude, word, title, more);
  seeded.push(made.cwd);
  return made;
};

// The run's Claude directory is every spec's: what these leave in it is a
// conversation each later daemon lists (and a hint it offers, aru-127).
test.afterAll(() => {
  rmSync(join(claude, "projects"), { recursive: true, force: true });
  rmSync(join(codexHome, "sessions"), { recursive: true, force: true });
  for (const cwd of seeded) rmSync(cwd, { recursive: true, force: true });
});

/** A Codex session in the run's Codex home, as Codex 0.163 writes its
 *  rollout: its meta line, then what was typed, as events. */
function seedCodex(word: string): { id: string; cwd: string } {
  const cwd = mkdtempSync(join(tmpdir(), `arugula-e2e-codex-${word}-`));
  seeded.push(cwd);
  const id = crypto.randomUUID();
  const now = new Date();
  const day = join(codexHome, "sessions", `${now.getUTCFullYear()}`, `${now.getUTCMonth() + 1}`.padStart(2, "0"), `${now.getUTCDate()}`.padStart(2, "0"));
  mkdirSync(day, { recursive: true });
  const at = now.toISOString();
  const typed = (text: string) => ({ timestamp: at, type: "event_msg", payload: { type: "item_completed", item: { type: "UserMessage", content: [{ type: "text", text }] } } });
  const lines = [
    { timestamp: at, type: "session_meta", payload: { session_id: id, id, timestamp: at, cwd, originator: "codex-tui", cli_version: "0.163.0", source: "cli", git: { branch: "main" } } },
    { timestamp: at, type: "turn_context", payload: { cwd, model: "gpt-6.1-sol" } },
    typed(`count the ${word}s`),
    typed("and again"),
  ];
  const stamp = at.slice(0, 19).replace(/:/g, "-");
  writeFileSync(join(day, `rollout-${stamp}-${id}.jsonl`), lines.map((l) => JSON.stringify(l)).join("\n") + "\n");
  return { id, cwd };
}

/** A process holds the session, as a running Claude Code would: this
 *  test's own, or a stand-in's. */
function hold(id: string, pid = process.pid) {
  // Its start time as Claude Code writes it: field 22 of /proc/<pid>/stat
  // on Linux, `ps -o lstart=` in the C locale and UTC elsewhere.
  const proc = `/proc/${pid}/stat`;
  const stat = existsSync(proc) ? readFileSync(proc, "utf8") : null;
  const procStart = stat
    ? stat.slice(stat.lastIndexOf(")") + 1).trim().split(/\s+/)[19]
    : execFileSync("ps", ["-o", "lstart=", "-p", `${pid}`], { env: { ...process.env, LC_ALL: "C", TZ: "UTC" }, encoding: "utf8" }).trim();
  writeFileSync(
    join(claude, "sessions", `${pid}.json`),
    JSON.stringify({ pid, sessionId: id, procStart, kind: "interactive", entrypoint: "cli", status: "idle" }),
  );
}

const agentBlock = (page: Page) =>
  page.evaluate(() => window.__arugula.client.state!.panes.find((p) => p.type === "agent")?.id ?? null);

const picker = (page: Page) => page.getByRole("dialog", { name: "Claude Code conversations" });

test.describe("desktop", () => {
  test("open a conversation as an agent block from the pane menu, read it, continue it", async ({ page }) => {
    const { id } = seed("kestrel", "Bird watching");
    await reset(page);
    const [term] = await panes(page);
    await menu(page, paneEl(page, term), "Claude Code conversations…");
    const dialog = picker(page);
    await expect(dialog).toBeVisible();
    const row = dialog.locator(`[data-conversation="${id}"]`);
    await expect(row).toContainText("Bird watching");
    await expect(row.locator(".conv-source")).toHaveText("Terminal");
    // Search narrows it.
    await dialog.locator(".picker-filter").fill("no such words at all");
    await expect(row).toBeHidden();
    await dialog.locator(".picker-filter").fill("bird");
    await expect(dialog.locator("[data-continue-here]")).toHaveText("Continue here");
    await dialog.locator("[data-open-block]").click();
    await expect(dialog).toBeHidden();

    // A stopped block beside the pane: its transcript, nothing running.
    await expect.poll(() => agentBlock(page)).not.toBeNull();
    const block = (await agentBlock(page))!;
    expect(await panes(page)).toEqual([term, block]);
    const el = paneEl(page, block);
    await expect(el.locator(".agent-status")).toHaveText("Conversation");
    await expect(el.locator(".agent-import")).toContainText("from a terminal");
    await expect(el.locator(".agent-user").first()).toHaveText("remember kestrel");
    await expect(el.locator(".agent-tool .agent-tool-title")).toHaveText("echo kestrel");
    await expect(el.locator(".agent-tool .agent-tool-status")).toHaveText("completed");
    await expect(el.locator(".agent-composer textarea")).toHaveAttribute("placeholder", "Continue the conversation…");

    // Sending continues it, with its context.
    await el.locator(".agent-composer textarea").fill("recall");
    await el.locator(".agent-composer textarea").press("Enter");
    await expect(el.locator(".agent-msg").last()).toHaveText("You said kestrel.");
    await expect(el.locator(".agent-status")).toHaveText("Ready");
    await expect(el.locator(".agent-import")).toBeHidden();
    await expect(el.locator(".agent-note", { hasText: "Continued in Arugula" })).toBeVisible();

    // Picking it again goes to the block.
    await menu(page, paneEl(page, term), "Claude Code conversations…");
    await expect(picker(page).locator(`[data-conversation="${id}"] .chip-tag`)).toHaveText(`%${block}`);
    await picker(page).locator(`[data-conversation="${id}"]`).click();
    await expect(picker(page)).toBeHidden();
    expect(await panes(page)).toEqual([term, block]);
  });

  test("what Continue won't remember is folded away (#79)", async ({ page }) => {
    // A rewind: "red" was sent, then "blue" from the same point instead.
    const reply = (base: Base, uuid: string, parent: string, m: string) => ({
      ...base("assistant", uuid, parent),
      message: { id: m, role: "assistant", model: "claude-haiku-4-5-20251001", content: [{ type: "text", text: "OK." }] },
    });
    seed("wren", "Colours", (base) => [
      { ...base("user", "u3", "u2"), message: { role: "user", content: "the colour is red" } },
      reply(base, "a3", "u3", "m3"),
      { type: "last-prompt", lastPrompt: "the colour is red", leafUuid: "a3" },
      { ...base("user", "u4", "u2"), message: { role: "user", content: "the colour is blue" } },
      reply(base, "a4", "u4", "m4"),
      { type: "last-prompt", lastPrompt: "the colour is blue", leafUuid: "a4" },
    ]);
    await reset(page);
    const [term] = await panes(page);
    await menu(page, paneEl(page, term), "Claude Code conversations…");
    await picker(page).locator(".picker-filter").fill("colours");
    await picker(page).locator("[data-open-block]").click();
    await expect.poll(() => agentBlock(page)).not.toBeNull();
    const el = paneEl(page, (await agentBlock(page))!);
    const fold = el.locator(".agent-forgotten");
    await expect(fold).toHaveCount(1);
    await expect(fold.locator("summary")).toHaveText("Not in what it remembers: Continue goes on from another branch (2 entries)");
    await expect(el.locator(".agent-user", { hasText: "red" })).toBeHidden();
    await expect(el.locator(".agent-user", { hasText: "blue" })).toBeVisible();
    await fold.locator("summary").click();
    await expect(fold.locator(".agent-user")).toHaveText("the colour is red");
  });

  test("one open elsewhere isn't continued as it is: moved here, or forked", async ({ page }) => {
    const { id } = seed("plover", "Shore birds");
    // This test's own process holds it: never moved, only asked about.
    hold(id);
    await reset(page);
    const [term] = await panes(page);
    await menu(page, paneEl(page, term), "Claude Code conversations…");
    const row = picker(page).locator(`[data-conversation="${id}"]`);
    await expect(row.locator(".conv-live")).toBeVisible();
    await picker(page).locator(".picker-filter").fill("shore");
    // Not as it is: two Claude Codes on one conversation write over each
    // other. Moving it asks first; clicking its row doesn't move it.
    await row.click();
    await expect(picker(page).locator("[data-continue-here]")).toHaveCount(0);
    await expect(picker(page).locator("[data-move-ask]")).toHaveText(/^Running in .+\. Move here\?$/);
    await expect(picker(page).locator("[data-move-here]")).toHaveText("Move here");
    await expect(picker(page).locator("[data-fork-here]")).toHaveText("Fork here");
    await picker(page).locator("[data-open-block]").click();
    await expect.poll(() => agentBlock(page)).not.toBeNull();
    const el = paneEl(page, (await agentBlock(page))!);
    await expect(el.locator(".agent-status")).toHaveText("Open elsewhere");
    await expect(el.locator("[data-continue]")).toBeDisabled();
    await expect(el.locator(".agent-import")).toContainText("Fork it to go on here");

    await el.locator("[data-fork]").click();
    await expect(el.locator(".agent-note", { hasText: "Forked into a new session" })).toBeVisible();
    await expect(el.locator(".agent-status")).toHaveText("Ready");
    await el.locator(".agent-composer textarea").fill("recall");
    await el.locator(".agent-composer textarea").press("Enter");
    await expect(el.locator(".agent-msg").last()).toHaveText("You said plover.");
  });
});

test.describe("continue here", () => {
  test("resumes it in a terminal tab named after it, no agent block", async ({ page }) => {
    seed("osprey", "Fish hawks");
    await reset(page);
    const [term] = await panes(page);
    await menu(page, paneEl(page, term), "Claude Code conversations…");
    await picker(page).locator(".picker-filter").fill("fish hawks");
    await picker(page).locator("[data-continue-here]").click();
    await expect(picker(page)).toBeHidden();
    await expect.poll(async () => (await tab(page))?.name).toBe("Fish hawks");
    const pane = await active(page);
    expect(pane).not.toBe(term);
    // A terminal (its `kind` is what runs in it: the agent, `claude`).
    expect(await page.evaluate((p) => window.__arugula.client.info(p)?.type, pane)).toBe("terminal");
    expect(await agentBlock(page)).toBeNull();
  });

  test("its folder gone, it says so and runs it in home", async ({ page }) => {
    const { cwd } = seed("egret", "Gone birds");
    rmSync(cwd, { recursive: true, force: true });
    await reset(page);
    const [term] = await panes(page);
    await menu(page, paneEl(page, term), "Claude Code conversations…");
    const dialog = picker(page);
    // A gone folder's conversation is listed with All.
    await dialog.getByRole("switch", { name: "All" }).check();
    await dialog.locator(".picker-filter").fill("gone birds");
    await dialog.locator("[data-continue-here]").click();
    await expect(dialog.locator("[data-folder-gone]")).toContainText(`${cwd} is gone`);
    expect(await panes(page)).toEqual([term]);
    await dialog.locator("[data-continue-home]").click();
    await expect(dialog).toBeHidden();
    await expect.poll(async () => (await tab(page))?.name).toBe("Gone birds");
    expect(await agentBlock(page)).toBeNull();
  });
});

test.describe("move here", () => {
  test("stops it in the other terminal and continues it here", async ({ page }) => {
    const { id } = seed("curlew", "Moor birds");
    // A stand-in Claude Code in another terminal: SIGINT ends it.
    const other = spawn("sleep", ["300"], { stdio: "ignore" });
    const ended = new Promise<NodeJS.Signals | null>((resolve) => other.on("exit", (_, signal) => resolve(signal)));
    try {
      hold(id, other.pid!);
      // A narrow window: the question wraps rather than being cut off.
      await page.setViewportSize({ width: 360, height: 720 });
      await reset(page);
      const [term] = await panes(page);
      await menu(page, paneEl(page, term), "Claude Code conversations…");
      const dialog = picker(page);
      await dialog.locator(".picker-filter").fill("moor birds");
      const ask = dialog.locator("[data-move-ask]");
      await expect(ask).toHaveText(/^Running in .+\. Move here\?$/);
      expect(await ask.evaluate((el) => el.scrollWidth <= el.clientWidth)).toBe(true);
      const box = (await ask.boundingBox())!;
      const frame = (await dialog.boundingBox())!;
      expect(box.x + box.width).toBeLessThanOrEqual(frame.x + frame.width);

      const continued = page.waitForResponse((r) => r.url().includes("/api/conversations/continue"));
      await dialog.locator("[data-move-here]").click();
      expect((await continued).ok()).toBe(true);
      expect(await ended).toBe("SIGINT");
      await expect(dialog).toBeHidden();
      await expect.poll(async () => (await tab(page))?.name).toBe("Moor birds");
      const pane = await active(page);
      expect(pane).not.toBe(term);
      expect(await page.evaluate((p) => window.__arugula.client.info(p)?.type, pane)).toBe("terminal");
    } finally {
      other.kill("SIGKILL");
      rmSync(join(claude, "sessions", `${other.pid}.json`), { force: true });
    }
  });
});

test.describe("codex", () => {
  test("a Codex session is listed by its folder and continues in a terminal tab", async ({ page }) => {
    const { id, cwd } = seedCodex("heron");
    await reset(page);
    const [term] = await panes(page);
    await menu(page, paneEl(page, term), "Claude Code conversations…");
    const dialog = picker(page);
    const row = dialog.locator(`[data-conversation="${id}"]`);
    await expect(row.locator(".conv-source")).toHaveText("Codex");
    await expect(row.locator(".conv-title")).toHaveText("count the herons");
    await expect(dialog.locator(".conv-folder", { hasText: cwd.split("/").pop()! })).toBeVisible();

    // One way on: Continue here, as Claude Code's, with no agent block
    // and no Fork.
    await dialog.locator(".picker-filter").fill("herons");
    const go = dialog.locator("[data-continue-here]");
    await expect(go).toHaveText("Continue here");
    await expect(dialog.locator("[data-open-block]")).toHaveCount(0);
    await expect(dialog.locator(".picker-actions button", { hasText: "Fork" })).toHaveCount(0);
    const continued = page.waitForResponse((r) => r.url().includes("/api/conversations/continue"));
    await go.click();
    const res = await continued;
    expect(res.ok()).toBe(true);
    expect(await res.json()).toMatchObject({ opened: true, conversation: id, cwd });
    await expect(dialog).toBeHidden();
    await expect.poll(async () => (await tab(page))?.name).toBe("count the herons");
    const pane = await active(page);
    expect(await page.evaluate((p) => window.__arugula.client.info(p)?.type, pane)).toBe("terminal");
    expect(await agentBlock(page)).toBeNull();
  });
});

test.describe("phone", () => {
  const { viewport, userAgent, deviceScaleFactor, isMobile, hasTouch } = devices["Pixel 7"];
  test.use({ viewport, userAgent, deviceScaleFactor, isMobile, hasTouch });

  test("the sheet lists conversations and continues one here", async ({ page }) => {
    const { id } = seed("heron", "Wading birds");
    await reset(page);
    await page.locator(".sheet-button").click();
    await page.locator("[data-conversations]").click();
    const dialog = picker(page);
    await expect(dialog).toHaveClass(/\bphone\b/);
    await dialog.locator(`[data-conversation="${id}"]`).click();
    await expect(dialog).toBeHidden();
    await expect.poll(async () => (await tab(page))?.name).toBe("Wading birds");
    expect(await agentBlock(page)).toBeNull();
  });
});
