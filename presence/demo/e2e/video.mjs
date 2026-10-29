// Voice turns into video and back (#318's P8, Arugula #326), in headless
// Chromiums with fake cameras against a relay and a LiveKit of your own
// (or staging): Ada and Bea are in one room, Cy too. Ada waves at Bea,
// Bea says Talk, and they're in a voice call. Ada starts video: Bea's tile
// for Ada plays it (a real <video> with frames coming), Ada's own tile
// shows it, and Cy, in the room but not the call, still sees Ada's
// character, live. Ada's camera is shared, not opened twice: her page
// asks for a camera once. The publisher sends simulcast layers, and Bea
// gets the one her tile's size needs, and none while it's hidden. Ada
// stops video: her character comes back on Bea's tile, and the camera she
// turned on for it goes off.
//
// The same, in a meet room (guests, by link) with `--meet`, or both with
// `--both`:
//
//   docker run -d --name presence-livekit -p 127.0.0.1:7890:7880 \
//     -p 127.0.0.1:7891:7891 -p 127.0.0.1:7892:7892/udp \
//     livekit/livekit-server:latest --dev --bind 0.0.0.0 --config-body \
//     "$(printf 'port: 7880\nrtc:\n  tcp_port: 7891\n  udp_port: 7892\n  use_external_ip: false\n  node_ip: 127.0.0.1\nkeys:\n  devkey: secret\n')"
//   (cd ../relay && PORT=4209 DEMO=1 MEET_ORIGINS=http://127.0.0.1:4209 \
//     LIVEKIT_URL=ws://127.0.0.1:7890 LIVEKIT_API_KEY=devkey \
//     LIVEKIT_API_SECRET=secret mix phx.server)
//   pnpm build && node e2e/video.mjs [http://127.0.0.1:4209] [--meet|--both]
//
// (LiveKit's ICE candidates name its rtc ports, so the host's ports match
// the container's.) Screenshots go to e2e/out/ (not kept in git).

import { chromium } from "playwright";
import { mkdirSync } from "node:fs";
import { fileURLToPath } from "node:url";

const args = process.argv.slice(2);
const relay = (args.find((a) => !a.startsWith("--")) ?? process.env.RELAY ?? "http://127.0.0.1:4209").replace(/\/$/, "");
const where = args.includes("--both") ? ["demo", "meet"] : args.includes("--meet") ? ["meet"] : ["demo"];
const out = fileURLToPath(new URL("./out/", import.meta.url));
mkdirSync(out, { recursive: true });

const step = (what) => console.log(`· ${what}`);
let failed = false;
const check = (ok, what) => {
  console.log(`${ok ? "  ok" : "  FAIL"} ${what}`);
  if (!ok) failed = true;
};

const browser = await chromium.launch({
  args: ["--use-fake-ui-for-media-stream", "--use-fake-device-for-media-stream", "--autoplay-policy=no-user-gesture-required"],
});

// Before any page script: count camera opens, and keep each peer
// connection (for its stats: the layers sent, the size received).
const watch = () => {
  window.__cams = 0;
  const gum = navigator.mediaDevices.getUserMedia.bind(navigator.mediaDevices);
  navigator.mediaDevices.getUserMedia = (c) => {
    if (c && c.video) window.__cams++;
    return gum(c);
  };
  window.__pcs = [];
  const PC = window.RTCPeerConnection;
  window.RTCPeerConnection = function (...a) {
    const pc = new PC(...a);
    window.__pcs.push(pc);
    return pc;
  };
  window.RTCPeerConnection.prototype = PC.prototype;
};

async function context(name) {
  const ctx = await browser.newContext({ permissions: ["microphone", "camera"], viewport: { width: 1180, height: 820 } });
  await ctx.addInitScript(watch);
  const page = await ctx.newPage();
  page.on("pageerror", (e) => console.log(`  [${name}] page error: ${e.message}`));
  return page;
}

/** Into a demo room: /r/<room>, a name. */
async function demoPerson(name, room) {
  const page = await context(name);
  await page.goto(`${relay}/r/${room}`);
  await page.getByPlaceholder("your name").fill(name);
  await page.getByRole("button", { name: "Join the room" }).click();
  await page.locator(".room .bar .stats", { hasText: "here" }).waitFor();
  return page;
}

