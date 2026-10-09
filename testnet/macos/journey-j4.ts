// J4 (#664): an agent installs Arugula. Given "install Arugula on this
// Mac and check that it runs", it reads the site's install page, runs what
// the page says, and then reads only what its commands print and what
// `arugulad --help` and `arugula --help` say. Each command it runs must be
// in what it has read so far: one that isn't is a guess, an unguided step,
// and fails the run. Over ssh in a fresh tart VM (testnet/macos/vm.sh), as
// an agent in a terminal would, with no person and no screen.
//
// The page is the live one, docs.arugula.io/install/: its text, and the
// commands its code blocks' Copy buttons carry. The install script is the
// released one at https://arugula.io/install.sh.
//
// The report is J4 in web/journey-reports/ (local, never committed).
//   node --experimental-strip-types testnet/macos/journey-j4.ts
//   (or: just macos journey-j4)
//   KEEP=1  leave the VM up
//
// Exit codes: 0 every command was one it had read, 1 not.

import { spawnSync } from "node:child_process";
import { join } from "node:path";
import { Journey, Stopped } from "../../web/e2e/journey/record.ts";
import { here, root } from "./journey-lib.ts";

process.env.JOURNEY_REPORT_DIR ??= join(root, "web/journey-reports");
const VM = process.env.ARUGULA_MACOS_VM ?? "arugula-j4";
const PAGE = "https://docs.arugula.io/install/";
const vm = (...args: string[]) => spawnSync(join(here, "vm.sh"), [args[0], VM, ...args.slice(1)], { encoding: "utf8" });

/** Everything the agent has read: the page, then what each command printed. */
let read = "";
const j = new Journey("J4", "an agent installs Arugula from what the site and --help give it", undefined, ["agent"]);

/** The command's own words in what was read, as the step's prompt; null when it isn't there (a guess). */
const found = (cmd: string) => async () => (read.includes(cmd) ? `"${cmd}", in what it read` : null);

/** Run `cmd` in the VM's shell, as the agent types it; what it printed is read. */
function run(cmd: string): { out: string; code: number } {
  j.typed(cmd);
  const r = vm("ssh", cmd);
  const out = `${r.stdout ?? ""}${r.stderr ?? ""}`;
  read += `\n${out}`;
  return { out, code: r.status ?? -1 };
}

const unescape = (t: string) =>
  t.replace(/&#x7f;/gi, "\n").replace(/&quot;/g, '"').replace(/&#39;|&#x27;/g, "'").replace(/&lt;/g, "<").replace(/&gt;/g, ">").replace(/&amp;/g, "&");

/** The page as an agent reads it: its text, and each code block's command. */
function pageText(html: string): { text: string; commands: string[] } {
  const commands = [...html.matchAll(/data-code="([^"]*)"/g)].map((m) => unescape(m[1]));
  const text = unescape(html.replace(/<(script|style)[\s\S]*?<\/\1>/g, "").replace(/<[^>]+>/g, " ")).replace(/\s+/g, " ");
  return { text, commands };
}

let ok = false;
try {
  vm("down");
  if (vm("up").status !== 0) throw new Error("the VM didn't come up");

  await j.step({ id: "task", title: "The agent is asked to install Arugula on this Mac and check that it runs", actor: "agent", surface: "terminal", prompt: null, own: "its task" }, async () => {});

  let install = "";
  await j.step({ id: "read-page", title: "It reads the site's install page", actor: "agent", surface: "browser", prompt: null, own: "the site, the first place an agent looks" }, async () => {
    const res = await fetch(PAGE);
    if (!res.ok) throw new Error(`${PAGE}: HTTP ${res.status}`);
    const page = pageText(await res.text());
    read = `${page.text}\n${page.commands.join("\n")}`;
    j.opened(PAGE);
    // The macOS tab comes first: the first command that runs install.sh.
    install = page.commands.find((c) => c.includes("install.sh") && c.endsWith("| sh")) ?? "";
    if (!install) throw new Error("the page has no install.sh command");
  });

  await j.step({ id: "install", title: "It runs the page's command for macOS", actor: "agent", surface: "terminal", prompt: found(install), expect: "the install command" }, async () => {
    const r = run(install);
    if (r.code !== 0) throw new Error(`exit ${r.code}: ${r.out.trim().split("\n").slice(-5).join("\n")}`);
  });

  // Where the CLI is: on the agent's PATH, or wherever what it read says.
  let cli = "arugula";
  await j.step({ id: "help", title: "It reads arugula --help, to find how to check", actor: "agent", surface: "terminal", prompt: found("arugula"), expect: "the CLI's name" }, async () => {
    let r = run("arugula --help");
    if (r.code === 127) {
      // Not on PATH: the install said where it put it, or the agent guesses.
      // The install prints its commands with the path when it isn't on PATH.
      const said = /(\S+\/arugula) (?:web|status)\b/.exec(read)?.[1];
      j.issue(`\`arugula\` isn't on the shell's PATH over ssh${said ? `; the install's summary runs it as ${said}` : ""}`);
      if (!said) throw new Error("`arugula` isn't on PATH, and nothing it read says where it is");
      cli = said;
      r = run(`${cli} --help`);
    }
    if (r.code !== 0) throw new Error(`exit ${r.code}: ${r.out.trim().split("\n").slice(-5).join("\n")}`);
  });

  // The install's own words, where it gave them (its Check line), else --help's.
  const check = /(\S*arugula status)\b/.exec(read)?.[1] ?? `${cli} status`;
  await j.step({ id: "status", title: "It checks with arugula status", actor: "agent", surface: "terminal", prompt: found(check), expect: "the status command, as it can run it" }, async () => {
    const r = run(check);
    if (r.code !== 0) throw new Error(`exit ${r.code}: ${r.out.trim()}`);
    if (!/daemon\s+.*(answer|running|\d+\.\d+\.\d+)/i.test(r.out)) throw new Error(`status says nothing of a running daemon:\n${r.out.trim()}`);
  });

  await j.finish();
  ok = true;
} catch (e) {
  console.error(e instanceof Stopped ? e.message : e);
  j.write();
} finally {
  if (!process.env.KEEP) vm("down");
}
process.exit(ok ? 0 : 1);
