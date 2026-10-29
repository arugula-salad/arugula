// The dashboard's proof (#318 P10): a new customer sets up an app end to
// end, on a local relay, without us.
//
// Maya signs in by an emailed link (read from the relay's log: a dev relay
// logs its mail) and makes an app. She adds an origin (a wrong one first),
// copies a test key, sets an OpenID Connect provider (its errors first,
// then right), a room rule, guests on and a cap. Then a little page of
// hers, on that origin, joins a room with the copied key as a guest, and
// the same page on another origin is refused. Usage is the relay's 404
// (before #318's P9 meters) and then a stub of P9's answer. Screenshots
// of each screen, desktop and phone, go to e2e/out.
//
// Against a dev relay with the dashboard and the SDK built into it (pnpm
// build here and in ../elements), its log going to a file:
//
//   cd ../relay && PORT=4211 mix phx.server > /tmp/relay.log 2>&1
//   DASHBOARD_URL=http://localhost:4211 RELAY_LOG=/tmp/relay.log pnpm e2e
//
// Her page is served from :4213 (SITE_PORT) as app.localhost, an origin
// that isn't localhost itself (a test key works from any localhost).

import { chromium } from "playwright";
import { createServer } from "node:http";
import { mkdirSync, readFileSync, statSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const RELAY = (process.env.DASHBOARD_URL ?? "http://localhost:4211").replace(/\/$/, "");
const BASE = `${RELAY}/dashboard`;
const LOG = process.env.RELAY_LOG;
const SITE_PORT = Number(process.env.SITE_PORT ?? 4213);
const SITE = `http://app.localhost:${SITE_PORT}`;
const ELSEWHERE = `http://elsewhere.localhost:${SITE_PORT}`;
const OUT = join(dirname(fileURLToPath(import.meta.url)), "out");
const EMAIL = `maya+${Date.now()}@example.com`;
if (!LOG) throw new Error("RELAY_LOG: the dev relay's log file, where its mail goes");
mkdirSync(OUT, { recursive: true });

const PHONE = { width: 390, height: 844 };
const DESKTOP = { width: 1280, height: 900 };

const step = (s) => console.log(`· ${s}`);
const check = (ok, what) => {
  if (!ok) throw new Error(what);
};

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

// Maya's own page: the SDK from the relay, her key, a room, as a guest. It
// says how it went in #status (data-live: on, or out) and #socket (a bare
// socket with the same key: open, or refused).
const PAGE = `<!doctype html>
<meta charset="utf-8"><title>Sketchpad</title>
<p id="socket" data-state="trying">trying</p>
<p id="status" data-live="connecting">connecting</p>
<div id="people"></div>
<script type="module">
  const q = new URLSearchParams(location.search);
  const key = q.get("key"), room = q.get("room"), relay = q.get("relay");
  const socket = document.getElementById("socket");
  const ws = new WebSocket(relay + "/websocket?vsn=2.0.0&" + new URLSearchParams({ key, room, name: "Maya" }));
  ws.onopen = () => ((socket.dataset.state = socket.textContent = "open"), ws.close());
  ws.onclose = () => socket.dataset.state === "trying" && (socket.dataset.state = socket.textContent = "refused");
  const status = document.getElementById("status");
  const set = (s, text) => ((status.dataset.live = s), (status.textContent = text));
  try {
    const { connect } = await import(q.get("sdk"));
    const r = await connect({ room, key, guest: "Maya", relay });
    const inRoom = () => r.members.some((m) => m.name === "Maya");
    for (let i = 0; i < 30 && !inRoom(); i++) await new Promise((ok) => setTimeout(ok, 100));
    if (inRoom()) {
      r.mount(document.getElementById("people"), { size: "head" });
      set("on", "Maya is in " + room);
    } else set("out", "not in");
  } catch (e) {
    set("out", e.message);
  }
</script>`;

const site = createServer((req, res) => {
  res.writeHead(200, { "content-type": "text/html; charset=utf-8" });
  res.end(PAGE);
}).listen(SITE_PORT, "127.0.0.1");

// P9's answer, until P9: the last 31 days, a cap, what PUT /cap was sent.
const day = (n) => new Date(Date.now() - n * 86_400_000).toISOString().slice(0, 10);
const days = Array.from({ length: 31 }, (_, i) => {
  const d = 30 - i;
  const weekend = [0, 6].includes(new Date(`${day(d)}T12:00:00Z`).getUTCDay());
  return { day: day(d), active_users: weekend ? 2 + (d % 3) : 6 + ((d * 7) % 9), call_minutes: weekend ? 0 : (d * 13) % 40 };
});
const thisMonth = days.filter((d) => d.day.slice(0, 7) === day(0).slice(0, 7));
const stub = {
  months: [
    { month: "2026-09", active_users: 31, call_minutes: 412 },
    { month: day(0).slice(0, 7), active_users: 38, call_minutes: thisMonth.reduce((n, d) => n + d.call_minutes, 0) },
  ],
  days,
  cap: { active_users: null },
  this_month: { active_users: 38, call_minutes: thisMonth.reduce((n, d) => n + d.call_minutes, 0) },
};
const capsSent = [];

async function stubUsage(ctx) {
  await ctx.route("**/api/v1/apps/*/usage*", (route) => route.fulfill({ json: stub }));
  await ctx.route("**/api/v1/apps/*/cap", (route) => {
    const body = route.request().postDataJSON();
    capsSent.push({ method: route.request().method(), body, type: route.request().headers()["content-type"] });
    stub.cap = { active_users: body.active_users };
    return route.fulfill({ json: { cap: stub.cap } });
  });
}

/** Desktop and phone shots of a screen; at phone width, no sideways scroll. */
async function shots(page, name, { full = true } = {}) {
  await page.waitForTimeout(250);
  await page.screenshot({ path: join(OUT, `${name}-desktop.png`), fullPage: full });
  await page.setViewportSize(PHONE);
  await page.waitForTimeout(250);
  const wide = await page.evaluate(() => document.documentElement.scrollWidth - innerWidth);
  check(wide <= 0, `${name}: ${wide}px of sideways scroll at phone width`);
  await page.screenshot({ path: join(OUT, `${name}-phone.png`), fullPage: full });
  await page.setViewportSize(DESKTOP);
}

/** Opens Maya's page at `origin` with `key`; how it went. */
async function visit(browser, origin, key, room) {
  const ctx = await browser.newContext({ viewport: { width: 800, height: 500 } });
  const page = await ctx.newPage();
  page.on("pageerror", (e) => console.error(`page at ${origin}: ${e.message}`));
  const ws = RELAY.replace(/^http/, "ws") + "/socket";
  await page.goto(`${origin}/?${new URLSearchParams({ key, room, relay: ws, sdk: `${RELAY}/sdk/v0/presence.js` })}`);
  await page.locator('#socket:not([data-state="trying"])').waitFor();
  await page.locator('#status:not([data-live="connecting"])').waitFor({ timeout: 15_000 });
  const result = { socket: await page.locator("#socket").getAttribute("data-state"), live: await page.locator("#status").getAttribute("data-live"), text: await page.locator("#status").textContent() };
  return { ...result, page, ctx };
}

const browser = await chromium.launch();
let failed = false;
try {
  const ctx = await browser.newContext({ viewport: DESKTOP });
  await ctx.grantPermissions(["clipboard-read", "clipboard-write"], { origin: new URL(RELAY).origin });
  const page = await ctx.newPage();
  page.on("pageerror", (e) => console.error(`dashboard: page error: ${e.message}`));

  step("the dashboard, signed out");
  await page.goto(`${BASE}/`);
  await page.getByRole("heading", { name: "Sign in" }).waitFor();
  await shots(page, "1-sign-in");

  step(`Maya asks for a sign-in link (${EMAIL}) and follows it`);
  const from = statSync(LOG).size;
  await page.getByLabel("Your email").fill(EMAIL);
  await page.getByRole("button", { name: "Email me a link" }).click();
  await page.getByText("Check your email").waitFor();
  await page.goto(await linkFor(EMAIL, from));
  await page.getByRole("button", { name: "Sign in" }).click();
  await page.waitForURL(`${BASE}/`);
  await page.getByRole("heading", { name: "Make your first app" }).waitFor();
  await shots(page, "2-no-apps");

  step("makes an app; its secret shows once");
  await page.getByLabel("The app's name").fill("Sketchpad");
  await page.getByRole("button", { name: "Make the app" }).click();
  await page.waitForURL(/\/dashboard\/apps\/app_[0-9a-f]{16}$/);
  const appUrl = page.url();
  await page.getByText("Your app's server secret").waitFor();
  check((await page.getByLabel("The server secret").textContent()).length >= 40, "the secret is shown");
  await page.getByRole("heading", { name: "Keys" }).waitFor();
  await page.screenshot({ path: join(OUT, "3-new-app-desktop.png") });
  await page.getByRole("button", { name: "I've kept it" }).click();
  check(!(await page.getByText("Your app's server secret").isVisible()), "the secret goes away");

  step("usage, before the relay meters: a calm note, not an error");
  await page.getByText("Usage will appear here once people start joining your rooms.").waitFor();

  step("adds an origin: a wrong one first");
  await page.getByLabel("An origin").fill("app.localhost");
  await page.getByRole("button", { name: "Add the origin" }).click();
  await page.locator("#origins").getByRole("alert").getByText("an origin is http(s)://host[:port]").waitFor();
  await page.getByLabel("An origin").fill(`${SITE}/`);
  await page.getByRole("button", { name: "Add the origin" }).click();
  await page.locator("#origins li", { hasText: SITE }).waitFor();
  check(!(await page.locator("#origins").getByRole("alert").isVisible()), "the origin's error clears");

  step("makes a test key and copies it");
  await page.getByRole("button", { name: "New test key" }).click();
  await page.getByRole("button", { name: /^Copy test key/ }).click();
  await page.getByRole("button", { name: "Copied" }).waitFor();
  const testKey = await page.evaluate(() => navigator.clipboard.readText());
  check(/^pk_test_[A-Za-z0-9_-]{10,}$/.test(testKey), `copied a test key, not ${testKey}`);
  check((await page.locator(".key-row").count()) === 2, "a live key and a test key");

  step("sets an OpenID Connect provider: errors under their fields, then right");
  await page.getByRole("radio", { name: "OpenID Connect" }).click();
  await page.getByRole("button", { name: "Use OpenID Connect" }).click();
  const issuerField = page.locator("label.field", { hasText: "Issuer URL" });
  await issuerField.getByRole("alert").getByText("this provider needs its URL (issuer)").waitFor();
  await issuerField.locator("input").fill("http://id.sketchpad.example/realms/main");
  await page.getByRole("button", { name: "Use OpenID Connect" }).click();
  await issuerField.getByRole("alert").getByText("the issuer is https://").waitFor();
  await issuerField.locator("input").fill("https://id.sketchpad.example/realms/main");
  await page.getByRole("button", { name: "Use OpenID Connect" }).click();
  const audienceField = page.locator("label.field", { hasText: "Audience" });
  await audienceField.getByRole("alert").getByText("oidc needs an audience").waitFor();
  check(!(await issuerField.getByRole("alert").isVisible()), "the issuer's error clears");
  await page.locator("#sign-in").screenshot({ path: join(OUT, "4-provider-error-desktop.png") });
  await audienceField.locator("input").fill("sketchpad-web");
  await page.getByRole("button", { name: "Use OpenID Connect" }).click();
  await page.locator("#sign-in").getByRole("status").getByText("Saved.").waitFor();
  await page.locator("#sign-in .now", { hasText: "https://id.sketchpad.example/realms/main" }).waitFor();

  step("sets a room rule");
  await page.getByRole("radio", { name: "Room starts with it" }).click();
  await page.getByLabel("The claim").fill("team");
  await page.getByRole("button", { name: "Save the rule" }).click();
  await page.locator("#room-rule").getByRole("status").getByText("Saved.").waitFor();

  step("turns guests on");
  await page.getByRole("switch").click();
  await page.locator('[role="switch"][aria-checked="true"]').waitFor();

  step("the quickstart has her key and her provider");
  const snippet = await page.locator("#quickstart pre").textContent();
  check(snippet.includes(testKey) || snippet.includes("pk_live_"), "the snippet has a key of hers");
  await page.getByRole("radio", { name: "Test key" }).click();
  await page.getByRole("tab", { name: "JavaScript" }).click();
  const js = await page.locator("#quickstart pre").textContent();
  check(js.includes(`key: "${testKey}"`) && js.includes("userManager.getUser()") && js.includes(`relay: "ws://`), "the JS snippet: her test key, OIDC, this relay");

  step("everything she set is kept (a reload)");
  await page.reload();
  await page.locator("#origins li", { hasText: SITE }).waitFor();
  await page.locator('[role="switch"][aria-checked="true"]').waitFor();
  check((await page.getByLabel("The claim").inputValue()) === "team", "the room rule is kept");
  check((await page.locator("label.field", { hasText: "Audience" }).locator("input").inputValue()) === "sketchpad-web", "the audience is kept");
  check(!(await page.getByText("Your app's server secret").isVisible()), "the secret isn't shown again");
  await shots(page, "5-app");
  await page.locator("#quickstart").screenshot({ path: join(OUT, "6-quickstart-desktop.png") });
  await page.locator("#sign-in").screenshot({ path: join(OUT, "7-provider-desktop.png") });

  step("usage, on a stub of P9's answer; she sets a cap");
  await stubUsage(ctx);
  await page.reload();
  await page.getByText("Active users a day").waitFor();
  await page.locator(".tile", { hasText: "Active users this month" }).getByText("No cap set.").waitFor();
  await page.getByLabel("Cap on active users").fill("0");
  await page.getByRole("button", { name: "Set the cap" }).click();
  await page.locator("#usage").getByRole("alert").getByText("A cap is a whole number of people, 1 or more.").waitFor();
  await page.getByLabel("Cap on active users").fill("40");
  await page.getByRole("button", { name: "Set the cap" }).click();
  await page.locator("#usage").getByRole("status").getByText("Saved.").waitFor();
  await page.locator(".tile.meter.near", { hasText: "of 40" }).getByText("Near your cap").waitFor();
  check(capsSent.length === 1 && capsSent[0].method === "PUT" && capsSent[0].body.active_users === 40 && capsSent[0].type.startsWith("application/json"), `PUT /cap {active_users: 40}, as JSON: ${JSON.stringify(capsSent)}`);
  await page.locator("#usage svg").first().hover({ position: { x: 300, y: 40 } });
  await page.locator("#usage .read").first().getByText(/active users$/).waitFor();
  await page.locator("#usage").scrollIntoViewIfNeeded();
  await page.locator("#usage").screenshot({ path: join(OUT, "8-usage-desktop.png") });
  await page.setViewportSize(PHONE);
  await page.locator("#usage").screenshot({ path: join(OUT, "8-usage-phone.png") });
  await page.setViewportSize(DESKTOP);
  await ctx.unrouteAll();

  step("her page on her origin joins a room with the copied key, as a guest");
  const room = "team-a:standup";
  const ok = await visit(browser, SITE, testKey, room);
  check(ok.socket === "open" && ok.live === "on", `from ${SITE}: ${JSON.stringify({ ...ok, page: undefined, ctx: undefined })}`);
  await ok.page.screenshot({ path: join(OUT, "9-her-page.png") });
  await ok.ctx.close();

  step("the same page on another origin is refused");
  const no = await visit(browser, ELSEWHERE, testKey, room);
  check(no.socket === "refused" && no.live !== "on", `from ${ELSEWHERE}: ${JSON.stringify({ ...no, page: undefined, ctx: undefined })}`);
  await no.ctx.close();

  step("her apps list");
  await page.goto(`${BASE}/`);
  await page.locator(".app-card", { hasText: "Sketchpad" }).getByText("2 keys · 1 origin · guests welcome").waitFor();
  await shots(page, "10-apps");

  step("revoking the test key asks first, then it stops working");
  await page.goto(appUrl);
  await page.getByRole("button", { name: `Revoke ${testKey}` }).click();
  await page.getByRole("group", { name: /Revoke it\?/ }).getByRole("button", { name: "Revoke" }).click();
  await page.locator('.key-row[data-mode="test"]').waitFor({ state: "detached" });
  check((await page.locator(".key-row").count()) === 1, "one key left");
  const gone = await visit(browser, SITE, testKey, room);
  check(gone.socket === "refused", "a revoked key is refused");
  await gone.ctx.close();

  console.log(`ok: Maya set up ${appUrl} and joined from ${SITE}; screenshots in ${OUT}`);
} catch (e) {
  failed = true;
  console.error(`failed: ${e.stack ?? e}`);
} finally {
  await browser.close();
  site.close();
}
process.exit(failed ? 1 : 0);
