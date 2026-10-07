// J1 on a real Mac (#551): web/e2e/journey-j1.spec.ts's path, with the
// real app from its release zip in a fresh tart VM (testnet/macos/vm.sh),
// its own launch agent and daemon, and the real Safari it opens links in.
// Nothing is emulated but GitHub. Every step acts the way a person does,
// through what the screen shows: System Events presses buttons and links
// by their visible names in the app's window and in Safari, and reads
// their text. Only the terminal's text is read another way (`arugula
// capture` in the VM: the canvas has none).
//
// Control and the fake GitHub run here, reached from the VM through ssh
// tunnels at the same loopback addresses. The daemon gets this control as
// ARUGULA_CONTROL in the app's launch agent (the bundle signed again, ad
// hoc as released), and the run stops before Connect if the app would go
// anywhere else: it never touches the hosted control.
//
// The report is J1-mac in web/journey-reports/ (local, never committed):
// the same graph as J1's, with the Mac's screen at each step.
//
//   node --experimental-strip-types testnet/macos/journey-j1.ts
//   (or: just macos journey)
//   ARUGULA_MACOS_APP_ZIP=URL|path  another app (default: app-latest, what a new user downloads)
//   KEEP=1                          leave the VM up
//
// Exit codes: 0 the journey held (every step led by the screen, or known),
// 1 it didn't.

import { execFileSync, spawn, type ChildProcess } from "node:child_process";
import { mkdtempSync, readFileSync, rmSync } from "node:fs";
import type { AddressInfo } from "node:net";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { fakeGithub } from "../../web/e2e/journey/github.ts";
import { Journey, Stopped } from "../../web/e2e/journey/record.ts";

const here = dirname(fileURLToPath(import.meta.url));
const root = join(here, "../..");
const target = process.env.ARUGULA_MACOS_BIN ?? join(root, "target/debug");
const vm = process.env.ARUGULA_MACOS_VM ?? "arugula-journey";
const zip = process.env.ARUGULA_MACOS_APP_ZIP ?? "https://github.com/arugula-salad/arugula/releases/download/app-latest/arugula-desktop-macos-arm64.zip";
process.env.JOURNEY_REPORT_DIR ??= join(root, "web/journey-reports");

const procs: ChildProcess[] = [];
const dirs: string[] = [];
const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));
const temp = (w: string) => {
  const d = mkdtempSync(join(tmpdir(), `arugula-journey-mac-${w}-`));
  dirs.push(d);
  return d;
};
const v = (...args: string[]) => execFileSync(join(here, "vm.sh"), [args[0], vm, ...args.slice(1)], { encoding: "utf8" }).trim();
const sh = (cmd: string) => v("ssh", cmd);

/** System Events in the VM, on `proc`'s front window: `all()` is every
 * element in it, `find(role, name)` the first whose role is one of
 * `role` (|-separated) and whose name (or value, or title) matches the
 * regular expression `name`. */
function ax(proc: string, body: string): string {
  return execFileSync(join(here, "vm.sh"), ["ssh", vm, "osascript", "-l", "JavaScript", "-"], {
    input: `const se = Application("System Events");
const proc = se.processes.byName(${JSON.stringify(proc)});
const g = (e, k) => { try { const v = e[k](); return v == null ? "" : String(v); } catch (_) { return ""; } };
const label = (e) => g(e, "name") || g(e, "value") || g(e, "description") || g(e, "title");
const all = () => proc.windows[0].entireContents();
const find = (role, name) => { const re = new RegExp(name, "i"); return all().find((e) => role.split("|").includes(g(e, "role")) && re.test(label(e))); };
const press = (e) => { try { e.actions.byName("AXPress").perform(); } catch (_) { e.click(); } };
${body}`,
    encoding: "utf8",
    timeout: 120_000,
  }).trim();
}

