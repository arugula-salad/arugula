// The real Mac for the journeys (#551, #664): a fresh tart VM with the
// released app, and what a person does there, through System Events:
// press what the screen names, read what it says. J3 (journey-j3.ts) and
// J4 (journey-j4.ts) use it. J1 and J2a on the Mac started a control here
// (startMac, firstRunMac): they left with control for its own repository.
// The journey's record and report are web/e2e/journey's.

import { execFileSync } from "node:child_process";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

export const here = dirname(fileURLToPath(import.meta.url));
export const root = join(here, "../..");
export const vm = process.env.ARUGULA_MACOS_VM ?? "arugula-journey";
process.env.JOURNEY_REPORT_DIR ??= join(root, "web/journey-reports");

export const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));
export const v = (...args: string[]) => execFileSync(join(here, "vm.sh"), [args[0], vm, ...args.slice(1)], { encoding: "utf8" }).trim();
export const sh = (cmd: string) => v("ssh", cmd);

/** System Events in the VM, on `proc`'s front window: `all()` is every
 * element in it, `find(role, name)` the first whose role is one of
 * `role` (|-separated) and whose name (or value, or title) matches the
 * regular expression `name`. */
export function ax(proc: string, body: string): string {
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

export const APP = "arugula-desktop";
/** Every text on `proc`'s front window, one per line. */
export const screen = (proc: string) => ax(proc, `all().map(label).filter((t) => t).join("\\n")`);
/** The visible text matching `re` on `proc`'s window, or null. */
export const shows = (proc: string, re: RegExp) => async () => {
  // Names as the page writes them, whatever case CSS shows them in.
  const i = new RegExp(re.source, "i");
  return (
    screen(proc)
      .split("\n")
      .find((t) => i.test(t)) ?? null
  );
};
export async function until<T>(what: string, f: () => T | null | undefined | Promise<T | null | undefined>, ms = 30_000): Promise<T> {
  const end = Date.now() + ms;
  for (;;) {
    const x = await (async () => f())().catch(() => null);
    if (x) return x;
    if (Date.now() > end) throw new Error(`timed out waiting for ${what}`);
    await sleep(700);
  }
}
/** The Mac's screen, as a person sees it. */
export async function shot(): Promise<Buffer | undefined> {
  const b64 = sh("screencapture -x -t jpg /tmp/journey.jpg && sips -Z 1200 /tmp/journey.jpg >/dev/null && base64 -i /tmp/journey.jpg").replace(/\s/g, "");
  return b64 ? Buffer.from(b64, "base64") : undefined;
}
