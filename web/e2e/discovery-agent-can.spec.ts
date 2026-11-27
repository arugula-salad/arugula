// What your agent can do here (Discovery 7): the daemon's own answer
// (`GET /api/agent-can`, a line per MCP tool this machine offers), shown in
// Getting started's Agents step, and folded in the Start-an-agent dialog
// (an example goes into the prompt). The test daemon hasn't joined control, so what control
// gives (Cloud workspaces, runs) isn't listed.

import { expect, test } from "@playwright/test";
import { closeContexts, reset } from "./helpers";

test.afterAll(closeContexts);
test.use({ viewport: { width: 1440, height: 900 } });

test("the Agents step lists what an agent can do on this machine, and not what it can't", async ({ page }) => {
  await reset(page);
  await page.evaluate(() => dispatchEvent(new CustomEvent("arugula:getting-started", { detail: "agents" })));
  const card = page.getByRole("dialog", { name: "Getting started" }).locator("[data-agent-can-card]");
  await expect(card).toContainText("What your agent can do here");
  for (const tool of ["run", "show", "doc", "start_agent"]) await expect(card.locator(`[data-agent-can="${tool}"]`)).toHaveCount(1);
  await expect(card.locator('[data-agent-can="show"]')).toContainText("Ask it to show you things beside it");
  await expect(card.locator('[data-agent-can="show"] [data-agent-can-example]')).toHaveText("Start the dev server and show it to me beside you.");
  // A part of another line, and what control would give.
  for (const tool of ["wait", "close", "cloud", "start_run"]) await expect(card.locator(`[data-agent-can="${tool}"]`)).toHaveCount(0);
});
