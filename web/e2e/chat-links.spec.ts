// A conversation's links (#856), on a local control with Alice and Bob on a
// team: Alice has two machines of her own (alices, the conversation's home,
// and alices-2), Bob one of his own (bobs). Every daemon is reached
// through control's relay; no machine is a team box.
//
// Alice makes the conversation from a pane's menu on alices (*Add to
// conversation…*, New…), which links that pane. She grants a pane on
// alices-2 and links it at home, and alices-2 refuses the home a pane she
// didn't grant, and refuses another conversation of hers the pane she did.
// She adds Bob, who reached none of her machines, as a member: being one
// routes him to alices, for the conversation. From his
// own machine he grants a pane there and links it into her conversation, so
// alices drives a pane on a teammate's machine. Each link's chip says it's
// live. Revoking a grant on the pane closes its link for good: on her own
// machine from its menu, and on Bob's, where the home hears "revoked".

import { expect, test, type Page } from "@playwright/test";
import { closeContexts, paneEl } from "./helpers";
import { TeamControl, call, home, live, show } from "./team-fixture";

const t = new TeamControl("links");

test.describe.configure({ mode: "serial" });
test.afterAll(async ({ browser }) => {
  await closeContexts({ browser });
  t.stop();
});

let alice: Page;
let bob: Page;
let conv = "";
let bobAccount = "";

const deviceOf = (page: Page, host: string) => page.evaluate((h) => window.__arugula.hosts.find(h)?.id ?? "", host);
const firstPane = async (page: Page) => {
  const panes = (await call(page, "GET", "/api/panes")).body as unknown as { id: number; epoch?: number }[];
  return panes[0].id;
};
/** Each link's `name:state`, as the home reads them now. */
async function states(page: Page): Promise<string[]> {
  await show(page, "alices");
  const r = await call(page, "GET", `/api/convs/${conv}/links`);
  expect(r.status, JSON.stringify(r.body)).toBe(200);
  return (r.body as unknown as { name: string; state: string }[]).map((s) => `${s.name}:${s.state}`);
}

test("a team, and two machines of Alice's and one of Bob's", async ({ browser }) => {
  test.setTimeout(180_000);
  await t.start();
  alice = await t.person(browser, "alice");
  await alice.evaluate(() => window.__arugula.control!.createTeam("Acme"));
  const team = await alice.evaluate(() => window.__arugula.control!.teams[0].team);
  const link = await alice.evaluate((tm) => window.__arugula.control!.invite(tm, "editor", true), team);
  bob = await t.person(browser, "bob");
  await bob.goto(link);
  await bob.locator("[data-accept-invite]").click();
  await expect(bob.locator("[data-invite-pending]")).toBeVisible();
  await alice.evaluate(() => window.__arugula.control!.refresh());
  await expect(alice.locator("[data-admit-yes]")).toBeVisible({ timeout: 15_000 });
  await alice.locator("[data-admit-yes]").click();
  await expect.poll(() => alice.evaluate(() => window.__arugula.control!.teams[0].roster.members.length), { timeout: 15_000 }).toBe(2);
  bobAccount = await alice.evaluate(() => window.__arugula.control!.teams[0].roster.members.find((m) => m.name === "bob")!.account);
  await t.addMachine(alice, "alices");
  await t.addMachine(alice, "alices-2");
  await t.addMachine(bob, "bobs");
  for (const p of [alice, bob]) await home(p);
  await expect.poll(() => live(alice, ["alices", "alices-2"]), { timeout: 30_000 }).toBe(true);
  await expect.poll(() => live(bob, ["bobs"]), { timeout: 30_000 }).toBe(true);
  // Their own machines aren't each other's.
  expect(await alice.evaluate(() => window.__arugula.hosts.names)).not.toContain("bobs");
  expect(await bob.evaluate(() => window.__arugula.hosts.names)).not.toContain("alices");
  // Each machine pins the team once its owner's browser shows it (#233).
  await show(bob, "bobs");
  await show(alice, "alices-2");
  await show(alice, "alices");
});

