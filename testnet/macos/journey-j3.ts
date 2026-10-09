// J3 (#664): someone installs the Mac app by hand, from the site's steps
// alone, on a Mac that already runs an older daemon (arugulad 0.26.1:
// #661). They download the app as the site says,
// move it to Applications and open it; macOS refuses it
// ("Arugula" Not Opened), and they follow the site: Done, then Open
// Anyway in System Settings › Privacy & Security, Open Anyway again in
// macOS's second dialog (Open "Arugula"?) and their password. The app
// opens, says the daemon is older, and offers the update; they take it,
// and the daemon updates itself, with their pane still running.
//
// A fresh tart VM, with Gatekeeper on (the base image turns it off) and
// the app from app-latest marked as Safari marks a download. Every step
// acts through System Events on what the screen names; each step's prompt
// is the site's words for it (docs.arugula.io/install/) or the window's.
//
// The report is J3 in web/journey-reports/ (local, never committed).
//   node --experimental-strip-types testnet/macos/journey-j3.ts
//   (or: testnet/macos/test.sh journey-j3)
//   ARUGULA_MACOS_APP_ZIP=URL|path  another app (default: app-latest)
//   KEEP=1                          leave the VM up
//
// Exit codes: 0 the journey held (every step led), 1 it didn't.

import { join } from "node:path";
import { Journey, Stopped } from "../../web/e2e/journey/record.ts";

process.env.ARUGULA_MACOS_VM ??= "arugula-j3";
const lib = await import("./journey-lib.ts");
const { APP, ax, root, sh, shot, shows, until, v, zip } = lib;
process.env.JOURNEY_REPORT_DIR ??= join(root, "web/journey-reports");

/** The older daemon: the oldest whose own update finds the renamed
 * repository's releases (#525). */
const OLD = "0.26.1";
/** `a` is a newer `X.Y.Z` than `b`. */
const newerThan = (a: string, b: string) => {
  const n = (v: string) => v.split(/[-+]/)[0].split(".").map(Number);
  const [x, y] = [n(a), n(b)];
  for (let i = 0; i < 3; i++) if ((x[i] ?? 0) !== (y[i] ?? 0)) return (x[i] ?? 0) > (y[i] ?? 0);
  return false;
};
const PAGE = "https://docs.arugula.io/install/";
const GK = "CoreServicesUIAgent";
const SETTINGS = "System Settings";

