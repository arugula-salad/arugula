// J6 (#664): two people on one tailnet, with no control account. Ana owns
// a machine; Ben, on the same tailnet with his own Tailscale login, gets a
// session of hers: she shares it to his login, he opens it at her
// machine's tailnet address, watches, then drives once she lets him. Then
// Cam, who shares Ben's Tailscale login, opens it too: it works, with
// Ben's role, and Ana's Share dialog says the login is on two devices
// (#663). Rules as in journey-j1.spec.ts: only what's on screen, each
// step's prompt named.
//
// There's no control in this world. A stand-in for tailscaled's socket
// says who owns the machine and which device each tailnet address is; Ben
// and Cam reach Ana's daemon as `tailscale serve` passes a request on:
// with their login in Tailscale-User-Login, from their device's address.
//   just journey j6

import { spawn, type ChildProcess } from "node:child_process";
import { mkdtempSync, rmSync } from "node:fs";
import { createServer, type Server } from "node:net";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { expect, test, type Browser, type Page } from "@playwright/test";
import { clickTerminal, screenText } from "./journey/first-run";
import { Journey, type Surface } from "./journey/record";
import { asPerson, BIN } from "./journey/world";
import { closeContexts } from "./helpers";
import { ANY, daemonPort } from "./ports";

test.afterAll(closeContexts);
test.use({ actionTimeout: 20_000 });

const ANA = "ana@example.com";
const BEN = "ben@example.com";
const TAILNET = "anamac.tail1.ts.net";
// The tailnet as tailscaled tells it: each address's device and login.
const DEVICES: Record<string, { name: string; login: string }> = {
  "100.64.0.11": { name: "ben-laptop", login: BEN },
  "100.64.0.12": { name: "cam-laptop", login: BEN },
};

let daemon: ChildProcess;
let tailscaled: Server;
let dir: string;
let base = "";

test.beforeAll(async () => {
  dir = mkdtempSync(join(tmpdir(), "arugula-journey-j6-"));
  const sock = join(dir, "ts.sock");
  tailscaled = createServer((c) => {
    c.once("data", (d) => {
      const path = d.toString().split(" ")[1] ?? "";
      const addr = (new URL(path, "http://x").searchParams.get("addr") ?? "").split(":")[0];
      const dev = DEVICES[addr];
      const body = path.startsWith("/localapi/v0/status")
        ? { BackendState: "Running", TUN: true, Self: { DNSName: `${TAILNET}.`, UserID: 1, TailscaleIPs: ["100.64.0.10"] }, User: { "1": { LoginName: ANA } } }
        : dev && { Node: { ComputedName: dev.name }, UserProfile: { LoginName: dev.login } };
      const text = body ? JSON.stringify(body) : "";
      c.end(`HTTP/1.0 ${body ? "200 OK" : "404 Not Found"}\r\nContent-Length: ${Buffer.byteLength(text)}\r\n\r\n${text}`);
    });
  });
  await new Promise<void>((r) => tailscaled.listen(sock, r));
  daemon = spawn(
    `${BIN}/arugulad`,
    [
      ...["--listen", ANY, "--name", "anamac", "--state-dir", join(dir, "state")],
      ...["--shell", "bash --norc --noprofile", "--no-manager-env", "--tailscale-socket", sock],
    ],
    { stdio: "ignore", env: { ...process.env, HOME: dir, ARUGULA_CLAUDE_IDE_DIR: join(dir, "ide") } },
  );
  base = `http://127.0.0.1:${await daemonPort(join(dir, "state"), daemon)}`;
});

test.afterAll(() => {
  daemon?.kill("SIGKILL");
  tailscaled?.close();
  rmSync(dir, { recursive: true, force: true, maxRetries: 5, retryDelay: 200 });
});

/** Someone's own browser. Ben and Cam come through the tailnet, as serve passes them on. */
async function browserOf(browser: Browser, from?: { ip: string; login: string }): Promise<Page> {
  const ctx = await browser.newContext({
    viewport: { width: 1100, height: 720 },
    ...(from ? { extraHTTPHeaders: { "tailscale-user-login": from.login, "x-forwarded-for": from.ip } } : {}),
  });
  await ctx.addInitScript(asPerson);
  return ctx.newPage();
}

