// M33 in the client: Claude Code conversations from a terminal or the
// desktop app, in Your work (Arugula #1396) opened from a pane's menu
// (desktop) or the sheet (phone). Continue here resumes one in a terminal
// tab named after it (`claude --resume`, a stand-in here: the tab and its
// pane are what's checked; the daemon's test checks the command and
// folder), and offers home when its folder is gone; a Codex one too
// (`codex resume`, a stand-in as well). `⋯`'s Open as agent block shows it
// as a block, continued there; one still open elsewhere is moved here (a
// stand-in Claude Code process stopped with SIGINT and the conversation
// continued), or forked. The page itself, the owner's home with
// no session open, is your-work.spec.ts. The Claude directory is
// the run's own (playwright.config.ts) and Claude Code's adapter is the
// fake ACP agent, so nothing here costs anything.

import { execFileSync, spawn } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { devices, expect, test, type Page } from "@playwright/test";
import { active, menu, menuItem, paneEl, panes, reset, seedConversation, tab, type Base } from "./helpers";

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

const picker = (page: Page) => page.getByRole("dialog", { name: "Your work" });

/** The row's `⋯`, then `item`. */
async function more(page: Page, id: string, item: string) {
  const row = picker(page).locator(`[data-conversation="${id}"]`);
  await row.click();
  await row.locator("[data-conv-more]").click();
  await (await menuItem(page, item)).click();
}

