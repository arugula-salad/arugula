// A person's first run of the Mac app (#551): J1's path, from a fresh home
// and no account to a command in a terminal and the app reopened. J1 is
// this as its own journey; J2 runs it for each person first, as its own
// report, so a signup failure can't hide a linking failure.

import { expect, type Locator, type Page } from "@playwright/test";
import type { App } from "./app";
import type { Journey } from "./record";
import type { World } from "./world";

export const CODE = /\b[A-Z0-9]{4,6}-[A-Z0-9]{4,6}\b/;
export const FP = /\b[0-9a-f]{4}(?:[-\s][0-9a-f]{4}){3,}\b/;

/** The code or fingerprint in `loc`'s visible text. */
export async function read(loc: Locator, re: RegExp): Promise<string> {
  await expect(loc.first()).toBeVisible();
  const m = re.exec(await loc.first().innerText());
  if (!m) throw new Error(`no ${re} in "${await loc.first().innerText()}"`);
  return m[0].replace(/\s/g, "-");
}

/** What the terminal shows (read, never driven, through the page's own
 * hook: the canvas has no text a locator finds). */
export const screenText = (page: Page) =>
  page.evaluate(() =>
    window.__arugula?.client.state
      ? window.__arugula.text(window.__arugula.client.active()!)
      : "",
  );

/** Click into the terminal as a person does: the big area under the bar. */
export async function clickTerminal(page: Page) {
  const v = page.viewportSize()!;
  await page.mouse.click(v.width / 2, v.height / 2);
}

/** J1's steps, recorded in `j` as `actor`, on `app` (whose machine is
 * `machine`). Leaves the app open on control's page, signed in. */
