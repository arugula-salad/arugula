// `just dev-browser [options] URL [STEPS]`: a headless Chrome on a page
// (dev-control's, usually), driven by a few plain commands, for a PR's
// screenshots of screens a person would have to click to. Not a test: no
// assertions beyond "that step worked", and nothing kept in the repo.
//
// Commands come one per line from STEPS (a file), or from stdin: a
// heredoc, or typed at the prompt.
//
//   goto PATH|URL          open it (a path is on URL's origin)
//   click TEXT|SELECTOR    a button, link or text by what it says, or a
//                          selector (starts with [ # . css= text= role=)
//   fill SELECTOR VALUE    type VALUE into the field (replacing what's there)
//   type TEXT              type at the focus;  press KEY  (Enter, Tab…)
//   check SELECTOR         tick a checkbox
//   wait TEXT|SELECTOR|MS  until it shows, or MS milliseconds
//   shot FILE [full]       screenshot the viewport (or the full page)
//   text                   print the page's visible text
//   url                    print where the page is
//   eval JS                run JS in the page; print the result
//   passkeys               the virtual authenticator's credentials
//   quit
//
// Options:
//   --passkey        a virtual platform authenticator (CTAP2, resident keys,
//                    user verification, PRF), as Touch ID would be
//   --login NAME     the fake GitHub signs this browser in as NAME
//   --profile DIR    keep cookies, storage (the browser's device keys) and
//                    the passkeys between runs (a passkey brought back
//                    signs in, but without PRF); a fresh browser without it
//   --size WxH       viewport [1280x800]; --scale N  device pixel ratio [1]
//   --headed         show the window
//
// Passkeys need a host name: run dev-control with DEV_CONTROL_HOST=localhost.
// Sam's first sign-in (a new account shows its recovery codes first), then
// a passkey:
//
//   just dev-browser --passkey --login sam --profile /tmp/sam http://localhost:7690 <<'EOF'
//   click Sign in with GitHub
//   check [data-stored-codes]
//   click [data-saved-codes]
//   click sam
//   click Sign-in and account…
//   click Add a passkey
//   wait [data-passkey-name]
//   shot /tmp/sam-passkey.png
//   EOF
//
// --login sets a cookie the fake GitHub reads (fixtures/fakes.ts), so it
// works only against dev-control's fake GitHub, which is on 127.0.0.1.
//
// Exits non-zero at the first step that fails (from a file or a pipe).

import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { createInterface } from "node:readline";
import { chromium, type BrowserContext, type CDPSession, type Locator, type Page } from "@playwright/test";

const usage = "usage: dev-browser [--passkey] [--login NAME] [--profile DIR] [--size WxH] [--scale N] [--headed] URL [STEPS]";
const args = process.argv.slice(2);
const opt: { passkey: boolean; login?: string; profile?: string; size: string; scale: number; headed: boolean } = {
  passkey: false,
  size: "1280x800",
  scale: 1,
  headed: false,
};
const rest: string[] = [];
for (let i = 0; i < args.length; i++) {
  const a = args[i];
  const val = () => args[++i] ?? die(`${a} needs a value`);
  if (a === "--passkey") opt.passkey = true;
  else if (a === "--login") opt.login = val();
  else if (a === "--profile") opt.profile = resolve(val());
  else if (a === "--size") opt.size = val();
  else if (a === "--scale") opt.scale = Number(val());
  else if (a === "--headed") opt.headed = true;
  else if (a === "-h" || a === "--help") die(usage, 0);
  else if (a.startsWith("--")) die(`unknown option ${a}\n${usage}`);
  else rest.push(a);
}
const [start, stepsFile] = rest;
if (!start) die(usage);
const [width, height] = opt.size.split("x").map(Number);

function die(msg: string, code = 1): never {
  (code ? console.error : console.log)(msg);
  process.exit(code);
}

// The system Chrome, as the e2e specs use; Playwright's own if there's none.
const launch = { headless: !opt.headed, channel: "chrome" };
const viewport = { width, height };
let ctx: BrowserContext;
try {
  ctx = await open(launch);
} catch {
  ctx = await open({ ...launch, channel: undefined });
}
async function open(l: { headless: boolean; channel?: string }): Promise<BrowserContext> {
  if (opt.profile) {
    mkdirSync(opt.profile, { recursive: true });
    return chromium.launchPersistentContext(join(opt.profile, "chrome"), { ...l, viewport, deviceScaleFactor: opt.scale });
  }
  const b = await chromium.launch(l);
  return b.newContext({ viewport, deviceScaleFactor: opt.scale });
}
const page: Page = ctx.pages()[0] ?? (await ctx.newPage());

// --login: a cookie the fake GitHub (dev-control's, on 127.0.0.1) signs in
// by, picker or not. A session cookie: gone when the browser closes.
if (opt.login)
  await ctx.addCookies([{ name: "fake_github_login", value: opt.login, domain: "127.0.0.1", path: "/login/oauth" }]);