test.describe("desktop", () => {
  test("open a conversation as an agent block from the pane menu, read it, continue it", async ({ page }) => {
    const { id } = seed("kestrel", "Bird watching");
    await reset(page);
    const [term] = await panes(page);
    await menu(page, paneEl(page, term), "Your work…");
    const dialog = picker(page);
    await expect(dialog).toBeVisible();
    const row = dialog.locator(`[data-conversation="${id}"]`);
    // Its first prompt, in the person's words, and whose it is.
    await expect(row.locator(".yw-first")).toHaveText("remember kestrel");
    await expect(row.locator(".yw-tool")).toHaveText("Claude Code");
    // Search narrows it.
    await dialog.locator(".yw-filter").fill("no such words at all");
    await expect(row).toBeHidden();
    await dialog.locator(".yw-filter").fill("bird");
    await row.click();
    await expect(row.locator("[data-continue-here]")).toHaveText("Continue here");
    await more(page, id, "Open as agent block");
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
    await menu(page, paneEl(page, term), "Your work…");
    const again = picker(page).locator(`[data-conversation="${id}"]`);
    await expect(again.locator("[data-live]")).toHaveText("● in a pane here");
    await again.click();
    await again.locator("[data-go-to-pane]").click();
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
    await menu(page, paneEl(page, term), "Your work…");
    await picker(page).locator(".yw-filter").fill("colours");
    const id = await picker(page).locator("[data-conversation]").first().getAttribute("data-conversation");
    await more(page, id!, "Open as agent block");
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
    await menu(page, paneEl(page, term), "Your work…");
    const row = picker(page).locator(`[data-conversation="${id}"]`);
    await expect(row.locator("[data-live]")).toHaveText(/^● running in .+$/);
    // Not as it is: two Claude Codes on one conversation write over each
    // other. Moving it asks first; clicking its row doesn't move it.
    await row.click();
    await expect(row.locator("[data-continue-here]")).toHaveCount(0);
    await expect(row.locator("[data-move-ask]")).toHaveText(/^Running in .+\. Move here\?$/);
    await expect(row.locator("[data-move-here]")).toHaveText("Move here");
    expect(await panes(page)).toEqual([term]);

    // A live one's ⋯ offers only the fork.
    await row.locator("[data-conv-more]").click();
    await expect(page.getByRole("menuitem", { name: "Open as agent block" })).toHaveCount(0);
    await expect(page.locator("[data-fork-here]")).toHaveText("Fork here");
    await page.locator("[data-fork-here]").click();
    await expect.poll(() => agentBlock(page)).not.toBeNull();
    const el = paneEl(page, (await agentBlock(page))!);
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
    await menu(page, paneEl(page, term), "Your work…");
    await picker(page).locator(".yw-filter").fill("fish hawks");
    // Enter in the search continues the row it's on.
    await picker(page).locator(".yw-filter").press("Enter");
    await expect(picker(page)).toBeHidden();
    await expect.poll(async () => (await tab(page))?.name).toBe("Fish hawks");
    const pane = await active(page);
    expect(pane).not.toBe(term);
    // A terminal (its `kind` is what runs in it: the agent, `claude`).
    expect(await page.evaluate((p) => window.__arugula.client.info(p)?.type, pane)).toBe("terminal");
    expect(await agentBlock(page)).toBeNull();
  });

  test("its folder gone, it says so and runs it in home", async ({ page }) => {
    const { id, cwd } = seed("egret", "Gone birds");
    await reset(page);
    const [term] = await panes(page);
    await menu(page, paneEl(page, term), "Your work…");
    const dialog = picker(page);
    const row = dialog.locator(`[data-conversation="${id}"]`);
    await expect(row).toBeVisible();
    // Gone while it's listed.
    rmSync(cwd, { recursive: true, force: true });
    await row.click();
    await row.locator("[data-continue-here]").click();
    await expect(row.locator("[data-folder-gone]")).toContainText(`${cwd} is gone`);
    expect(await panes(page)).toEqual([term]);
    await row.locator("[data-continue-home]").click();
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
      await menu(page, paneEl(page, term), "Your work…");
      const dialog = picker(page);
      await dialog.locator(".yw-filter").fill("moor");
      const row = dialog.locator(`[data-conversation="${id}"]`);
      await row.click();
      const ask = row.locator("[data-move-ask]");
      await expect(ask).toHaveText(/^Running in .+\. Move here\?$/);
      expect(await ask.evaluate((el) => el.scrollWidth <= el.clientWidth)).toBe(true);
      const box = (await ask.boundingBox())!;
      const frame = (await dialog.boundingBox())!;
      expect(box.x + box.width).toBeLessThanOrEqual(frame.x + frame.width);

      const continued = page.waitForResponse((r) => r.url().includes("/api/conversations/continue"));
      await row.locator("[data-move-here]").click();
      const res = await continued;
      expect(res.ok()).toBe(true);
      expect(res.request().postDataJSON()).toMatchObject({ id, move_here: true });
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

  test("one that won't stop stays running there, and the row says to quit it there", async ({ page }) => {
    const { id } = seed("dunlin", "Mud birds");
    // This test's own process holds it, so the daemon's answer is the
    // spec's: a real move would stop the test. The daemon's refusals
    // (another user's, still running after the waits) are its own tests'.
    hold(id);
    await reset(page);
    const [term] = await panes(page);
    await page.route("**/api/conversations/continue", (r) =>
      r.fulfill({ status: 409, contentType: "application/json", body: JSON.stringify({ error: "Quit it there, then continue here (it didn't stop, in a terminal)" }) }),
    );
    await menu(page, paneEl(page, term), "Your work…");
    const row = picker(page).locator(`[data-conversation="${id}"]`);
    await row.click();
    await row.locator("[data-move-here]").click();
    await expect(row.locator("[data-move-refused]")).toHaveText(/^Quit it there, then continue here/);
    await expect(row.locator("[data-live]")).toBeVisible();
    expect(await panes(page)).toEqual([term]);
  });
});

test.describe("codex", () => {
  test("a Codex session is listed by its folder and continues in a terminal tab", async ({ page }) => {
    const { id, cwd } = seedCodex("heron");
    await reset(page);
    const [term] = await panes(page);
    await menu(page, paneEl(page, term), "Your work…");
    const dialog = picker(page);
    const row = dialog.locator(`[data-conversation="${id}"]`);
    await expect(row.locator(".yw-tool")).toHaveText("Codex");
    await expect(row.locator(".yw-first")).toHaveText("count the herons");
    await expect(dialog.locator(`[data-repo="${cwd}"] .yw-repo-name`)).toHaveText(cwd.split("/").pop()!);

    // One way on: Continue here, as Claude Code's, with no ⋯ (no agent
    // block, no fork).
    await row.click();
    const go = row.locator("[data-continue-here]");
    await expect(go).toHaveText("Continue here");
    await expect(row.locator("[data-conv-more]")).toHaveCount(0);
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
    await expect(page.locator(".your-work-dialog")).toHaveClass(/\bphone\b/);
    const row = dialog.locator(`[data-conversation="${id}"]`);
    await row.click();
    await row.locator("[data-continue-here]").click();
    await expect(dialog).toBeHidden();
    await expect.poll(async () => (await tab(page))?.name).toBe("Wading birds");
    expect(await agentBlock(page)).toBeNull();
  });
});
