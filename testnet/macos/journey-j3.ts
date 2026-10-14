// J3 (#664): someone installs the Mac app by hand, from the site's steps
// alone, on a Mac that already runs an older daemon (arugulad 0.26.1:
// #661). They download the app as the site says (the .dmg, the site's
// main download, or the .zip), put it in Applications and open it. The
// app is Developer ID signed and notarized (#177), so macOS asks only its
// usual question for a download ("Arugula" is an app downloaded from the
// Internet) and they choose Open: no "Not Opened", no Open Anyway in
// Privacy & Security, no password, and no silent hang at launch (#315).
// The app opens, says the daemon is older, and offers the update; they
// take it, and the daemon updates itself, with their pane still running.
//
// A fresh tart VM, with Gatekeeper on (the base image turns it off) and
// the app from app-latest marked as Safari marks a download. Every step
// acts through System Events on what the screen names; each step's prompt
// is the site's words for it (docs.arugula.io/install/) or the window's.
//
// The report is J3 in web/journey-reports/ (local, never committed).
//   node --experimental-strip-types testnet/macos/journey-j3.ts
//   (or: testnet/macos/test.sh journey-j3)
//   ARUGULA_J3_FROM=dmg|zip         which download (default: dmg)
//   ARUGULA_MACOS_APP_ZIP=URL|path  another app, a .dmg or .zip as FROM
//                                   says (default: app-latest's)
//   KEEP=1                          leave the VM up
//
// Exit codes: 0 the journey held (every step led), 1 it didn't.

import { join } from "node:path";
import { Journey, Stopped } from "../../web/e2e/journey/record.ts";

process.env.ARUGULA_MACOS_VM ??= "arugula-j3";
const lib = await import("./journey-lib.ts");
const { APP, ax, root, screen, sh, shot, shows, sleep, until, v } = lib;
process.env.JOURNEY_REPORT_DIR ??= join(root, "web/journey-reports");

const FROM = process.env.ARUGULA_J3_FROM === "zip" ? "zip" : "dmg";
const FILE = `arugula-desktop-macos-arm64.${FROM}`;
const src = process.env.ARUGULA_MACOS_APP_ZIP ?? `https://github.com/arugula-salad/arugula/releases/download/app-latest/${FILE}`;

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
      `curl -s -m 5 -H "Authorization: Bearer $(cat ~/.local/state/arugula/local-token)" http://127.0.0.1:7681/api/host | python3 -c 'import json,sys; print(json.load(sys.stdin)["version"])' 2>/dev/null`,
    );
  } catch {
    return "";
  }
};
/** Every text on Gatekeeper's window, or "" when it has none. */
const gatekeeper = () => {
  try {
    return ax(GK, `proc.windows.length ? all().map(label).filter((t) => t).join("\\n") : ""`);
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
  console.log(`macOS ${sh("sw_vers -productVersion")} (${sh("sw_vers -buildVersion")}); the ${FROM} from ${src}`);
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
  console.log(sh("echo admin | sudo -S spctl --master-enable 2>/dev/null; spctl --status"));

  const page = await siteSays();

  await step("download", `Downloads the Mac app from the site (the .${FROM})`, "browser", site(page, /arugula-desktop-macos-arm64/), async () => {
    j.opened(PAGE);
    if (!/^https?:/.test(src)) v("push", src, `/tmp/${FILE}`);
    // Saved as a browser saves it: marked as downloaded, which is what Gatekeeper checks.
    sh(`set -e; cd ~/Downloads; case '${src}' in http*) curl -fsSL -o ${FILE} '${src}' ;; *) cp /tmp/${FILE} ${FILE} ;; esac; xattr -w com.apple.quarantine "0083;$(printf %x $(date +%s));Safari;$(uuidgen)" ${FILE}`);
    if (FROM === "zip") sh(`set -e; cd ~/Downloads; ditto -x -k ${FILE} .; xattr -p com.apple.quarantine Arugula.app >/dev/null`);
  }, { expect: "the download's name on the site" });

  await step("move", "Moves Arugula to Applications and opens it", "app", site(page, /Move \*?Arugula\*? to your Applications folder/), async () => {
    // Finder can't be scripted over ssh here (its Apple events wait on a
    // consent nobody can give), so the app goes to Applications with cp
    // from the mounted .dmg (keeping its quarantine, as a Finder drag
    // does) or mv from the unzipped .zip: macOS then runs it translocated,
    // as an app moved in Terminal. The steps are the same.
    if (FROM === "dmg") sh(`set -e; hdiutil attach -quiet -nobrowse -mountpoint /tmp/arugula-dmg ~/Downloads/${FILE}; cp -R /tmp/arugula-dmg/Arugula.app /Applications/; hdiutil detach -quiet /tmp/arugula-dmg`);
    else sh("mv ~/Downloads/Arugula.app /Applications/");
    if (!sh("xattr -p com.apple.quarantine /Applications/Arugula.app 2>/dev/null || true")) throw new Error("the app in Applications isn't marked as downloaded");
    console.log(`spctl: ${sh("spctl -a -vv /Applications/Arugula.app 2>&1 || true").replace(/\s+/g, " ")}`);
    sh("(open /Applications/Arugula.app >/dev/null 2>&1 &)");
  }, { expect: "the site's step to move it and open it" });

  // What macOS shows for a notarized download, word for word. "Not
  // Opened" (anything that wants Open Anyway) is what this journey is for.
  const asked = await until("macOS's question", () => gatekeeper() || null, 60_000).catch(() => "");
  console.log(`macOS shows (${GK}):\n${asked || "(nothing)"}`);
  // The dialog itself (a step's screenshot is after its act), left in the
  // VM for KEEP=1 runs to fetch.
  sh("screencapture -x /tmp/j3-gatekeeper.png || true");
  if (/Not Opened|Open Anyway|Move to Trash|was blocked/i.test(asked)) throw new Error(`macOS refused the notarized app: ${asked.replace(/\n/g, " / ")}`);

  await step("confirm", "macOS asks about an app downloaded from the Internet; they choose Open", "app", shows(GK, /downloaded from the Internet|Open “Arugula”\?/), async () => {
    pressIn(GK, "Open");
    // #315 was a launch that never drew a window.
    await until("the app's window", () => (ax(APP, "proc.windows.length") !== "0" ? true : null), 60_000);
    const at = sh(`ps -axo command= | grep -m1 '[A]rugula.app/Contents/MacOS' || true`);
    console.log(`the app runs from: ${at}`);
    await sleep(3000);
    const again = gatekeeper();
    if (again) throw new Error(`macOS asked again: ${again.replace(/\n/g, " / ")}`);
  }, { expect: "macOS's question for a download" });

  await step("offered", "The app says the daemon is older and offers the update", "app", shows(APP, /arugulad (here is )?0\.26\.1, older than/), async () => {
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