const APP = "arugula-desktop";
const SAFARI = "Safari";
/** Every text on `proc`'s front window, one per line. */
const screen = (proc: string) => ax(proc, `all().map(label).filter((t) => t).join("\\n")`);
/** The visible text matching `re` on `proc`'s window, or null. */
const shows = (proc: string, re: RegExp) => async () => {
  // Names as the page writes them, whatever case CSS shows them in.
  const i = new RegExp(re.source, "i");
  return screen(proc).split("\n").find((t) => i.test(t)) ?? null;
};
/** Press the element `role` named like `name` on `proc`'s window. */
function press(proc: string, role: string, name: RegExp) {
  const r = ax(proc, `const e = find(${JSON.stringify(role)}, ${JSON.stringify(name.source)}); if (!e) "missing"; else { press(e); "ok" }`);
  if (r !== "ok") throw new Error(`no ${role} "${name.source}" on ${proc}'s window`);
}
async function until<T>(what: string, f: () => T | null | undefined | Promise<T | null | undefined>, ms = 30_000): Promise<T> {
  const end = Date.now() + ms;
  for (;;) {
    const x = await (async () => f())().catch(() => null);
    if (x) return x;
    if (Date.now() > end) throw new Error(`timed out waiting for ${what}`);
    await sleep(700);
  }
}
/** The Mac's screen, as a person sees it. */
async function shot(): Promise<Buffer | undefined> {
  const b64 = sh("screencapture -x -t jpg /tmp/journey.jpg && sips -Z 1200 /tmp/journey.jpg >/dev/null && base64 -i /tmp/journey.jpg").replace(/\s/g, "");
  return b64 ? Buffer.from(b64, "base64") : undefined;
}
/** What the terminal shows (read through the CLI the app put in ~/.local/bin). */
const capture = () => {
  try {
    return sh("~/.local/bin/arugula capture %1 2>/dev/null");
  } catch {
    return "";
  }
};
const CODE = /\b[A-Z0-9]{4,6}-[A-Z0-9]{4,6}\b/;
const FP = /\b[0-9a-f]{4}(?:[-\s][0-9a-f]{4}){3,}\b/;
/** The first `re` on `proc`'s window (after the line matching `after`),
 * lowercased: CSS may show it in capitals on one screen and not the other. */
const read = (proc: string, re: RegExp, after?: RegExp) => {
  const lines = screen(proc).split("\n");
  const from = after ? lines.findIndex((l) => new RegExp(after.source, "i").test(l)) : 0;
  if (from < 0) return "";
  // What the label labels: on its line or the few after it.
  return lines.slice(from, after ? from + 4 : undefined).join("\n").match(new RegExp(re.source, "i"))?.[0]?.replace(/\s/g, "-").toLowerCase() ?? "";
};

let ok = false;
const j = new Journey("J1-mac", "one person, unregistered to working (the real Mac app in a fresh VM, Safari)", undefined, ["newcomer"]);
// Each step's time here includes the accessibility reads that find and
// check things (seconds each): only a long wait is the person's.
j.slowMs = 30_000;
const step = (id: string, title: string, surface: "app" | "browser" | "terminal", prompt: (() => Promise<string | null>) | null, act: () => Promise<void>, opts: { expect?: string; own?: string } = {}) =>
  j.step({ id, title, actor: "newcomer", surface, prompt, shot, ...opts }, act);