/** Into a meet room as a guest: the first starts it, the rest open its link. */
async function meetPerson(name, link) {
  const page = await context(name);
  if (!link) {
    await page.goto(`${relay}/meet/`);
    await page.getByLabel("Your name").fill(name);
    await page.getByRole("button", { name: "Continue as a guest" }).click();
    await page.getByLabel("The room's name").fill("Video check");
    await page.getByRole("button", { name: "Start a room" }).click();
    await page.waitForURL(/\/r\/[0-9a-z-]+$/);
  } else await page.goto(link);
  await page.getByLabel("Your name").fill(name);
  await page.getByRole("button", { name: "Join the room" }).click();
  await page.locator('[data-live="on"]').waitFor();
  return page;
}

/** What `page`'s tile for `name` shows: a playing video (its size, frames
 * over a second), a canvas (a character or outline), or an initial. */
const tileOf = (page, name) =>
  page.locator("figure.tile", { has: page.locator(".who", { hasText: name }) }).evaluate(async (fig) => {
    const v = fig.querySelector(".view video");
    const canvas = !!fig.querySelector(".view canvas");
    if (!v) return { video: false, canvas, live: fig.classList.contains("live") };
    const q0 = v.getVideoPlaybackQuality().totalVideoFrames;
    let callbacks = 0;
    const count = () => {
      callbacks++;
      v.requestVideoFrameCallback(count);
    };
    v.requestVideoFrameCallback(count);
    await new Promise((r) => setTimeout(r, 1000));
    return {
      video: true,
      canvas,
      live: fig.classList.contains("live"),
      width: v.videoWidth,
      height: v.videoHeight,
      paused: v.paused,
      frames: v.getVideoPlaybackQuality().totalVideoFrames - q0,
      callbacks,
      mirror: v.classList.contains("mirror"),
    };
  });

/** Video RTP on `page`'s peer connections: what goes out (each simulcast
 * layer's rid and size) and what comes in (its size). */
const rtp = (page) =>
  page.evaluate(async () => {
    const sent = [];
    const got = [];
    let track = null;
    for (const pc of window.__pcs) {
      if (pc.connectionState === "closed") continue;
      for (const s of pc.getSenders()) if (s.track?.kind === "video") track = s.track.getSettings();
      (await pc.getStats()).forEach((s) => {
        if (s.kind !== "video") return;
        if (s.type === "outbound-rtp") sent.push({ rid: s.rid ?? "", w: s.frameWidth ?? 0, h: s.frameHeight ?? 0, fps: Math.round(s.framesPerSecond ?? 0), active: s.active !== false });
        if (s.type === "inbound-rtp") got.push({ w: s.frameWidth ?? 0, h: s.frameHeight ?? 0, frames: s.framesDecoded ?? 0 });
      });
    }
    return { sent, got, track: track && { w: track.width, h: track.height, fps: track.frameRate } };
  });

async function until(fn, ok, tries = 40) {
  let v;
  for (let i = 0; i < tries; i++) {
    v = await fn();
    if (ok(v)) return v;
    await new Promise((r) => setTimeout(r, 500));
  }
  return v;
}