// --passkey: Chrome's virtual authenticator, kept in --profile between runs
// (it lives only as long as the browser).
let cdp: CDPSession | undefined;
let authenticatorId = "";
const passkeyFile = opt.profile && join(opt.profile, "passkeys.json");
if (opt.passkey) {
  cdp = await ctx.newCDPSession(page);
  await cdp.send("WebAuthn.enable", { enableUI: false });
  ({ authenticatorId } = await cdp.send("WebAuthn.addVirtualAuthenticator", {
    options: {
      protocol: "ctap2",
      ctap2Version: "ctap2_1",
      transport: "internal",
      hasResidentKey: true,
      hasUserVerification: true,
      isUserVerified: true,
      hasPrf: true,
      automaticPresenceSimulation: true,
    },
  }));
  if (passkeyFile && existsSync(passkeyFile)) {
    const kept = JSON.parse(readFileSync(passkeyFile, "utf8"));
    for (const credential of kept) await cdp.send("WebAuthn.addCredential", { authenticatorId, credential });
    // Chrome doesn't hand out a credential's PRF secret, so one brought
    // back signs in but answers no PRF: make PRF passkeys in the run that
    // uses them.
    if (kept.length) console.log(`dev-browser: ${kept.length} passkey(s) from ${passkeyFile} (they sign in, without PRF)`);
  }
}
async function credentials() {
  if (!cdp) throw new Error("no authenticator: start with --passkey");
  return (await cdp.send("WebAuthn.getCredentials", { authenticatorId })).credentials;
}
async function close() {
  if (cdp && passkeyFile) writeFileSync(passkeyFile, JSON.stringify(await credentials(), null, 1));
  await ctx.close();
}

const origin = new URL(start).origin;
const absolute = (u: string) => (/^[a-z]+:/.test(u) ? u : new URL(u, origin).href);
const isSelector = (s: string) => /^([\[#.]|css=|text=|role=|xpath=|data-)/.test(s);
const split = (s: string) => {
  const at = s.search(/\s/);
  return at < 0 ? [s, ""] : [s.slice(0, at), s.slice(at).trim()];
};

/** What says TEXT, preferring a button or link, then a label, then any
 * text: the first one that's on the page. */
async function find(what: string, timeout = 10_000): Promise<Locator> {
  if (isSelector(what)) return page.locator(what).first();
  const name = { name: what, exact: true };
  const tries = [
    page.getByRole("button", name),
    page.getByRole("link", name),
    page.getByRole("menuitem", name),
    page.getByRole("tab", name),
    page.getByLabel(what, { exact: true }),
    page.getByText(what, { exact: true }),
    page.getByRole("button", { name: what }),
    page.getByRole("link", { name: what }),
    page.getByText(what),
  ];
  const until = Date.now() + timeout;
  do {
    for (const l of tries) if (await l.first().isVisible().catch(() => false)) return l.first();
    await page.waitForTimeout(200);
  } while (Date.now() < until);
  throw new Error(`nothing on the page says "${what}"`);
}

async function step(line: string): Promise<boolean> {
  const [cmd, arg] = split(line.trim());
  switch (cmd) {
    case "":
    case "#":
      break;
    case "goto":
      await page.goto(absolute(arg || start));
      break;
    case "click":
      await (await find(arg)).click();
      break;
    case "fill": {
      const [sel, value] = split(arg);
      await (await find(sel)).fill(value);
      break;
    }
    case "check":
      await (await find(arg)).check();
      break;
    case "type":
      await page.keyboard.type(arg);
      break;
    case "press":
      await page.keyboard.press(arg);
      break;
    case "wait":
      if (/^\d+$/.test(arg)) await page.waitForTimeout(Number(arg));
      else await find(arg, 30_000);
      break;
    case "shot": {
      const [file, full] = split(arg);
      if (!file) throw new Error("shot FILE");
      mkdirSync(dirname(resolve(file)), { recursive: true });
      await page.screenshot({ path: file, fullPage: full === "full" });
      console.log(`shot ${resolve(file)}`);
      break;
    }
    case "text":
      console.log(await page.locator("body").innerText());
      break;
    case "url":
      console.log(page.url());
      break;
    case "eval":
      // Bytes (a PRF result, say) as hex, which JSON would print as {}.
      console.log(
        await page.evaluate(`(async () => JSON.stringify(await (${arg}), (k, v) =>
          v instanceof ArrayBuffer || ArrayBuffer.isView(v) ? [...new Uint8Array(v.buffer ?? v)].map((b) => b.toString(16).padStart(2, "0")).join("") : v, 1))()`),
      );
      break;
    case "passkeys":
      for (const c of await credentials()) console.log(`${c.rpId} ${c.userName ?? ""} resident=${c.isResidentCredential} signs=${c.signCount}`);
      break;
    case "quit":
    case "exit":
      return false;
    default:
      if (!cmd.startsWith("#")) throw new Error(`unknown command "${cmd}"`);
  }
  return true;
}

await page.goto(start);
const interactive = !stepsFile && process.stdin.isTTY;
const input = createInterface({ input: stepsFile ? (await import("node:fs")).createReadStream(stepsFile) : process.stdin, terminal: interactive });
if (interactive) {
  console.log(`dev-browser: ${start}. Commands: goto click fill check type press wait shot text url eval passkeys quit`);
  input.setPrompt("> ");
  input.prompt();
}
let code = 0;
for await (const line of input) {
  try {
    if (!(await step(line))) break;
  } catch (e) {
    console.error(`${line.trim()}: ${e instanceof Error ? e.message.split("\n")[0] : e}`);
    if (!interactive) {
      code = 1;
      break;
    }
  }
  if (interactive) input.prompt();
}
input.close();
await close();
process.exit(code);