export async function firstRun(
  j: Journey,
  world: World,
  app: App,
  actor: string,
  machine: string,
) {
  const step = (
    id: string,
    title: string,
    surface: "app" | "browser" | "terminal",
    page: () => Page,
    prompt: (p: Page) => Locator | null,
    act: (p: Page) => Promise<void>,
    why?: string,
  ) =>
    j.step(
      { id, title, actor, surface, page: page(), prompt: prompt(page()), why },
      () => act(page()),
    );
  const win = () => app.window;
  const tab = () => app.lastOpened;
  const start = () => win().getByRole("dialog", { name: "Getting started" });
  let machineCode = "";
  let account = "";
  let appCode = "";

  await j.step(
    {
      id: "app-opens",
      title: "Open the app for the first time",
      actor,
      surface: "app",
      prompt: null,
      own: "opens the app they installed",
    },
    async () => {
      await app.launch();
    },
  );

  await step(
    "getting-started",
    "Getting started greets you",
    "app",
    win,
    () => start().getByRole("heading").first(),
    async () => {
      await expect(start()).toBeVisible({ timeout: 20_000 });
    },
  );

  await step(
    "set-it-up",
    "Start setting up",
    "app",
    win,
    () => start().getByText(/Each takes a click/),
    async () => {
      await start()
        .getByRole("button", { name: /Set it up/ })
        .click();
    },
  );

  await step(
    "skip-phone",
    "Skip the phone for now",
    "app",
    win,
    () => start().getByRole("button", { name: /Skip for now/ }),
    async () => {
      await start()
        .getByRole("button", { name: /Skip for now/ })
        .click();
      await expect(
        start().getByRole("heading", { name: /to your account or team/ }),
      ).toBeVisible();
    },
  );

  await step(
    "connect",
    "Connect this machine to the cloud",
    "app",
    win,
    () => start().getByRole("button", { name: /^Connect to / }),
    async () => {
      await start()
        .getByRole("button", { name: /^Connect to / })
        .click();
      await expect(
        start().getByText("Approve this code on a device you use"),
      ).toBeVisible({ timeout: 20_000 });
      machineCode = await read(
        start()
          .getByText("Approve this code on a device you use")
          .locator(".."),
        CODE,
      );
    },
  );

  await step(
    "open-approval",
    "Open the approval link",
    "app",
    win,
    () => start().getByRole("link", { name: /Approve in Arugula cloud/ }),
    async (p) => {
      const before = app.opened.length;
      await start()
        .getByRole("link", { name: /Approve in Arugula cloud/ })
        .click();
      await expect
        .poll(() => app.opened.length, { timeout: 10_000 })
        .toBeGreaterThan(before);
      void p;
    },
  );

  await step(
    "sign-up",
    "Make an account from the approval link",
    "browser",
    tab,
    (p) => p.getByText("Sign in to approve this machine."),
    async (p) => {
      await p.getByRole("link", { name: "Sign in with GitHub" }).click();
    },
  );

  await step(
    "github-authorize",
    "Authorize Arugula on GitHub",
    "browser",
    tab,
    (p) => p.getByRole("button", { name: /Authorize/ }),
    async (p) => {
      await p.getByRole("button", { name: /Authorize/ }).click();
    },
  );

  await step(
    "recovery-codes",
    "Keep the recovery codes",
    "browser",
    tab,
    (p) => p.getByRole("heading", { name: "Your recovery codes" }),
    async (p) => {
      await p.getByLabel("I've stored these somewhere safe").check();
      await p.getByRole("button", { name: "Continue" }).click();
    },
  );

  await step(
    "approve-machine",
    "Approve the machine, checking its code",
    "browser",
    tab,
    (p) => p.getByRole("heading", { name: "Add a machine?", exact: true }),
    async (p) => {
      const shown = await read(p.getByText(/asks to join/), CODE);
      j.compared(`machine code ${machineCode} (app) = ${shown} (browser)`);
      expect(shown).toBe(machineCode);
      account = await read(p.getByText(/Your account:/), FP);
      await p.getByRole("button", { name: /^Approve/ }).click();
      await expect(
        p.getByRole("heading", { name: "Add a machine?", exact: true }),
      ).toBeHidden({ timeout: 20_000 });
    },
  );

  await step(
    "confirm-account",
    "Back in the app, check the account's fingerprint",
    "app",
    win,
    () => start().getByText("Is this your account?"),
    async () => {
      const shown = await read(
        start().getByText("Is this your account?").locator(".."),
        FP,
      );
      j.compared(`account ${account} (browser) = ${shown} (app)`);
      expect(shown).toBe(account);
      await start().getByRole("button", { name: "They match" }).click();
    },
  );

  await step(
    "joined",
    "The app says the machine is in",
    "app",
    win,
    (p) =>
      p.getByText(/Joined to your account|This window, every machine/).first(),
    async () => {},
  );

  await step(
    "app-signin",
    "Sign the app in, so its window reaches the account",
    "app",
    win,
    (p) => p.getByRole("button", { name: "Sign the app in" }),
    async (p) => {
      await expect(
        p.getByRole("heading", { name: "This window, every machine" }),
      ).toBeVisible({ timeout: 20_000 });
      const before = app.opened.length;
      await p.getByRole("button", { name: "Sign the app in" }).click();
      await expect(p.getByText("Check your browser shows")).toBeVisible({
        timeout: 20_000,
      });
      appCode = await read(
        p.getByText("Check your browser shows").locator(".."),
        /\b[A-Z0-9-]{4,}\b/,
      );
      await expect
        .poll(() => app.opened.length, { timeout: 10_000 })
        .toBeGreaterThan(before);
    },
  );

  await step(
    "allow-app",
    "Allow the app in the browser, checking its code",
    "browser",
    tab,
    (p) => p.getByRole("heading", { name: "Sign in the app?" }),
    async (p) => {
      const shown = (
        await p.getByText(/asks to sign in as you/).innerText()
      ).includes(appCode);
      j.compared(
        `app sign-in code ${appCode} (app) shown in the browser: ${shown}`,
      );
      expect(shown).toBe(true);
      await p.getByRole("button", { name: "Allow" }).click();
    },
  );

  let appFp = "";
  await step(
    "approve-app",
    "Approve the app as a device, checking its fingerprint",
    "browser",
    () =>
      app.opened.find(
        (t) => !t.isClosed() && t.url().startsWith(world.control),
      ) ?? tab(),
    (p) => p.getByRole("heading", { name: "New device?" }),
    async (p) => {
      appFp = await read(
        win().getByText("It shows this fingerprint").locator(".."),
        FP,
      );
      const shown = await read(
        p.getByRole("heading", { name: "New device?" }).locator(".."),
        FP,
      );
      j.compared(`app device ${appFp} (app) = ${shown} (browser)`);
      expect(shown).toBe(appFp);
      await p.getByRole("button", { name: "Approve" }).click();
    },
  );

  await step(
    "terminal",
    "Run a command in a terminal on the machine",
    "app",
    win,
    (p) => p.getByRole("button", { name: new RegExp(machine) }).first(),
    async (p) => {
      await expect.poll(() => screenText(p), { timeout: 30_000 }).not.toBe("");
      await clickTerminal(p);
      const cmd = "echo hello-$((6*7))";
      j.typed(cmd);
      await p.keyboard.type(`${cmd}\n`, { delay: 5 });
      await expect
        .poll(() => screenText(p), { timeout: 15_000 })
        .toContain("hello-42");
    },
  );

  await j.step(
    {
      id: "restart",
      title:
        "Quit and reopen the app: still signed in, the terminal still there",
      actor,
      surface: "app",
      page: win,
      prompt: null,
      own: "quits and reopens the app",
    },
    async () => {
      await app.quit();
      const w = await app.launch();
      await expect(
        w.getByRole("button", { name: new RegExp(machine) }).first(),
      ).toBeVisible({ timeout: 30_000 });
      await expect
        .poll(() => screenText(w), { timeout: 30_000 })
        .toContain("hello-42");
    },
  );
}
