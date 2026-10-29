// A wave becomes a voice call (#318's P7, Arugula #325), in two headless
// Chromiums against a relay and a LiveKit of your own: both join one demo
// room, A waves at B, B says Talk, and both end up in one LiveKit room with
// the other's audio playing (and level on it: the fake microphone's beep).
// Text carries on under the call; then A mutes (B's tile says so) and
// leaves (B, alone, leaves too).
//
// `--calls-off` checks a relay without LiveKit keys instead (as production
// is until it has them): a wave stays a wave, and no call starts.
//
//   docker run -d --name presence-livekit -p 127.0.0.1:7880:7880 \
//     -p 127.0.0.1:7881:7881 -p 127.0.0.1:7882:7882/udp \
//     livekit/livekit-server:latest --dev --bind 0.0.0.0
//   (cd ../relay && PORT=4207 DEMO=1 LIVEKIT_URL=ws://127.0.0.1:7880 \
//     LIVEKIT_API_KEY=devkey LIVEKIT_API_SECRET=secret mix phx.server)
//   pnpm build && node e2e/call.mjs [http://127.0.0.1:4207] [--calls-off]
//
// The in-call screenshot goes to e2e/out/ (not kept in git).

import { chromium } from "playwright";
import { mkdirSync } from "node:fs";
import { fileURLToPath } from "node:url";

const args = process.argv.slice(2);
const callsOff = args.includes("--calls-off");
const relay = args.find((a) => !a.startsWith("--")) ?? process.env.RELAY ?? "http://127.0.0.1:4207";
const out = fileURLToPath(new URL("./out/", import.meta.url));
const room = `call-proof-${Date.now().toString(36)}`;

const step = (what) => console.log(`· ${what}`);

const browser = await chromium.launch({
  args: ["--use-fake-ui-for-media-stream", "--use-fake-device-for-media-stream", "--autoplay-policy=no-user-gesture-required"],
});

/** A person in the room: their own browser context (their own storage). */
async function person(name) {
  const ctx = await browser.newContext({ permissions: ["microphone", "camera"], viewport: { width: 1180, height: 820 } });
  const page = await ctx.newPage();
  page.on("pageerror", (e) => console.log(`  [${name}] page error: ${e.message}`));
  await page.goto(`${relay}/r/${room}`);
  await page.getByPlaceholder("your name").fill(name);
  await page.getByRole("button", { name: "Join the room" }).click();
  await page.locator(".room .bar .stats", { hasText: "here" }).waitFor();
  return page;
}

/** What `page` hears: each playing call audio element's track state and
 * its loudest level over half a second (WebAudio on the element's stream). */
const hearing = (page) =>
  page.evaluate(async () => {
    const els = [...document.querySelectorAll("audio")];
    return Promise.all(
      els.map(async (el) => {
        const stream = el.srcObject;
        const track = stream?.getAudioTracks()[0];
        if (!track) return { track: false };
        const ctx = new AudioContext();
        await ctx.resume();
        const an = ctx.createAnalyser();
        ctx.createMediaStreamSource(stream).connect(an);
        const buf = new Uint8Array(an.fftSize);
        let peak = 0;
        for (let i = 0; i < 25; i++) {
          await new Promise((r) => setTimeout(r, 20));
          an.getByteTimeDomainData(buf);
          for (const v of buf) peak = Math.max(peak, Math.abs(v - 128));
        }
        await ctx.close();
        return { track: true, state: track.readyState, muted: track.muted, paused: el.paused, peak };
      }),
    );
  });

let failed = false;
const check = (ok, what) => {
  console.log(`${ok ? "  ok" : "  FAIL"} ${what}`);
  if (!ok) failed = true;
};