test("Add to conversation… on a pane makes a conversation there and links the pane", async () => {
  test.setTimeout(60_000);
  await show(alice, "alices");
  const pane = await firstPane(alice);
  await alice.evaluate((p) => window.__arugula.client.setActive(p), pane);
  await paneEl(alice, pane).click({ button: "right", position: { x: 60, y: 60 } });
  await alice.getByRole("menuitem", { name: "Add to conversation…" }).click();
  const dialog = alice.locator("[data-link-dialog]");
  await dialog.locator("input[data-link-new]").check();
  await dialog.locator("input[data-link-title]").fill("calc tests");
  await dialog.locator("button[data-link-go]").click();
  await expect(dialog).toHaveCount(0);
  await expect.poll(() => alice.evaluate(() => window.__arugula.client.state?.convs?.[0]?.links?.length ?? 0)).toBe(1);
  conv = await alice.evaluate(() => window.__arugula.client.state!.convs![0].id);
  const l1 = await alice.evaluate(() => window.__arugula.client.state!.convs![0].links[0]);
  expect(l1).toMatchObject({ name: "L1", pane, title: expect.any(String) });
  expect(l1.machine ?? null).toBeNull();
  // The pane lists its grant.
  const grants = (await call(alice, "GET", `/api/panes/${pane}/conv-grants`)).body as unknown as { conv: string }[];
  expect(grants.map((g) => g.conv)).toEqual([conv]);
});

test("a pane on another machine of hers, granted there, is linked; one she didn't grant is refused", async () => {
  test.setTimeout(90_000);
  const homeId = await deviceOf(alice, "alices");
  const otherId = await deviceOf(alice, "alices-2");
  expect(homeId && otherId).toBeTruthy();
  await show(alice, "alices-2");
  const granted = await firstPane(alice);
  // As *Add to conversation…* does it, through the client: the grant's
  // epoch (nanoseconds, above 2^53) is read exactly, as the daemon wrote
  // it (#889), not as JSON.parse rounds it.
  const g = await alice.evaluate(
    async ([p, h, c]) => {
      const client = window.__arugula.client;
      const grant = await client.grantConv(p as number, { device: h as string, device_name: "alices", conv: c as string, conv_title: "calc tests" });
      const raw = await (await client.request("GET", `/api/panes/${p}/conv-grants`)).text!();
      return { exact: grant.epoch_exact, epoch: grant.epoch, raw };
    },
    [granted, homeId, conv] as const,
  );
  expect(g.exact).toMatch(/^\d+$/);
  expect(g.raw).toContain(`"epoch":${g.exact}`);
  expect(Number(g.exact)).toBe(g.epoch);
  const run = await call(alice, "POST", "/api/run", { command: "sleep 600" });
  expect(run.status, JSON.stringify(run.body)).toBe(200);
  const unlinked = (run.body as { pane: number }).pane;
  expect(unlinked).not.toBe(granted);

  await show(alice, "alices");
  // Linked with the exact digits, still a JSON number to the home.
  const linked = await alice.evaluate(
    async ([c, m, p, e, x]) =>
      (await window.__arugula.client.linkConv(c as string, { machine: m as string, pane: p as number, epoch: e as number, title: "shell" }, x as string)).links.length,
    [conv, otherId, granted, g.epoch, g.exact] as const,
  );
  expect(linked).toBe(2);
  // The same machine, a pane nobody granted: the relay refuses the home.
  const refused = await call(alice, "POST", `/api/convs/${conv}/links`, { machine: otherId, pane: unlinked, title: "sleep" });
  expect(refused.status, JSON.stringify(refused.body)).toBe(403);
  expect(JSON.stringify(refused.body)).toContain("reaches only the agents here");
  // A grant is for one conversation: another one at the same home is
  // refused that pane too.
  const second = await call(alice, "POST", "/api/convs", { title: "another" });
  expect(second.status, JSON.stringify(second.body)).toBe(200);
  const other2 = await call(alice, "POST", `/api/convs/${(second.body as { id: string }).id}/links`, { machine: otherId, pane: granted, title: "shell" });
  expect(other2.status, JSON.stringify(other2.body)).toBe(403);
  expect((await call(alice, "DELETE", `/api/convs/${(second.body as { id: string }).id}`)).status).toBe(200);
  await expect.poll(() => states(alice), { timeout: 30_000 }).toEqual(["L1:live", "L2:live"]);
});

