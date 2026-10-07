// J2a on a real Mac (#551): web/e2e/journey-j2a.spec.ts's linking up,
// with Sam on the real app in a fresh tart VM (J1 there first, as the
// report J2a-mac-setup-sam) and Riley a friend on another computer, in a
// browser here (Chrome, through Playwright), signed up on control's own
// page (J2a-mac-setup-riley). Sam shares a session with Riley; Riley
// accepts, finds Sam's Mac, asks to drive, Sam allows it, Riley takes
// control and types, and it shows in Sam's terminal on Sam's Mac.
//
// Sam's steps press what the Mac's screen names (System Events) and Riley's
// what the page shows (roles and text); the report is J2a-mac in
// web/journey-reports/, with each person's screen at their steps. Gaps
// already filed on #551 are drawn, not failed on, as in the spec.
//
//   node --experimental-strip-types testnet/macos/journey-j2a.ts
//   (or: just macos journey-j2a)
//   KEEP=1  leave the VM up

import { createRequire } from "node:module";
import { join } from "node:path";
import type { Browser, Locator, Page } from "@playwright/test";
import { Journey, Stopped, type Surface } from "../../web/e2e/journey/record.ts";
import { APP, FP, byHelp, capture, firstRunMac, press, pressHelp, read, root, shot, shows, sleep, startMac, stopMac, typeInto, until } from "./journey-lib.ts";

const { chromium } = createRequire(join(root, "web/package.json"))("@playwright/test") as typeof import("@playwright/test");
const asPerson = () => Object.defineProperty(Navigator.prototype, "webdriver", { get: () => false });

let ok = false;
let browser: Browser | undefined;
const j = new Journey("J2a-mac", "two friends link up: Sam on the real Mac app, Riley in a browser elsewhere", undefined, ["sam", "riley"]);
j.slowMs = 30_000;

/** The text on Riley's screen that reads like `re`. */
const onPage = async (p: Page, re: RegExp) => ((await p.locator("body").innerText()).match(new RegExp(`[^\\n]*(${re.source})[^\\n]*`, "i"))?.[0] ?? "").trim();
/** What the terminal Riley is looking at shows (canvas: the page's hook). */
const riley_screen = (p: Page) => p.evaluate(() => (window as unknown as { __arugula?: { client: { active(): number }; text(n: number): string } }).__arugula?.text((window as unknown as { __arugula: { client: { active(): number } } }).__arugula.client.active()) ?? "");

