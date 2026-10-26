// Labs flags from control (#347): an account and a team set flags on
// control, which sends them to the machines they're for; the machine's own
// Developer settings say where each comes from, and win. Taking the
// machine's word back follows control again.

import { spawn, type ChildProcess } from "node:child_process";
import { existsSync, mkdtempSync, readFileSync, rmSync } from "node:fs";
import type { AddressInfo } from "node:net";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { expect, test, type Page } from "@playwright/test";
import { Device } from "../fixtures/device.ts";
import { fakeGithub } from "../fixtures/fakes.ts";
import { ANY, controlPort, daemonPort } from "./ports";

let control = "";
let mini = "";
let state = "";
let team = "";
let me: Device;
const procs: ChildProcess[] = [];
const dirs: string[] = [];
let gh: ReturnType<typeof fakeGithub>;

test.describe.configure({ mode: "serial" });

function temp(what: string) {
  const d = mkdtempSync(join(tmpdir(), `arugula-e2e-labsflags-${what}-`));
  dirs.push(d);
  return d;
}

async function up(url: string) {
  for (let i = 0; i < 100; i++) {
    try {
      if ((await fetch(url)).status < 500) return;
    } catch {
      // not yet
    }
    await new Promise((r) => setTimeout(r, 100));
  }
  throw new Error(`${url} didn't come up`);
}