try {
  step(`room ${room} on ${relay}`);
  const a = await person("Ada");
  const b = await person("Bea");
  await a.locator(".tile").nth(1).waitFor();
  await b.locator(".tile").nth(1).waitFor();
  step("both in the room");

  step("Ada waves at Bea");
  await a.locator(".tile").nth(1).getByRole("button", { name: "Wave" }).click();
  if (callsOff) {
    await b.locator(".toast", { hasText: "Calls aren't set up" }).waitFor();
    check(true, "Bea gets a wave, which says calls aren't set up here");
    await b.locator(".toast").getByRole("button", { name: "Wave back" }).click();
    await a.locator(".tile").nth(1).getByRole("button", { name: "waved back" }).waitFor();
    check(true, "Bea waves back; Ada's tile says so");
    check((await a.locator(".callbar").count()) === 0 && (await b.locator(".callbar").count()) === 0, "no call bar on either page");
    throw null;
  }
  await a.locator(".callbar", { hasText: "Calling Bea" }).waitFor();
  const toast = b.locator(".toast", { hasText: "waved at you" });
  await toast.waitFor();
  step("Bea says Talk");
  await toast.getByRole("button", { name: "Talk" }).click();

  await a.locator(".callbar", { hasText: "In a call with Bea" }).waitFor({ timeout: 20_000 });
  await b.locator(".callbar", { hasText: "In a call with Ada" }).waitFor({ timeout: 20_000 });
  check(true, "both connected to the LiveKit room, each with the other in it");

  // Remote audio: subscribed, attached, live, and carrying sound.
  const wait = async (page) => {
    for (let i = 0; i < 40; i++) {
      const h = await hearing(page);
      if (h.some((x) => x.track && x.state === "live" && x.peak > 2)) return h;
      await page.waitForTimeout(250);
    }
    return hearing(page);
  };
  const ha = await wait(a);
  const hb = await wait(b);
  console.log(`  Ada hears ${JSON.stringify(ha)}`);
  console.log(`  Bea hears ${JSON.stringify(hb)}`);
  check(ha.some((x) => x.track && x.state === "live" && x.peak > 2), "Ada has Bea's audio track subscribed, and it has sound");
  check(hb.some((x) => x.track && x.state === "live" && x.peak > 2), "Bea has Ada's audio track subscribed, and it has sound");
  check((await b.locator(".tile", { hasText: "Ada" }).locator(".tag.call").textContent()) === "in a call", "Bea's tile for Ada says in a call");

  step("Bea says hi in the chat, under the call");
  await b.getByRole("textbox", { name: "Message" }).fill("can you hear me?");
  await b.getByRole("textbox", { name: "Message" }).press("Enter");
  await a.locator(".chat-msg", { hasText: "can you hear me?" }).waitFor();
  check(true, "Ada gets it: presence and text carry on under the call");

  mkdirSync(out, { recursive: true });
  await a.screenshot({ path: `${out}in-call-ada.png` });
  await b.screenshot({ path: `${out}in-call-bea.png` });
  step(`screenshots: ${out}in-call-ada.png, ${out}in-call-bea.png`);

  step("Ada mutes");
  await a.locator(".callbar").getByRole("button", { name: "Mute" }).click();
  await b.locator(".tile", { hasText: "Ada" }).locator(".tag.call", { hasText: "muted" }).waitFor({ timeout: 10_000 });
  check(true, "Bea's tile for Ada says muted");

  step("Ada leaves");
  await a.locator(".callbar").getByRole("button", { name: "Leave" }).click();
  await a.locator(".callbar.on").waitFor({ state: "detached" });
  await b.locator(".callbar.on").waitFor({ state: "detached", timeout: 15_000 });
  check(true, "Bea, alone, left too; presence carries on");
  check((await b.locator(".tile").count()) === 2, "both still in the room");
} catch (e) {
  // null: the calls-off run's done.
  if (e !== null) {
    failed = true;
    console.log(`  FAIL ${e.message}`);
  }
} finally {
  await browser.close();
}
console.log(failed ? "the call proof FAILED" : "the call proof passed");
process.exit(failed ? 1 : 0);
