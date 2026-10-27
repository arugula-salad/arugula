// A hint from facts (DECISIONS.md "Hints come from facts, once per
// account", hints.spec.ts has the rest): a team made. On control's page,
// with a machine shown, a toast says once that a link invites and the
// team can have chat; Teams… opens there. Seen is the account's here
// (control's /api/me/prefs).
// Hints keep quiet under a test runner unless a spec asks: this one does.

import { expect, test, type Page } from "@playwright/test";
import { closeContexts } from "./helpers";
import { TeamControl, home, show } from "./team-fixture";

const t = new TeamControl("hints-team");

test.afterAll(async ({ browser }) => {
  await closeContexts({ browser });
  t.stop();
});

const hints = (p: Page) => p.addInitScript(() => Object.assign(window, { __arugulaHints: true }));

test("a team made: Invite with a link · chat, in Teams…", async ({ browser }) => {
  test.setTimeout(180_000);
  await t.start();
  const alice = await t.person(browser, "alice");
  await hints(alice);
  await alice.evaluate(() => window.__arugula.control!.createTeam("Acme"));
  await t.addMachine(alice, "hintbox");
  await home(alice);
  await show(alice, "hintbox");

  const toast = alice.locator('.hint-toast[data-hint="team-invite"]');
  await expect(toast).toContainText("Invite with a link · Turn on chat for this team");
  await toast.locator("[data-hint-act]").click();
  await expect(alice.getByRole("heading", { name: "Teams" })).toBeVisible();
  await expect(alice.locator("[data-hint]")).toHaveCount(0);
});