async function run(kind) {
  const tag = kind === "meet" ? "meet-" : "";
  const room = `video-proof-${Date.now().toString(36)}`;
  step(`${kind}: on ${relay}`);
  let a, b, c;
  if (kind === "meet") {
    a = await meetPerson("Ada");
    b = await meetPerson("Bea", a.url());
    c = await meetPerson("Cy", a.url());
  } else {
    a = await demoPerson("Ada", room);
    b = await demoPerson("Bea", room);
    c = await demoPerson("Cy", room);
  }
  const tile = (page, name) => page.locator("figure.tile", { has: page.locator(".who", { hasText: name }) });
  const bar = (page) => page.locator(".callbar.on");
  for (const p of [a, b, c]) await p.locator("figure.tile").nth(2).waitFor();
  step("all three in the room");

  step("Ada waves at Bea; Bea says Talk");
  await tile(a, "Bea").getByRole("button", { name: "Wave" }).click();
  const toast = b.locator(".toast", { hasText: "waved at you" });
  await toast.waitFor();
  await toast.getByRole("button", { name: "Talk" }).click();
  await bar(a).filter({ hasText: "In a call with Bea" }).waitFor({ timeout: 20_000 });
  await bar(b).filter({ hasText: "In a call with Ada" }).waitFor({ timeout: 20_000 });
  check(true, "a voice call, Ada and Bea");
  const before = await tileOf(b, "Ada");
  check(!before.video, `Bea's tile for Ada: no video in a voice call (${JSON.stringify(before)})`);
  await b.screenshot({ path: `${out}${tag}voice-bea.png` });

  step("Ada starts video");
  const camsBefore = await a.evaluate(() => window.__cams);
  await bar(a).getByRole("button", { name: "Start video" }).click();
  await bar(a).getByRole("button", { name: "Stop video" }).waitFor();
  const seen = await until(
    () => tileOf(b, "Ada"),
    (t) => t.video && t.width > 0 && t.frames > 5,
  );
  console.log(`  Bea's tile for Ada: ${JSON.stringify(seen)}`);
  check(seen.video && !seen.paused && seen.width > 0 && seen.frames > 5 && seen.callbacks > 5, "Bea's tile for Ada plays Ada's video, frames coming");
  check(!seen.canvas, "in place of her character");
  const own = await until(
    () => tileOf(a, "Ada"),
    (t) => t.video && t.width > 0,
  );
  check(own.video && own.width > 0 && own.mirror, `Ada's own tile shows her video, mirrored (${own.width}×${own.height})`);
  const camsAfter = await a.evaluate(() => window.__cams);
  check(camsAfter - camsBefore === 1, `Ada's page opened the camera once for presence and video together (${camsAfter - camsBefore} getUserMedia)`);
  const cy = await until(
    () => tileOf(c, "Ada"),
    (t) => t.live && t.canvas,
  );
  check(!cy.video && cy.canvas && cy.live, `Cy, not in the call, sees Ada's character, live: presence carries on (${JSON.stringify(cy)})`);
  const lay = await until(
    () => rtp(a),
    (r) => r.sent.filter((s) => s.w > 0).length >= 2,
  );
  console.log(`  Ada's published track ${JSON.stringify(lay.track)}; she sends ${JSON.stringify(lay.sent)}`);
  check(lay.sent.filter((s) => s.w > 0).length >= 2, "simulcast: Ada sends more than one layer");
  check(Math.max(...lay.sent.map((s) => s.w)) <= 640, "the top layer is at most 640 wide");
  const got = await rtp(b);
  console.log(`  Bea receives ${JSON.stringify(got.got)}`);
  check(got.got.some((g) => g.w > 0), "Bea receives one video, sized for her tile");

  step("Bea's tile for Ada goes out of view, then back");
  const decoded = async () => (await rtp(b)).got.reduce((n, g) => n + g.frames, 0);
  const shown = (on) => tile(b, "Ada").evaluate((fig, on) => (fig.style.display = on ? "" : "none"), on);
  await shown(false);
  await b.waitForTimeout(2500);
  const h0 = await decoded();
  await b.waitForTimeout(2000);
  const hidden = (await decoded()) - h0;
  await shown(true);
  await b.waitForTimeout(2500);
  const s0 = await decoded();
  await b.waitForTimeout(2000);
  const back = (await decoded()) - s0;
  check(hidden <= 2 && back > 10, `adaptive stream: Bea decodes ${hidden} frames in 2 s with the tile hidden, ${back} once it's back`);

  step("Bea says so in the chat, under the call");
  await b.getByRole("textbox", { name: "Message" }).fill("I can see you!");
  await b.getByRole("textbox", { name: "Message" }).press("Enter");
  await a.locator(".chat-msg", { hasText: "I can see you!" }).waitFor();
  check(true, "text carries on too");
  await a.waitForTimeout(500);
  await a.screenshot({ path: `${out}${tag}video-ada.png` });
  await b.screenshot({ path: `${out}${tag}video-bea.png` });
  await c.screenshot({ path: `${out}${tag}video-cy.png` });

  step("Ada stops video");
  await bar(a).getByRole("button", { name: "Stop video" }).click();
  const after = await until(
    () => tileOf(b, "Ada"),
    (t) => !t.video && t.canvas,
  );
  check(!after.video && after.canvas, `Bea's tile for Ada: her character is back (${JSON.stringify(after)})`);
  const camOff = await a.getByRole("button", { name: "Turn on my camera" }).count();
  check(camOff > 0, "Ada's camera, turned on for the video, is off again");
  check((await bar(b).filter({ hasText: "In a call with Ada" }).count()) === 1, "still a voice call");
  await b.screenshot({ path: `${out}${tag}back-bea.png` });

  step("Ada leaves");
  await bar(a).getByRole("button", { name: "Leave" }).click();
  await bar(b).waitFor({ state: "detached", timeout: 15_000 });
  check(true, "Bea, alone, left too");
  for (const p of [a, b, c]) await p.context().close();
  step(`screenshots: ${["voice-bea", "video-ada", "video-bea", "video-cy", "back-bea"].map((n) => `${out}${tag}${n}.png`).join(", ")}`);
}

try {
  for (const kind of where) await run(kind);
} catch (e) {
  failed = true;
  console.log(`  FAIL ${e.message}`);
} finally {
  await browser.close();
}
console.log(failed ? "the video proof FAILED" : "the video proof passed");
process.exit(failed ? 1 : 0);
