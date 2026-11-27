// What your agent can do here (Discovery 7): the daemon's own answer
// (`GET /api/agent-can`, a line per MCP tool this machine offers), folded
// in the Start-an-agent dialog (an example goes into the prompt; Getting
// started's Agents step showed it too, until Lift 7). The test daemon
// hasn't joined control, so what control
// gives (Cloud workspaces, runs) isn't listed.

import { expect, test } from "@playwright/test";
import { closeContexts, menu, paneEl, panes, reset } from "./helpers";

test.afterAll(closeContexts);
test.use({ viewport: { width: 1440, height: 900 } });

test("the Start-an-agent dialog lists what an agent can do on this machine, and not what it can't", async ({ page }) => {
  await reset(page);
  const [term] = await panes(page);
  await menu(page, paneEl(page, term), "Start an agent…");
  const card = page.getByRole("dialog", { name: "Start an agent" }).locator("details[data-agent-can-card]");
  await card.locator("summary").click();
  await expect(card).toContainText("What it can do here");
  for (const tool of ["run", "show", "doc", "start_agent"]) await expect(card.locator(`[data-agent-can="${tool}"]`)).toHaveCount(1);
  await expect(card.locator('[data-agent-can="show"]')).toContainText("Ask it to show you things beside it");
  await expect(card.locator('[data-agent-can="show"] [data-agent-can-use]')).toContainText("Start the dev server and show it to me beside you.");
  // A part of another line, and what control would give.
  for (const tool of ["wait", "close", "cloud", "start_run"]) await expect(card.locator(`[data-agent-can="${tool}"]`)).toHaveCount(0);
});