test("J6: two people on one tailnet, no control: share by login, watch, drive, and one login on two devices", async ({ browser }, info) => {
  test.setTimeout(300_000);
  const ana = await browserOf(browser);
  const j = new Journey("J6", "two people on one tailnet, no control: a session shared by tailnet login", info, ["ana", "ben", "cam"]);
  let ben: Page | undefined;
  let cam: Page | undefined;
  let address = "";
  const step = (
    id: string,
    title: string,
    actor: "ana" | "ben" | "cam",
    surface: Surface,
    page: () => Page,
    prompt: (p: Page) => ReturnType<Page["getByText"]> | null,
    act: (p: Page) => Promise<void>,
    wanted?: string,
  ) => j.step({ id, title, actor, surface, page, prompt: prompt(page()), expect: wanted }, () => act(page()));

  try {
    await j.step({ id: "open", title: "Ana opens Arugula on her Mac", actor: "ana", surface: "browser", page: ana, prompt: null, own: "her own machine's page" }, async () => {
      await ana.goto(base);
      await expect.poll(() => ana.evaluate(() => window.__arugula?.client.connected)).toBe(true);
    });

    await step(
      "close-setup",
      "Ana closes Arugula's setup: she wants no account, only her tailnet",
      "ana",
      "browser",
      () => ana,
      (p) => p.getByRole("dialog", { name: "Getting started" }).getByRole("button", { name: "Close" }),
      async (p) => {
        await p.getByRole("dialog", { name: "Getting started" }).getByRole("button", { name: "Close" }).click();
        await expect(p.getByRole("dialog", { name: "Getting started" })).toBeHidden();
      },
    );

    await step(
      "share",
      "Ana opens Share for her session",
      "ana",
      "browser",
      () => ana,
      (p) => p.getByRole("button", { name: /share/i }).first(),
      async (p) => {
        await p.getByRole("button", { name: /share/i }).first().click();
      },
      "a Share button on the main screen",
    );

    await step(
      "share-who",
      "Ana enters Ben's tailnet login, as someone who watches",
      "ana",
      "browser",
      () => ana,
      (p) => p.getByRole("textbox", { name: "Who" }),
      async (p) => {
        await p.getByRole("textbox", { name: "Who" }).fill(BEN);
        await p.getByRole("combobox", { name: "Role" }).selectOption({ label: "watches" });
        await p.getByRole("button", { name: "Share", exact: true }).click();
        await expect(p.locator(`[data-grant="tailnet:${BEN}"]`)).toBeVisible();
      },
    );

    await step(
      "tell-ben",
      "Ana tells Ben where to open it",
      "ana",
      "message",
      () => ana,
      (p) => p.getByText(/tailnet address/),
      async (p) => {
        address = (await p.locator("[data-share-url]").innerText()).trim();
        expect(address).toContain(TAILNET);
        j.compared(address);
      },
      "the machine's tailnet address, to send Ben",
    );

    await j.step({ id: "ben-opens", title: "Ben opens the address Ana sent", actor: "ben", surface: "browser", prompt: null, own: "the link in Ana's message" }, async () => {
      ben = await browserOf(browser, { ip: "100.64.0.11", login: BEN });
      j.opened(address);
      // The address is the tailnet name; here, the daemon behind it.
      await ben.goto(base);
      await expect.poll(() => ben!.evaluate(() => window.__arugula?.client.connected)).toBe(true);
    });

    await step(
      "ben-watches",
      "Ben sees Ana's session, live",
      "ben",
      "browser",
      () => ben!,
      (p) => p.getByText("you watch", { exact: true }),
      async (p) => {
        await ana.keyboard.press("Escape");
        await clickTerminal(ana);
        await ana.keyboard.type("echo ana-$((6*7))\n", { delay: 5 });
        await expect.poll(() => screenText(p), { timeout: 15_000 }).toContain("ana-42");
      },
      "something saying Ben is watching Ana's session",
    );

    await step(
      "ben-tries",
      "Ben clicks into the terminal and types",
      "ben",
      "browser",
      () => ben!,
      (p) => p.getByText("you watch", { exact: true }),
      async (p) => {
        await clickTerminal(p);
        await p.keyboard.type("echo ben-watching\n", { delay: 5 });
        await new Promise((r) => setTimeout(r, 1500));
        expect(await screenText(ana)).not.toContain("ben-watching");
      },
    );

    await step(
      "make-driver",
      "Ana lets Ben drive: his role, in Share",
      "ana",
      "browser",
      () => ana,
      (p) => p.getByRole("button", { name: /share/i }).first(),
      async (p) => {
        await p.getByRole("button", { name: /share/i }).first().click();
        await p.locator(`[data-grant="tailnet:${BEN}"]`).getByRole("combobox").selectOption({ label: "drives" });
        await p.keyboard.press("Escape");
      },
      "Ben's row in the Share dialog, with his role",
    );

    await step(
      "ask-to-drive",
      "Ben asks Ana to let him drive",
      "ben",
      "browser",
      () => ben!,
      (p) => p.getByText(/trust you with it|Ask the owner to let me drive|you may drive/i),
      async (p) => {
        const v = p.viewportSize()!;
        await p.mouse.click(v.width / 2, v.height / 2, { button: "right" });
        await p.getByRole("menuitem", { name: "Ask the owner to let me drive it" }).click();
      },
    );

    await step(
      "allow",
      "Ana allows Ben to drive, for 10 minutes",
      "ana",
      "browser",
      () => ana,
      (p) => p.getByText(/asks to drive/),
      async (p) => {
        const ask = p.getByText(/asks to drive/).locator("..");
        await ask.getByRole("combobox").selectOption("10");
        await ask.getByRole("button", { name: "Allow" }).click();
      },
    );

    await step(
      "take-control",
      "Ben takes control of the pane",
      "ben",
      "browser",
      () => ben!,
      (p) => p.getByText(/take control|ask for control/i),
      async (p) => {
        await p.getByRole("button", { name: "Take control" }).first().click();
      },
    );

    await step(
      "ben-drives",
      "Ben types in Ana's terminal, and Ana sees it",
      "ben",
      "browser",
      () => ben!,
      (p) => p.getByText("✎ you drive", { exact: true }),
      async (p) => {
        await clickTerminal(p);
        j.typed("echo hi-from-ben-$((6*7))");
        await p.keyboard.type("echo hi-from-ben-$((6*7))\n", { delay: 5 });
        await expect.poll(() => screenText(ana), { timeout: 15_000 }).toContain("hi-from-ben-42");
      },
    );

    await j.step({ id: "cam-opens", title: "Cam, on Ben's Tailscale login, opens the same address", actor: "cam", surface: "browser", prompt: null, own: "Ben forwards Ana's message" }, async () => {
      cam = await browserOf(browser, { ip: "100.64.0.12", login: BEN });
      j.opened(address);
      await cam.goto(base);
      await expect.poll(() => cam!.evaluate(() => window.__arugula?.client.connected)).toBe(true);
      await expect.poll(() => screenText(cam!), { timeout: 15_000 }).toContain("hi-from-ben-42");
    });

    await step(
      "ana-sees-shared-login",
      "Ana sees that Ben's login is on two devices, which share his role",
      "ana",
      "browser",
      () => ana,
      (p) => p.getByRole("button", { name: /share/i }).first(),
      async (p) => {
        await p.getByRole("button", { name: /share/i }).first().click();
        await expect(p.locator(`[data-shared-login="${BEN}"]`)).toContainText("signed in on 2 devices (ben-laptop, cam-laptop)");
      },
      "the Share dialog's note on Ben's login",
    );
  } catch (e) {
    await j.attach();
    throw e;
  }
  await j.finish();
});
