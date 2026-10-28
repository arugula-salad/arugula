// Discovery 10: the Start-an-agent dialog's More, folded, holds what
// `arugula agent` takes besides (permission mode, MCP servers, tools
// allowed without asking, Claude Code's own settings). A tool allowed
// there is approved without a card, as `--allow Bash` does. The agent is
// the scripted fake ACP server the daemon's tests use.

import { fileURLToPath } from "node:url";
import { expect, test, type Page } from "@playwright/test";
import { menu, paneEl, panes, reset } from "./helpers";

const fake = fileURLToPath(new URL("../../crates/daemon/tests/fake_acp.py", import.meta.url));

const agentBlock = (page: Page) =>
  page.evaluate(() => window.__arugula.client.state!.panes.find((p) => p.type === "agent")?.id ?? null);

test("More's allow list starts an agent that runs Bash without asking; a bad MCP line is refused", async ({ page }) => {
  await reset(page);
  const [term] = await panes(page);
  await menu(page, paneEl(page, term), "Start an agent…");
  const dialog = page.getByRole("dialog", { name: "Start an agent" });
  await dialog.locator("select[name=agent]").selectOption("acp");
  await dialog.locator("input[name=acp]").fill(`python3 ${fake}`);
  await dialog.locator("textarea[name=prompt]").fill("run ls --color");

  // Folded until opened; the defaults are empty.
  const more = dialog.locator("[data-agent-more]");
  await expect(more.locator("input[name=allow]")).toBeHidden();
  await more.locator("summary").click();
  await expect(more.locator("input[name=permission_mode]")).toHaveValue("");
  // Claude Code's own settings are Claude Code's only.
  await expect(more.locator("input[name=user_settings]")).toHaveCount(0);

  // An MCP server that isn't NAME=COMMAND is said, and nothing starts.
  await more.locator("textarea[name=mcp]").fill("just-a-name");
  await dialog.getByRole("button", { name: "Start" }).click();
  await expect(dialog.locator("[data-agent-mcp-error]")).toContainText("want NAME=COMMAND");
  await more.locator("textarea[name=mcp]").fill("");

  await more.locator("input[name=allow]").fill("Bash");
  await dialog.getByRole("button", { name: "Start" }).click();
  await expect(dialog).toBeHidden();
  await expect.poll(() => agentBlock(page)).not.toBeNull();
  const block = paneEl(page, (await agentBlock(page))!);
  // No card: the block approved it itself.
  const tool = block.locator(".agent-tool").first();
  await expect(tool.locator(".agent-tool-status")).toHaveText("completed", { timeout: 15_000 });
  await expect(block.getByRole("alertdialog")).toHaveCount(0);
  await expect(block.locator(".agent-msg")).toContainText("Ran it.");
});
