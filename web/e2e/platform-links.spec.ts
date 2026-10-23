// Old links of the platform's (M154): the shell's `#chat…`, `#cloud=…` and
// `#cloud-ws…` routes, with real parameters, land on the platform page's
// paths, forever; the fragments that carry secrets (`#join=`, `#device=`,
// invites, `#link=`) are never touched; and without a platform page the
// shell's routes are as they were.

import { expect, test, type Page } from "@playwright/test";
import { closeContexts } from "./helpers";
import { TeamControl, home } from "./team-fixture";

const t = new TeamControl("plinks");

test.describe.configure({ mode: "serial" });
test.afterAll(async ({ browser }) => {
  await closeContexts({ browser });
  t.stop();
});

let alice: Page;
let app = "";
let room = "";
let seq = 0;

test("Alice, her team's #general, and a message in it", async ({ browser }) => {
  test.setTimeout(120_000);
  await t.start(["--platform-host", "app.localhost"]);
  app = t.base.replace("//127.0.0.1", "//app.localhost");
  alice = await t.person(browser, "alice");
  await alice.evaluate(() => window.__arugula.control!.createTeam("Acme"));
  const team = await alice.evaluate(() => window.__arugula.control!.teams[0].team);
  await alice.evaluate((tm) => window.__arugula.control!.setLabsFlag("chat", true, tm), team);
  await home(alice);
  await expect.poll(() => alice.evaluate(() => window.__arugula.chat.on), { timeout: 15_000 }).toBe(true);
  room = await alice.evaluate(async (tm) => (await window.__arugula.chat.newChannel(tm, "general")).id, team);
  await alice.evaluate(async (r) => void (await window.__arugula.chat.post(r, "first")), room);
  seq = await alice.evaluate(async (r) => (await window.__arugula.chat.post(r, "the one a push named")).seq, room);
});

/** Open `hash` on control's page and say where the browser lands. */
async function lands(hash: string): Promise<string> {
  await alice.goto("about:blank");
  await alice.goto(`${t.base}/${hash}`);
  await alice.waitForURL((u) => u.origin === app, { timeout: 15_000 });
  // Let an id-only address find its project.
  await expect.poll(() => new URL(alice.url()).pathname.startsWith("/p/") || new URL(alice.url()).pathname.startsWith("/dm/"), { timeout: 15_000 }).toBe(true);
  return alice.url();
}

test("each old route of chat's lands on the platform page", async () => {
  test.setTimeout(120_000);
  const project = /^\/p\/(prj_[0-9a-f]+)-acme\/chat/;
  let at = new URL(await lands(`#chat=${room}&msg=${seq}`));
  expect(at.pathname).toMatch(new RegExp(`^/p/prj_[0-9a-f]+-acme/chat/${room}-general$`));
  expect(at.searchParams.get("msg")).toBe(String(seq));
  // …at that message, flashed.
  await expect(alice.locator(`[data-msg="${seq}"]`)).toContainText("the one a push named");
  at = new URL(await lands("#chat"));
  expect(at.pathname).toMatch(project);
  at = new URL(await lands("#chat=activity"));
  expect(at.pathname).toMatch(/\/chat\/activity$/);
  await expect(alice.locator("[data-chat-activity-view]")).toBeVisible();
  at = new URL(await lands("#chat=search/one%20a%20push"));
  expect(at.pathname).toMatch(/\/chat\/search$/);
  expect(at.searchParams.get("q")).toBe("one a push");
  await expect(alice.locator("[data-chat-search-view]")).toContainText("the one a push named", { timeout: 15_000 });
  // A machine's own thread, from before M125: chat, from the top.
  at = new URL(await lands("#chat=laptop/pane-3"));
  expect(at.pathname).toMatch(project);
});

test("followed on control's page as a hash change, an old link lands too", async () => {
  await home(alice);
  await alice.evaluate((r) => (location.hash = `chat=${r}`), room);
  await alice.waitForURL((u) => u.origin === app, { timeout: 15_000 });
  await expect(alice).toHaveURL(new RegExp(`/chat/${room}-general$`), { timeout: 15_000 });
});

test("the fragments that carry secrets stay on control's page, untouched", async () => {
  for (const hash of ["#join=ABCD-EFGH-IJKL.0123456789abcdef", "#device=WXYZ-1234.0123456789abcdef", "#pinvite=0011223344556677.seed", "#swarm"]) {
    await alice.goto("about:blank");
    await alice.goto(`${t.base}/${hash}`);
    await alice.waitForTimeout(1500);
    // Never sent to the platform page: the shell's to read (or refuse).
    expect(new URL(alice.url()).origin, hash).toBe(t.base);
  }
});
