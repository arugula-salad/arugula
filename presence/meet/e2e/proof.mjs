// meet's proof (#318 P6): two people hang out in a room on a local relay.
//
// One signs in by an emailed link (read from the relay's log: a dev relay
// logs its mail), starts a room and copies its link; a guest opens the
// link with a name. They see each other's tiles, trade messages, one
// waves and the other waves back, and one reacts. Screenshots of the home
// page, the rooms list and the room (desktop and phone) go to e2e/out.
//
// Against a dev relay with meet built into it (pnpm build here, which
// writes ../relay/priv/static/meet), its log going to a file:
//
//   cd ../relay && PORT=4206 MEET_ORIGINS=http://localhost:4206 \
//     mix phx.server > /tmp/relay.log 2>&1
//   MEET_URL=http://localhost:4206/meet MEET_RELAY_LOG=/tmp/relay.log pnpm e2e
//
// MEET_URL may be a meet. host's root too (http://meet.localhost:4206).

import { chromium } from "playwright";
import { mkdirSync, readFileSync, statSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const BASE = (process.env.MEET_URL ?? "http://localhost:4206/meet").replace(/\/$/, "");
const LOG = process.env.MEET_RELAY_LOG;
const OUT = join(dirname(fileURLToPath(import.meta.url)), "out");
const EMAIL = `jake+${Date.now()}@example.com`;
if (!LOG) throw new Error("MEET_RELAY_LOG: the dev relay's log file, where its mail goes");
mkdirSync(OUT, { recursive: true });

const PHONE = { width: 390, height: 844 };
const DESKTOP = { width: 1280, height: 860 };

const step = (s) => console.log(`· ${s}`);

/** The newest sign-in link for `email` the relay has logged since `from`. */
async function linkFor(email, from) {
  for (let i = 0; i < 50; i++) {
    const log = readFileSync(LOG, "utf8").slice(from);
    const at = log.lastIndexOf(email);
    const m = at >= 0 && /https?:\/\/[^\s"]+\/auth\/email\/[A-Za-z0-9_-]+/.exec(log.slice(at));
    if (m) return m[0];
    await new Promise((r) => setTimeout(r, 100));
  }
  throw new Error(`no sign-in link for ${email} in ${LOG}`);
}

const browser = await chromium.launch({
  args: ["--use-fake-ui-for-media-stream", "--use-fake-device-for-media-stream"],
});
let failed = false;
try {
  const origin = new URL(BASE).origin;
  const jakeCtx = await browser.newContext({ viewport: DESKTOP, permissions: ["camera", "clipboard-read", "clipboard-write"] });
  await jakeCtx.grantPermissions(["camera", "clipboard-read", "clipboard-write"], { origin });
  const mayaCtx = await browser.newContext({ viewport: DESKTOP, permissions: ["camera"] });
  const jake = await jakeCtx.newPage();
  const maya = await mayaCtx.newPage();
  for (const [who, p] of [["jake", jake], ["maya", maya]]) p.on("pageerror", (e) => console.error(`${who}: page error: ${e.message}`));

  step("the home page, signed out");
  await jake.goto(`${BASE}/`);
  await jake.getByRole("heading", { name: "Come in as a guest" }).waitFor();
  await jake.waitForTimeout(600);
  await jake.screenshot({ path: join(OUT, "home-desktop.png") });
  await jake.setViewportSize(PHONE);
  await jake.screenshot({ path: join(OUT, "home-phone.png"), fullPage: true });
  await jake.setViewportSize(DESKTOP);

  step(`Jake asks for a sign-in link (${EMAIL})`);
  const from = statSync(LOG).size;
  await jake.getByLabel("Your email").fill(EMAIL);
  await jake.getByRole("button", { name: "Email me a link" }).click();
  await jake.getByText("Check your email").waitFor();
  const link = await linkFor(EMAIL, from);

  step("and follows it");
  await jake.goto(link);
  await jake.getByRole("button", { name: "Sign in" }).click();
  await jake.waitForURL(`${BASE}/`);
  await jake.getByText(EMAIL).waitFor();

  step("starts a room");
  await jake.getByLabel("The room's name").fill("Friday hang");
  await jake.getByRole("button", { name: "Start a room" }).click();
  await jake.waitForURL(/\/r\/[0-9a-z]{4}-[0-9a-z]{4}$/);
  await jake.getByLabel("Your name").fill("Jake");
  await jake.getByRole("button", { name: "Join the room" }).click();
  await jake.locator('[data-live="on"]').waitFor();

  step("copies its link");
  await jake.getByRole("button", { name: "Copy the link" }).click();
  await jake.getByRole("button", { name: "Link copied" }).waitFor();
  const roomLink = await jake.evaluate(() => navigator.clipboard.readText());
  if (roomLink !== jake.url()) throw new Error(`copied ${roomLink}, the room is ${jake.url()}`);

  step("Maya opens the link as a guest");
  await maya.goto(roomLink);
  await maya.getByRole("heading", { name: "Friday hang" }).waitFor();
  await maya.getByLabel("Your name").fill("Maya");
  await maya.getByRole("button", { name: "Join the room" }).click();
  await maya.locator('[data-live="on"]').waitFor();

  step("they see each other");
  await jake.locator('figure.tile[data-name="Maya"]').waitFor();
  await maya.locator('figure.tile[data-name="Jake"]').waitFor();
  await jake.getByText("2 here").waitFor();
  await maya.getByText("2 here").waitFor();

  step("and talk");
  await jake.getByLabel("Message").fill("hey Maya, welcome in");
  await jake.getByLabel("Message").press("Enter");
  await maya.locator(".chat-msg", { hasText: "hey Maya, welcome in" }).waitFor();
  await maya.getByLabel("Message").fill("hi Jake! this is nice");
  await maya.getByLabel("Message").press("Enter");
  await jake.locator(".chat-msg", { hasText: "hi Jake! this is nice" }).waitFor();
  // Each sees their own as "You".
  await jake.locator(".chat-msg.mine", { hasText: "hey Maya" }).waitFor();
  await maya.locator(".chat-msg.mine", { hasText: "hi Jake!" }).waitFor();

  step("Jake waves; Maya sees it and waves back");
  await jake.locator('figure.tile[data-name="Maya"]').getByRole("button", { name: "Wave" }).click();
  await maya.getByRole("alert").getByText("Jake waved at you.").waitFor();
  await maya.getByRole("button", { name: "Wave back" }).click();
  await jake.locator('figure.tile[data-name="Maya"]').getByRole("button", { name: "waved back" }).waitFor();

  step("Maya reacts 🎉; Jake sees it over her tile");
  await maya.getByRole("button", { name: "React 🎉" }).click();
  await jake.locator('figure.tile[data-name="Maya"] .react', { hasText: "🎉" }).waitFor();
  await jake.waitForTimeout(300);
  await jake.screenshot({ path: join(OUT, "room-desktop.png") });

  step("Maya's view on a phone");
  await maya.setViewportSize(PHONE);
  await maya.getByRole("button", { name: "React 👋" }).click();
  await maya.waitForTimeout(400);
  await maya.screenshot({ path: join(OUT, "room-phone.png") });
  await maya.screenshot({ path: join(OUT, "room-phone-full.png"), fullPage: true });

  step("Jake's rooms list has the room");
  await jake.goto(`${BASE}/`);
  await jake.getByRole("heading", { name: "Your rooms" }).waitFor();
  await jake.locator(".room-card", { hasText: "Friday hang" }).waitFor();

  step("Maya signs in too, and the room is in her Lately");
  const from2 = statSync(LOG).size;
  const mayaEmail = EMAIL.replace("jake", "maya");
  await maya.goto(roomLink);
  await maya.getByRole("button", { name: "Or sign in, to keep this room in your list" }).click();
  await maya.getByLabel("Your email").fill(mayaEmail);
  await maya.getByRole("button", { name: "Email me a link" }).click();
  await maya.getByText("Check your email").waitFor();
  await maya.goto(await linkFor(mayaEmail, from2));
  await maya.getByRole("button", { name: "Sign in" }).click();
  await maya.waitForURL(roomLink);
  await maya.getByRole("button", { name: "Join the room" }).click();
  await maya.locator('[data-live="on"]').waitFor();
  // History: what was said before she came back.
  await maya.locator(".chat-msg", { hasText: "hey Maya, welcome in" }).waitFor();
  await maya.goto(`${BASE}/`);
  await maya.getByRole("heading", { name: "Lately" }).waitFor();
  await maya.locator(".lists .room-card", { hasText: "Friday hang" }).waitFor();

  step("screenshots of the rooms list");
  await jake.goto(`${BASE}/`);
  await jake.getByRole("heading", { name: "Your rooms" }).waitFor();
  await jake.waitForTimeout(400);
  await jake.screenshot({ path: join(OUT, "rooms-desktop.png"), fullPage: true });
  await maya.screenshot({ path: join(OUT, "rooms-phone.png"), fullPage: true });

  console.log(`ok: Jake and Maya hung out in ${roomLink}; screenshots in ${OUT}`);
} catch (e) {
  failed = true;
  console.error(`failed: ${e.stack ?? e}`);
} finally {
  await browser.close();
}
process.exit(failed ? 1 : 0);