test("adding Bob routes him to her machine; he links a pane on his own", async () => {
  test.setTimeout(120_000);
  // From the members list: a teammate who doesn't reach alices yet.
  await show(alice, "alices");
  await alice.locator("[data-open-chat]").click();
  const chat = alice.locator("[data-chat]");
  await chat.locator(`[data-chat-channels] [data-chat-thread="conv-${conv}"]`).click();
  await chat.locator("[data-conv-people]").click();
  await chat.locator("[data-chat-details-panel] [data-conv-add]").click();
  const dialog = alice.locator("[data-conv-dialog]");
  await dialog.locator(`[data-conv-teammate="account:${bobAccount}"]`).click();
  await dialog.getByRole("button", { name: "Add" }).click();
  await expect(dialog).toHaveCount(0);
  await expect(chat.locator(`[data-conv-member="account:${bobAccount}"]`)).toBeVisible();

  // Bob's fleet now has alices, where he sees the conversation and nothing else.
  await home(bob);
  await expect.poll(() => bob.evaluate(() => window.__arugula.hosts.names), { timeout: 60_000, intervals: [2_000] }).toContain("alices");
  await show(bob, "alices");
  await expect.poll(() => bob.evaluate(() => (window.__arugula.client.state?.convs ?? []).map((c) => c.id))).toEqual([conv]);
  expect(await bob.evaluate(() => window.__arugula.client.state!.panes.length)).toBe(0);

  // He grants a pane on bobs to the conversation's home, and links it there.
  const homeId = await deviceOf(bob, "alices");
  const bobsId = await deviceOf(bob, "bobs");
  await show(bob, "bobs");
  const pane = await firstPane(bob);
  const g = await call(bob, "POST", `/api/panes/${pane}/conv-grants`, { device: homeId, device_name: "alices", conv, conv_title: "calc tests" });
  expect(g.status, JSON.stringify(g.body)).toBe(200);
  await show(bob, "alices");
  await expect
    .poll(
      async () => {
        const r = await call(bob, "POST", `/api/convs/${conv}/links`, { machine: bobsId, pane, epoch: (g.body as { epoch: number }).epoch, title: "bob's shell" });
        return r.status === 200 ? 200 : JSON.stringify(r.body);
      },
      { timeout: 60_000, intervals: [3_000] },
    )
    .toBe(200);
  await expect.poll(() => states(alice), { timeout: 30_000 }).toEqual(["L1:live", "L2:live", "L3:live"]);
});

test("each link's chip shows it live", async () => {
  const chat = alice.locator("[data-chat]");
  for (const name of ["L1", "L2", "L3"]) {
    const chip = chat.locator(`[data-conv-links] [data-conv-link="${name}"]`);
    await expect(chip).toHaveAttribute("data-link-known", "1", { timeout: 30_000 });
    await expect(chip).toHaveAttribute("data-link-state", "live");
  }
  await expect(chat.locator('[data-conv-link="L3"]')).toContainText("bobs");
});

test("revoking a grant on the pane closes its link for good", async () => {
  test.setTimeout(90_000);
  // On alices-2, from the pane's own menu.
  await alice.locator("[data-chat] [data-open-panes]").click();
  await expect(alice.locator("[data-chat]")).toBeHidden();
  await show(alice, "alices-2");
  const pane = await firstPane(alice);
  await alice.evaluate((p) => window.__arugula.client.setActive(p), pane);
  await paneEl(alice, pane).click({ button: "right", position: { x: 60, y: 60 } });
  await alice.getByRole("menuitem", { name: "Linked conversations…" }).click();
  const grants = alice.locator("[data-grants-dialog]");
  await grants.locator(`[data-revoke-grant="${conv}"]`).click();
  await expect(grants.locator("[data-grants-none]")).toBeVisible();
  await alice.keyboard.press("Escape");

  // On Bob's machine: the home is told it was revoked, not just shut out.
  await show(bob, "bobs");
  const bobsPane = await firstPane(bob);
  const homeId = await deviceOf(bob, "alices");
  const left = await call(bob, "POST", `/api/panes/${bobsPane}/conv-grants/revoke`, { device: homeId, conv });
  expect(left.status, JSON.stringify(left.body)).toBe(200);
  expect(left.body).toEqual([]);

  await expect.poll(() => states(alice), { timeout: 30_000 }).toEqual(["L1:live", "L2:closed", "L3:closed"]);
  // Closed for good, with why, in the record every member's page has.
  const why = () => alice.evaluate(() => window.__arugula.client.state!.convs![0].links.map((l) => `${l.name}:${l.closed ?? ""}`));
  await expect.poll(why).toEqual(["L1:", "L2:alices-2 revoked it, or its pane closed", "L3:bobs revoked it, or its pane closed"]);
  await alice.locator("[data-open-chat]").click();
  const chat = alice.locator("[data-chat]");
  await chat.locator(`[data-chat-channels] [data-chat-thread="conv-${conv}"]`).click();
  await expect(chat.locator('[data-conv-link="L2"]')).toHaveAttribute("data-link-state", "closed");
  await expect(chat.locator('[data-conv-link="L3"]')).toHaveAttribute("data-link-state", "closed");
});