/** The site's install page, as text: what the person follows. */
async function siteSays(): Promise<string> {
  const html = await (await fetch(PAGE)).text();
  return html
    .replace(/<(script|style)[\s\S]*?<\/\1>/g, "")
    .replace(/<[^>]+>/g, " ")
    .replace(/&#x([0-9a-f]+);/gi, (_, h) => String.fromCodePoint(parseInt(h, 16)))
    .replace(/&#(\d+);/g, (_, d) => String.fromCodePoint(Number(d)))
    .replace(/&quot;/g, '"')
    .replace(/&amp;/g, "&")
    .replace(/\s+/g, " ");
}
/** The site's sentence with `words`, as a step's prompt. */
const site = (page: string, words: RegExp) => async () => page.match(new RegExp(`[^.]*${words.source}[^.]*\\.?`, "i"))?.[0]?.trim() ?? null;

/** A window's button by its visible name, pressed. */
function pressIn(proc: string, name: string) {
  const r = ax(proc, `const e = all().find((e) => g(e, "role") === "AXButton" && (g(e, "name") === ${JSON.stringify(name)} || g(e, "title") === ${JSON.stringify(name)})); if (!e) "missing"; else { press(e); "ok" }`);
  if (r !== "ok") throw new Error(`no button "${name}" on ${proc}'s window`);
}
const keys = (script: string) => sh(`osascript -e ${JSON.stringify(`tell application "System Events" to ${script}`)}`);
/** What the daemon on 7681 says it is, with the local token. */
const daemonVersion = () => {
  try {
    return sh(
      `curl -s -m 5 -H "Authorization: Bearer $(cat ~/.local/state/arugula/local-token)" http://127.0.0.1:7681/api/host | python3 -c 'import json,sys; print(json.load(sys.stdin)["version"])'`,
    );
  } catch {
    return "";
  }
};

const j = new Journey("J3", "installs the Mac app by hand from the site, over an older daemon", undefined, ["person"]);
j.slowMs = 30_000;
const step = (id: string, title: string, surface: "app" | "browser" | "terminal", prompt: (() => Promise<string | null>) | null, act: () => Promise<void>, opts: { expect?: string; own?: string } = {}) =>
  j.step({ id, title, actor: "person", surface, prompt, shot, ...opts }, act);

let ok = false;
try {
  // --- The Mac as this person has it: arugulad 0.26.1 running, with a pane.
  v("down");
  v("up");
  const tgz = `arugula-${OLD}-aarch64-apple-darwin.tar.gz`;
  sh(`set -e; mkdir -p /tmp/old; cd /tmp/old; curl -fsSL -o ${tgz} https://github.com/arugula-salad/arugula/releases/download/v${OLD}/${tgz}; tar -xzf ${tgz} --strip-components 1; ./arugulad install >/dev/null`);
  await until(`arugulad ${OLD} answering`, () => (daemonVersion() === OLD ? true : null), 60_000);
  sh(`~/.local/bin/arugula run --session work "i=0; while :; do i=\\$((i+1)); echo \\$i > /tmp/count; sleep 0.2; done" >/dev/null`);
  const pane = () => sh("pgrep -f '[>] /tmp/count; sleep' | head -1 || true");
  await until("the counting pane", () => pane() || null, 20_000);
  const pane0 = pane();
  // Screenshots over ssh: allow the screen-recording question once.
  sh("screencapture -x /tmp/first.png || true");
  await until("the screen-recording question answered", () => (ax("UserNotificationCenter", `const b = proc.windows.length ? all().find((e) => g(e, "role") === "AXButton" && g(e, "name") === "Allow") : null; if (b) { press(b); "pressed" } else "none"`) === "none" ? true : null), 30_000).catch(() => {});
  // Gatekeeper on, as on a person's Mac (the base image turns it off).
  sh("echo admin | sudo -S spctl --master-enable 2>/dev/null; spctl --status");

  const page = await siteSays();

  await step("download", "Downloads the Mac app from the site", "browser", site(page, /arugula-desktop-macos-arm64/), async () => {
    j.opened(PAGE);
    if (!/^https?:/.test(zip)) v("push", zip, "/tmp/app.zip");
    // Saved as a browser saves it: marked as downloaded, which is what Gatekeeper checks.
    sh(`set -e; cd ~/Downloads; case '${zip}' in http*) curl -fsSL -o arugula-desktop-macos-arm64.zip '${zip}' ;; *) cp /tmp/app.zip arugula-desktop-macos-arm64.zip ;; esac; xattr -w com.apple.quarantine "0083;$(printf %x $(date +%s));Safari;$(uuidgen)" arugula-desktop-macos-arm64.zip; ditto -x -k arugula-desktop-macos-arm64.zip .; xattr -p com.apple.quarantine Arugula.app >/dev/null`);
  }, { expect: "the download's name on the site" });

  await step("move", "Moves Arugula to Applications and opens it", "app", site(page, /Move \*?Arugula\*? to your Applications folder/), async () => {
    // Finder can't be scripted over ssh here (its Apple events wait on a
    // consent nobody can give), so the move is mv: macOS then runs the app
    // translocated, as for an app moved in Terminal. The steps are the same.
    sh("mv ~/Downloads/Arugula.app /Applications/");
    sh("(open /Applications/Arugula.app >/dev/null 2>&1 &)");
  }, { expect: "the site's step to move it and open it" });

  await step("refused", "macOS refuses it; they choose Done, as the site says", "app", shows(GK, /Not Opened/), async () => {
    pressIn(GK, "Done");
  }, { expect: "macOS's \"Not Opened\" dialog" });

  await step("privacy", "Opens System Settings › Privacy & Security", "app", site(page, /Privacy (&|and) Security/), async () => {
    sh(`open "x-apple.systempreferences:com.apple.settings.PrivacySecurity.extension"`);
    await until("Privacy & Security", () => shows(SETTINGS, /was blocked/)(), 30_000);
  }, { expect: "the site's step to open Privacy & Security" });

  await step("open-anyway", "Chooses Open Anyway beside \"Arugula\" was blocked", "app", shows(SETTINGS, /“Arugula” was blocked/), async () => {
    // Its accessible name is the sentence beside it; the button says Open Anyway.
    const r = ax(SETTINGS, `const e = all().find((e) => g(e, "role") === "AXButton" && /was blocked/.test(g(e, "name"))); if (!e) "missing"; else { press(e); "ok" }`);
    if (r !== "ok") throw new Error("no Open Anyway in Privacy & Security");
  }, { expect: "Open Anyway in Privacy & Security" });

  await step("open-anyway-again", "macOS asks once more; Open Anyway", "app", shows(GK, /Open “Arugula”\?/), async () => {
    pressIn(GK, "Open Anyway");
  }, { expect: "the dialog Open \"Arugula\"?" });

  await step("password", "Enters their password", "app", async () => {
    // coreautha's window (its second: the first is its Passwords… button).
    const said = sh(
      `osascript -l JavaScript -e 'const p = Application("System Events").processes.byName("coreautha"); p.windows().flatMap((w) => w.entireContents().map((e) => { try { return String(e.name() || e.value() || "") } catch (_) { return "" } })).find((t) => /administrator/.test(t)) || ""' 2>/dev/null || true`,
    );
    return said || null;
  }, async () => {
    keys(`keystroke "admin"`);
    keys("key code 36");
    await until("the app's window", () => (ax(APP, "proc.windows.length") !== "0" ? true : null), 60_000);
  }, { expect: "the password prompt" });

  await step("offered", "The app says the daemon is older and offers the update", "app", shows(APP, /arugulad 0\.26\.1, older than/), async () => {
    if (daemonVersion() !== OLD) throw new Error(`the daemon changed before anyone asked: ${daemonVersion()}`);
  }, { expect: "the app saying the daemon is older" });

  await step("update", "Takes the update: Update arugulad", "app", shows(APP, /^Update arugulad$/), async () => {
    keys(`set frontmost of process "${APP}" to true`);
    keys("key code 36");
    await until(`an arugulad newer than ${OLD} answering`, () => (newerThan(daemonVersion(), OLD) ? true : null), 180_000);
    if (sh("launchctl print gui/$(id -u)/arugulad >/dev/null 2>&1 && echo loaded || true") !== "loaded") throw new Error("the arugulad agent isn't loaded");
    if (pane() !== pane0) j.issue(`the pane was ${pane0}, now ${pane() || "gone"}`);
  }, { expect: "the update button" });

  await j.finish();
  ok = true;
} catch (e) {
  console.error(e instanceof Stopped ? e.message : e);
  j.write();
} finally {
  if (!process.env.KEEP) v("down");
}
process.exit(ok ? 0 : 1);