try {
  const { control, github, version } = await startMac("sam");
  console.log(`J2a-mac: the app ${version}, control ${control}`);

  // Sam's first run on the Mac, its own report.
  const samSetup = new Journey("J2a-mac-setup-sam", "Sam's first run (J1, on the real Mac), before J2a-mac", undefined, ["sam"]);
  try {
    await firstRunMac(samSetup, "sam", control);
  } finally {
    samSetup.write();
  }
  const machine = byHelp(APP, "Hosts").replace(/\s*(relayed|direct)?\s*▾?$/, "").trim();

  // Riley, in a browser on another computer: an account on control's page.
  browser = await chromium.launch();
  const ctx = await browser.newContext({ viewport: { width: 1100, height: 720 } });
  await ctx.addInitScript(asPerson);
  await ctx.addCookies([{ name: "gh_user", value: "riley", url: github }]);
  const riley = await ctx.newPage();
  const rileySetup = new Journey("J2a-mac-setup-riley", "Riley signs up in a browser, before J2a-mac", undefined, ["riley"]);
  try {
    await rileySetup.step({ id: "open", title: "Riley opens Arugula cloud", actor: "riley", surface: "browser", page: riley, prompt: null, own: "Riley opens the cloud's page" }, async () => {
      await riley.goto(control);
    });
    await rileySetup.step({ id: "sign-up", title: "Riley signs up with GitHub", actor: "riley", surface: "browser", page: riley, prompt: riley.getByRole("link", { name: "Sign in with GitHub" }) }, async () => {
      await riley.getByRole("link", { name: "Sign in with GitHub" }).click();
      await riley.getByRole("button", { name: /Authorize/ }).click();
    });
    await rileySetup.step({ id: "recovery-codes", title: "Riley keeps the recovery codes", actor: "riley", surface: "browser", page: riley, prompt: riley.getByRole("heading", { name: "Your recovery codes" }) }, async () => {
      await riley.getByLabel("I've stored these somewhere safe").check();
      await riley.getByRole("button", { name: "Continue" }).click();
      await riley.getByRole("heading", { name: "Add a machine" }).waitFor();
    });
  } finally {
    rileySetup.write();
  }

  const sam = (id: string, title: string, surface: Surface, prompt: (() => Promise<string | null>) | null, act: () => Promise<void>, wanted?: string) =>
    j.step({ id, title, actor: "sam", surface, prompt, shot, expect: wanted }, act);
  const ri = (id: string, title: string, prompt: Locator | null, act: () => Promise<void>, wanted?: string) =>
    j.step({ id, title, actor: "riley", surface: "browser", page: riley, prompt, expect: wanted }, act);
  const marker = "hi-from-riley-$((6*7))";
  let fpShown = "";
  let rileyFp = "";

  await j.step({ id: "agree", title: "Riley tells Sam their Arugula login", actor: "riley", surface: "message", prompt: null, own: "the friends agree to link up; Riley sends their login (riley)" }, async () => {});

  await sam("open-share", "Sam opens Share session…", "app", shows(APP, /^Share$/), async () => {
    pressHelp(APP, "Sessions");
    await sleep(800);
    press(APP, "AXMenuItem|AXButton", /^Share session/);
    j.issue("Share is inside the session menu; nothing on the main screen says Share");
  }, "a Share button on the main screen");

  await sam("share-who", "Sam enters Riley's login, as someone who drives", "app", shows(APP, /^Who$/), async () => {
    typeInto(APP, "AXTextField", /^Who$/, "riley");
    // The role: a select; typing its first letter picks "drives".
    typeInto(APP, "AXPopUpButton", /^Role$|watches/, "d");
    press(APP, "AXButton", /^Share$/);
    fpShown = await until("Riley's first device", () => read(APP, FP, /first device is/) || null);
  });

  await sam("ask-fingerprint", "Sam asks Riley for their first device's fingerprint", "message", shows(APP, /check it with them/), async () => {});

  await ri("find-fingerprint", "Riley finds their own fingerprint", riley.getByText(/fingerprint/i), async () => {
    await riley.getByRole("button", { name: "Devices and machines…" }).click();
    rileyFp = (/[0-9a-f]{4}(?:-[0-9a-f]{4}){3,}/.exec(await riley.getByText(/account/i).filter({ hasText: /[0-9a-f]{4}-[0-9a-f]{4}/ }).first().innerText())?.[0] ?? "").toLowerCase();
    const all = await riley.getByText(/[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}/).allInnerTexts();
    if (all.length > 1) j.issue(`the panel shows ${all.length} fingerprints; which one Sam means ("first device") isn't said`);
    await riley.getByRole("button", { name: "Done" }).click();
  });

  await j.step({ id: "send-fingerprint", title: "Riley sends Sam the fingerprint", actor: "riley", surface: "message", prompt: null, own: "a message to Sam, as Sam asked" }, async () => {});

  await sam("confirm-share", "Sam checks the fingerprint and shares", "app", shows(APP, /^Share with riley/), async () => {
    j.compared(`Riley's first device ${fpShown} (Sam's Mac) = ${rileyFp} (Riley's browser)`);
    if (fpShown !== rileyFp) j.issue(`the fingerprint Sam is shown (${fpShown}) isn't the one Riley found (${rileyFp})`);
    press(APP, "AXButton", /^Share with riley/);
    await sleep(1500);
    press(APP, "AXButton", /^Done$/);
  });

  await ri("accept", "Riley accepts the shared session", riley.getByRole("heading", { name: "A shared session" }), async () => {
    await riley.getByRole("button", { name: "Accept" }).click();
  });

  await ri("find-machine", "Riley finds Sam's Mac", riley.getByText(machine), async () => {
    await riley.getByTitle("Hosts").click();
    await riley.getByRole("menuitem", { name: new RegExp(machine.replace(/[.]/g, "\\.")) }).click();
    await until("Sam's terminal on Riley's screen", async () => ((await riley_screen(riley)).includes("hello-42") ? true : null), 30_000);
  }, `anything naming Sam's Mac (${machine}) outside the host menu`);

  await ri("try-typing", "Riley clicks into Sam's terminal and types", riley.getByTitle("Hosts"), async () => {
    const v = riley.viewportSize()!;
    await riley.mouse.click(v.width / 2, v.height / 2);
    await riley.keyboard.type("echo first-try\n", { delay: 5 });
    await sleep(1500);
    if (!/first-try/.test(capture())) {
      const said = await onPage(riley, /can't type|trust you|watching/);
      j.issue(said ? `typing did nothing; the screen said: "${said}"` : "typing did nothing, and nothing on screen said why");
    }
  });

  await ri("ask-to-drive", "Riley asks Sam to let them drive", riley.getByText(/trust you with it|Ask the owner to let me drive/i), async () => {
    const v = riley.viewportSize()!;
    await riley.mouse.click(v.width / 2, v.height / 2, { button: "right" });
    await riley.getByRole("menuitem", { name: "Ask the owner to let me drive it" }).click();
  });

  await sam("allow", "Sam allows Riley to drive", "app", shows(APP, /asks to drive/), async () => {
    press(APP, "AXButton", /^Allow$/);
  });

  await ri("riley-types", "Riley, allowed now, types in Sam's terminal", riley.getByTitle("Hosts"), async () => {
    const v = riley.viewportSize()!;
    await riley.mouse.click(v.width / 2, v.height / 2);
    await riley.keyboard.type("echo second-try\n", { delay: 5 });
    await sleep(1500);
    if (!/second-try/.test(capture())) {
      const said = await onPage(riley, /control|driv/);
      j.issue(said ? `allowed, typing still did nothing: Sam has control of the pane; the screen said: "${said}"` : "allowed, typing still did nothing: Sam has control of the pane, and nothing on screen says so");
    }
  });

  await ri("take-control", "Riley takes control of the pane", riley.getByText(/take control|ask for control/i), async () => {
    const v = riley.viewportSize()!;
    await riley.mouse.click(v.width / 2, v.height / 2, { button: "right" });
    await riley.getByRole("menuitem", { name: /Take control/ }).click();
  });

  await ri("riley-drives", "Riley types in Sam's terminal", riley.getByTitle("Hosts"), async () => {
    const v = riley.viewportSize()!;
    await riley.mouse.click(v.width / 2, v.height / 2);
    j.typed(`echo ${marker}`);
    await riley.keyboard.type(`echo ${marker}\n`, { delay: 5 });
    await until("Riley's line on Riley's screen", async () => ((await riley_screen(riley)).includes("hi-from-riley-42") ? true : null), 15_000);
  });

  await sam("sam-sees", "Sam sees what Riley typed, on his Mac", "app", shows(APP, /relayed|direct/), async () => {
    await until("Riley's line in Sam's terminal", () => (/hi-from-riley-42/.test(capture()) ? true : null), 15_000);
  });

  await j.finish({
    // Filed on #551, as in web/e2e/journey-j2a.spec.ts.
    "open-share": "#551: Share is only in the session menu",
    "find-fingerprint": "#551: nothing says where your first device's fingerprint is",
    // (find-machine isn't here: Riley has no machine of their own, so after
    // Accept the window shows Sam's.)
    "take-control": "#551: allowed to drive, but the owner has control, and nothing says Take control",
  });
  ok = true;
} catch (e) {
  console.error(e instanceof Stopped ? e.message : e);
  j.write();
} finally {
  await browser?.close().catch(() => {});
  stopMac();
}
process.exit(ok ? 0 : 1);