test.beforeAll(async () => {
  gh = fakeGithub(0);
  await new Promise((r) => gh.once("listening", r));
  const github = `http://127.0.0.1:${(gh.address() as AddressInfo).port}`;
  const db = join(temp("db"), "control.db");
  procs.push(
    spawn(
      "../target/debug/arugula-control",
      [
        ...["--listen", ANY, "--public-url", "http://127.0.0.1:0", "--db", db],
        ...["--github-client-id", "id", "--github-client-secret", "s"],
        ...["--github-url", github, "--github-api", github],
      ],
      { stdio: "ignore" },
    ),
  );
  control = `http://127.0.0.1:${await controlPort(db, procs.at(-1))}`;
  await up(`${control}/control.json`);

  // A team's machine, joined by code into the team, no flags of its own.
  me = await Device.signIn({ control, login: "jake" });
  team = await me.createTeam("Crew");
  state = temp("mini");
  const joining = spawn("../target/debug/arugulad", ["join", control, "--name", "mini", "--state-dir", state, "--account", me.keys.id], {
    stdio: ["ignore", "pipe", "ignore"],
  });
  procs.push(joining);
  const code = await new Promise<string>((res) => {
    let out = "";
    joining.stdout!.on("data", (d) => {
      out += d;
      const m = out.match(/#join=([A-Z0-9.-]+)\s/);
      if (m) res(m[1]);
    });
  });
  const exited = new Promise<number | null>((r) => joining.on("exit", r));
  await me.approveJoin(code, team);
  expect(await exited).toBe(0);
  const d = spawn(
    "../target/debug/arugulad",
    [
      ...["--listen", ANY, "--name", "mini", "--state-dir", state],
      ...["--shell", "bash --norc --noprofile", "--no-manager-env", "--tailscale-socket", "/nonexistent/sock", "--no-claude-ide"],
    ],
    { stdio: "ignore" },
  );
  procs.push(d);
  mini = `http://127.0.0.1:${await daemonPort(state, d)}`;
  await up(`${mini}/api/host`);
  // On control's relay: a flag set before it dials is nudged to nobody,
  // and the next look is a minute off.
  await me.waitOnline("mini");
});

test.afterAll(() => {
  for (const p of procs) p.kill("SIGKILL");
  gh?.close();
  for (const d of dirs) rmSync(d, { recursive: true, force: true });
});

const setOnControl = (name: string, on: boolean | null, forTeam?: string) => me.api("/api/flags", { name, on, team: forTeam ?? null });

const dialog = (page: Page) => page.getByRole("dialog", { name: "Developer settings" });
const row = (page: Page, flag: string) => dialog(page).locator("li", { has: page.locator(`[data-flag="${flag}"]`) });

/** The dialog again, as it is now: control's word reaches the machine on
 * its next refresh, which a change nudges. */
async function reopen(page: Page) {
  await page.goto(`${mini}/#developer`);
  await expect(dialog(page)).toBeVisible();
}

test("a team's and an account's flags reach the machine, each saying where it comes from", async ({ page }) => {
  expect(existsSync(join(state, "flags.json"))).toBe(false);
  // No machine of the account has Developer settings: the page offers none.
  expect((await me.api<{ dev: boolean }>("/api/flags")).dev).toBe(false);
  await setOnControl("chat", true, team);
  await setOnControl("workspaces", true, team);
  await setOnControl("workspaces", false);
  // Each set nudges the machine: the file may come before the last of them.
  const fromControl = () => {
    const f = join(state, "flags.control.json");
    return existsSync(f) ? JSON.parse(readFileSync(f, "utf8")) : null;
  };
  await expect.poll(fromControl, { timeout: 20_000 }).toEqual({
    account: { workspaces: false },
    team: { chat: true, workspaces: true },
  });

  await reopen(page);
  await expect(row(page, "chat").locator("input")).toBeChecked();
  await expect(row(page, "chat").locator("[data-flag-source]")).toHaveText("Set for your team on control");
  // The account's word beats the team's.
  await expect(row(page, "workspaces").locator("input")).not.toBeChecked();
  await expect(row(page, "workspaces").locator("[data-flag-source]")).toHaveText("Set for your account on control");
  await expect(row(page, "vms").locator("[data-flag-source]")).toHaveCount(0);
  // The agent catalog is the machine's alone: control won't take it, and
  // the dialog says so. Nor guest ssh, a sharing setting now (native #205).
  await expect(setOnControl("agents", true, team)).rejects.toThrow(/400/);
  await expect(setOnControl("guest-ssh", true, team)).rejects.toThrow(/400/);
  await expect(row(page, "agents").locator("[data-flag-local-only]")).toBeVisible();
  await expect(row(page, "guest-ssh")).toHaveCount(0);
  await expect(row(page, "chat").locator("[data-flag-local-only]")).toHaveCount(0);
  // Control saying something is the machine asking: the menus offer them.
  await expect.poll(() => page.evaluate(() => window.__arugula.client.hasDev())).toBe(true);
  await expect.poll(() => page.evaluate(() => window.__arugula.client.flag("chat"))).toBe(true);
  // And it tells control so on its next refresh (a change nudges one), so
  // control's page now offers the flags.
  await setOnControl("studio", true);
  await expect.poll(async () => (await me.api<{ dev: boolean }>("/api/flags")).dev, { timeout: 20_000 }).toBe(true);
  await setOnControl("studio", null);
});

test("the machine overrides control, and Use control's takes its word back", async ({ page }) => {
  await reopen(page);
  await row(page, "chat").locator("input").uncheck();
  await expect(row(page, "chat").locator("[data-flag-source]")).toHaveText("Set on this machine; control has it on for your team");
  expect(JSON.parse(readFileSync(join(state, "flags.json"), "utf8"))).toEqual({ flags: { chat: false } });
  await expect.poll(() => page.evaluate(() => window.__arugula.client.flag("chat"))).toBe(false);

  await row(page, "chat").getByRole("button", { name: "Use control's for Chat" }).click();
  await expect(row(page, "chat").locator("input")).toBeChecked();
  await expect(row(page, "chat").locator("[data-flag-source]")).toHaveText("Set for your team on control");
  await expect(row(page, "chat").getByRole("button")).toHaveCount(0);
  expect(JSON.parse(readFileSync(join(state, "flags.json"), "utf8"))).toEqual({ flags: {} });
});

test("unset on control, the machine is back to the flag's default", async ({ page }) => {
  await setOnControl("chat", null, team);
  await expect.poll(() => JSON.parse(readFileSync(join(state, "flags.control.json"), "utf8")).team, { timeout: 20_000 }).toEqual({ workspaces: true });
  await reopen(page);
  await expect(row(page, "chat").locator("input")).not.toBeChecked();
  await expect(row(page, "chat").locator("[data-flag-source]")).toHaveCount(0);
});