try {
  // Control and the fake GitHub, here, and at the same addresses in the VM.
  const gh = fakeGithub("newcomer");
  await new Promise<void>((r) => gh.listen(0, "127.0.0.1", r));
  const ghPort = (gh.address() as AddressInfo).port;
  const github = `http://127.0.0.1:${ghPort}`;
  const db = join(temp("control"), "control.db");
  procs.push(
    spawn(`${target}/arugula-control`, [
      ...["--listen", "127.0.0.1:0", "--public-url", "http://127.0.0.1:0", "--db", db, "--static-dir", join(root, "web/dist")],
      ...["--github-client-id", "id", "--github-client-secret", "s", "--github-url", github, "--github-api", github],
    ], { stdio: "ignore" }),
  );
  const port = await until("control's port", () => {
    try {
      return Number(readFileSync(join(dirname(db), "listen"), "utf8").trim().split(":").pop());
    } catch {
      return 0;
    }
  });
  const control = `http://127.0.0.1:${port}`;
  await until("control", () => fetch(`${control}/control.json`).then((r) => r.ok));

  v("down");
  v("up");
  const ssh = v("sshcmd");
  procs.push(spawn("sh", ["-c", `exec ${ssh} -N -o ExitOnForwardFailure=yes -R 127.0.0.1:${port}:127.0.0.1:${port} -R 127.0.0.1:${ghPort}:127.0.0.1:${ghPort}`], { stdio: "ignore" }));
  await until("the tunnels", () => (sh(`curl -sf -o /dev/null ${control}/control.json && echo up`) === "up" ? true : null));
  sh(`set -e; case '${zip}' in http*) curl -sSL -o /tmp/app.zip '${zip}' ;; esac; ditto -x -k /tmp/app.zip /Applications`);
  // Screenshots over ssh: macOS asks once whether sshd may record the
  // screen; allow it before the journey, and check it's gone.
  sh("screencapture -x /tmp/first.png || true");
  await until(
    "the screen-recording question answered",
    () => {
      const r = ax("UserNotificationCenter", `const b = proc.windows.length ? all().find((e) => g(e, "role") === "AXButton" && g(e, "name") === "Allow") : null; if (b) { press(b); "pressed" } else "none"`);
      return r === "none" ? true : null;
    },
    30_000,
  ).catch(() => {});
  // This run's control for the daemon from its first start: one variable
  // in the app's launch agent (the app itself takes ARUGULA_CONTROL as
  // "joined", so it can't go in the session), and the bundle signed again,
  // ad hoc as the release is.
  const agent = "/Applications/Arugula.app/Contents/Library/LaunchAgents/io.arugula.desktop.daemon.plist";
  sh(
    `set -e; plutil -replace EnvironmentVariables.ARUGULA_CONTROL -string ${control} ${agent}; codesign --force --deep --sign - --preserve-metadata=entitlements,flags /Applications/Arugula.app 2>/dev/null`,
  );
  const version = sh("defaults read /Applications/Arugula.app/Contents/Info CFBundleShortVersionString");
  console.log(`J1-mac: the app ${version}, control ${control}`);

  await step("app-opens", "Open the app for the first time", "app", null, async () => {
    sh(`open -a /Applications/Arugula.app`);
    await until("the app's window", () => (ax(APP, `proc.windows.length`) !== "0" ? true : null), 60_000);
    // Its page, drawn: what the person waits for.
    await until("the app's page", () => (/Terminal input|Getting started|SETUP/.test(screen(APP)) ? true : null), 90_000);
    if (/App Background Activity/.test(screen("NotificationCenter").replace(/\n/g, " ")))
      j.issue("macOS shows \"App Background Activity: Arugula can run in the background\" on the first start");
  }, { own: "opens the app they installed" });

  await step("getting-started", "Getting started greets you", "app", shows(APP, /^TERMINALS THAT OUTLIVE/), async () => {});

  await step("set-it-up", "Start setting up", "app", shows(APP, /Each takes a click/), async () => press(APP, "AXButton", /^Set it up/));

  await step("skip-phone", "Skip the phone for now", "app", shows(APP, /^Skip for now/), async () => press(APP, "AXButton", /^Skip for now/));

  let machineCode = "";
  await step("connect", "Connect this machine to the cloud", "app", shows(APP, /^Connect to /), async () => {
    const button = await shows(APP, /^Connect to /)();
    // Never the hosted control.
    if (!button?.includes(new URL(control).host))
      throw new Error(`the app would connect to "${button}", not this run's control: stopping. It shows: ${screen(APP).split("\n").filter((t) => /connect|cloud/i.test(t)).join(" / ")}`);
    press(APP, "AXButton", /^Connect to /);
    await until("the machine's code", () => (machineCode = read(APP, CODE, /Approve this code/)) || null);
  });

  await step("open-approval", "Open the approval link", "app", shows(APP, /^Approve in Arugula cloud/), async () => {
    press(APP, "AXLink|AXButton", /^Approve in Arugula cloud/);
    const url = await until("Safari on control's page", () => {
      const u = sh(`osascript -e 'tell application "Safari" to get URL of front document'`);
      return u.startsWith(control) ? u : null;
    }, 60_000);
    j.opened(url);
  });

  await step("sign-up", "Make an account from the approval link", "browser", shows(SAFARI, /^Sign in to approve this machine/), async () => press(SAFARI, "AXLink|AXButton", /^Sign in with GitHub/));

  await step("github-authorize", "Authorize Arugula on GitHub", "browser", shows(SAFARI, /^Authorize arugula-salad/), async () =>
    press(SAFARI, "AXLink|AXButton", /^Authorize arugula-salad/),
  );

  await step("recovery-codes", "Keep the recovery codes", "browser", shows(SAFARI, /^Your recovery codes/), async () => {
    press(SAFARI, "AXCheckBox", /stored these somewhere safe/);
    press(SAFARI, "AXButton", /^Continue$/);
  });

  let account = "";
  await step("approve-machine", "Approve the machine, checking its code", "browser", shows(SAFARI, /^Add a machine\?/), async () => {
    const shown = read(SAFARI, CODE, /asks to join/);
    j.compared(`machine code ${machineCode} (app) = ${shown} (Safari)`);
    if (shown !== machineCode) throw new Error(`Safari shows ${shown}, the app ${machineCode}`);
    account = read(SAFARI, FP, /^Your account:/);
    if (!account) {
      const lines = screen(SAFARI).split("\n");
      const near = lines.flatMap((l, i) => (new RegExp(FP.source, "i").test(l) || /your account/i.test(l) ? [`${lines[i - 1]} >> ${l}`] : []));
      throw new Error(`no account fingerprint in Safari; fingerprints and "your account" on it: ${JSON.stringify(near)}`);
    }
    press(SAFARI, "AXButton", /^Approve/);
  });

  await step("confirm-account", "Back in the app, check the account's fingerprint", "app", shows(APP, /Is this your account\?/), async () => {
    const shown = read(APP, FP, /Is this your account/);
    j.compared(`account ${account} (Safari) = ${shown} (app)`);
    if (shown !== account) throw new Error(`the app shows ${shown}, Safari ${account}`);
    press(APP, "AXButton", /^They match$/);
  });

  await step("joined", "The app says the machine is in", "app", shows(APP, /^Joined to your account|THIS WINDOW, EVERY MACHINE/i), async () => {});

  let appCode = "";
  await step("app-signin", "Sign the app in, so its window reaches the account", "app", shows(APP, /^Sign the app in/), async () => {
    press(APP, "AXButton", /^Sign the app in/);
    await until("the app's code", () => (appCode = read(APP, /\b[A-Z0-9]{4}-[A-Z0-9]{4}\b/, /Check your browser shows/)) || null);
  });

  await step("allow-app", "Allow the app in Safari, checking its code", "browser", shows(SAFARI, /^Sign in the app\?/), async () => {
    const shows = screen(SAFARI).toLowerCase().includes(appCode);
    j.compared(`app sign-in code ${appCode} (app) shown in Safari: ${shows}`);
    if (!shows) throw new Error(`Safari doesn't show ${appCode}`);
    press(SAFARI, "AXButton", /^Allow$/);
  });

  await step("approve-app", "Approve the app as a device, checking its fingerprint", "browser", shows(SAFARI, /^New device\?/), async () => {
    const appFp = await until("the app's fingerprint", () => read(APP, FP, /fingerprint/) || null);
    const shown = read(SAFARI, FP, /shows this fingerprint/);
    j.compared(`app device ${appFp} (app) = ${shown} (Safari)`);
    if (shown !== appFp) throw new Error(`Safari shows ${shown}, the app ${appFp}`);
    press(SAFARI, "AXButton", /^Approve$/);
  });

  await step("terminal", "Run a command in a terminal on the machine", "app", shows(APP, /relayed|direct/), async () => {
    await until("a terminal in the app", () => (/Terminal input/.test(screen(APP)) ? true : null), 60_000);
    ax(APP, `proc.frontmost = true; const t = find("AXTextArea", "Terminal input"); t.focused = true; delay(0.5); se.keystroke(" "); delay(0.3); se.keystroke("echo hello-$((6*7))"); se.keyCode(36);`);
    j.typed("echo hello-$((6*7))");
    await until("hello-42 in the terminal", () => (/^hello-42$/m.test(capture()) ? true : null), 20_000);
  });

  await step("restart", "Quit and reopen the app: still signed in, the terminal still there", "app", null, async () => {
    sh(`osascript -e 'tell application "Arugula" to quit'`);
    await sleep(3000);
    sh(`open -a /Applications/Arugula.app`);
    await until("the app's window, signed in", () => (/relayed|direct/.test(screen(APP)) ? true : null), 60_000);
    if (!/^hello-42$/m.test(capture())) throw new Error("the terminal lost its output");
  }, { own: "quits and reopens the app" });

  await j.finish();
  ok = true;
} catch (e) {
  if (!(e instanceof Stopped)) console.error(e);
  else console.error(e.message);
  j.write();
} finally {
  for (const p of procs) p.kill("SIGKILL");
  for (const d of dirs) rmSync(d, { recursive: true, force: true });
  if (!process.env.KEEP) v("down");
}
process.exit(ok ? 0 : 1);
