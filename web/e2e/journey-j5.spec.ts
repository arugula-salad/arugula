// J5 (#664): someone who won't read docs. Dana opens Arugula and only
// clicks what it shows: Getting started from its first screen (no phone
// yet, no account), the one click that sets up Claude Code, Start an
// agent, a task, the agent's question answered, its work shown. Each step
// names the prompt on screen that led to it; a step only the docs would
// lead to is unguided and fails the run. Rules as in journey-j1.spec.ts.
//
// Claude Code's adapter is the e2e stand-in (playwright.config.ts: the
// scripted fake ACP server, as claude-agent-acp), so nothing here costs
// anything. No control: Dana takes none.
//   just journey j5

import { spawn, type ChildProcess } from "node:child_process";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { expect, test, type Locator, type Page } from "@playwright/test";
import { Journey } from "./journey/record";
import { asPerson, BIN } from "./journey/world";
import { closeContexts } from "./helpers";
import { ANY, daemonPort } from "./ports";

test.afterAll(closeContexts);
test.use({ actionTimeout: 20_000 });

let daemon: ChildProcess;
let dir: string;
let base = "";

test.beforeAll(async () => {
  dir = mkdtempSync(join(tmpdir(), "arugula-journey-j5-"));
  daemon = spawn(
    `${BIN}/arugulad`,
    [
      ...["--listen", ANY, "--name", "danamac", "--state-dir", join(dir, "state")],
      ...["--shell", "bash --norc --noprofile", "--no-manager-env", "--tailscale-socket", "/nonexistent/sock"],
    ],
    { stdio: "ignore", env: { ...process.env, HOME: dir, ARUGULA_CLAUDE_IDE_DIR: join(dir, "ide"), CLAUDE_CONFIG_DIR: join(dir, "claude") } },
  );
  base = `http://127.0.0.1:${await daemonPort(join(dir, "state"), daemon)}`;
});

test.afterAll(() => {
  daemon?.kill("SIGKILL");
  rmSync(dir, { recursive: true, force: true, maxRetries: 5, retryDelay: 200 });
});

test("J5: no docs: from the first screen to an agent that asked and did its work", async ({ browser }, info) => {
  test.setTimeout(240_000);
  const ctx = await browser.newContext({ viewport: { width: 1100, height: 720 } });
  await ctx.addInitScript(asPerson);
  const p = await ctx.newPage();
  const j = new Journey("J5", "won't read docs: Getting started to a working agent, by what the app shows", info, ["dana"]);
  const start = () => p.getByRole("dialog", { name: "Getting started" });
  const step = (id: string, title: string, prompt: () => Locator | null, act: () => Promise<void>, wanted?: string) =>
    j.step({ id, title, actor: "dana", surface: "browser", page: p, prompt: prompt(), expect: wanted }, act);

  try {
    await j.step({ id: "open", title: "Dana opens Arugula", actor: "dana", surface: "browser", page: p, prompt: null, own: "the app, just installed" }, async () => {
      await p.goto(base);
    });

    await step(
      "getting-started",
      "Getting started greets her",
      () => start().getByRole("heading").first(),
      async () => {
        await expect(start()).toBeVisible({ timeout: 20_000 });
      },
    );

    await step(
      "set-it-up",
      "She starts setting up",
      () => start().getByRole("button", { name: /Set it up/ }),
      async () => {
        await start()
          .getByRole("button", { name: /Set it up/ })
          .click();
      },
    );

    await step(
      "skip-phone",
      "No phone yet: she skips it",
      () => start().getByRole("button", { name: /Skip for now/ }),
      async () => {
        await start()
          .getByRole("button", { name: /Skip for now/ })
          .click();
        await expect(start().getByRole("heading", { name: /to your account or team/ })).toBeVisible();
      },
    );

    await step(
      "skip-cloud",
      "No account: she skips the cloud",
      () => start().getByRole("button", { name: /Skip for now/ }),
      async () => {
        await start()
          .getByRole("button", { name: /Skip for now/ })
          .click();
        await expect(start().getByRole("heading", { name: "Put agents to work" })).toBeVisible();
      },
    );

    await step(
      "use-claude",
      "She sets up Claude Code with one click",
      () => start().getByRole("button", { name: "Use Claude Code with Arugula" }),
      async () => {
        await start().getByRole("button", { name: "Use Claude Code with Arugula" }).click();
        await expect(start().getByText("Claude Code has Arugula's tools")).toBeVisible({ timeout: 30_000 });
      },
    );

    await step(
      "start-agent",
      "She starts an agent",
      () => start().getByRole("button", { name: "Start an agent…" }),
      async () => {
        await start().getByRole("button", { name: "Start an agent…" }).click();
        await expect(p.getByRole("dialog", { name: "Start an agent" })).toBeVisible();
      },
    );

    await step(
      "task",
      "She gives Claude Code a task",
      () => p.getByRole("dialog", { name: "Start an agent" }).getByPlaceholder("What should it do?"),
      async () => {
        const d = p.getByRole("dialog", { name: "Start an agent" });
        await d.getByPlaceholder("What should it do?").fill("run ls");
        await d.getByRole("button", { name: "Start" }).click();
        await expect(d).toBeHidden();
      },
      "a box for what the agent should do",
    );

    await step(
      "approve",
      "The agent asks to run ls; she allows it",
      () => p.getByRole("alertdialog", { name: /Allow ls/ }),
      async () => {
        await p.getByRole("alertdialog", { name: /Allow ls/ }).getByRole("button", { name: "Approve" }).click();
      },
      "the agent's question, with a way to answer it",
    );

    await step(
      "done",
      "She sees what it did",
      () => p.getByText("Ran it."),
      async () => {
        await expect(p.locator(".agent-tool").last()).toContainText("ran: ls");
      },
      "the agent saying it's done",
    );
  } catch (e) {
    await j.attach();
    throw e;
  }
  await j.finish();
});
