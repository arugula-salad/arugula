// Arugula #85: an agent block's kept images (M71) on a viewer's page. The
// owner pastes a picture into the composer and sends it; someone the
// session is shared with as a viewer sees it in the transcript (reading a
// kept image is a viewer's call, as a Doc's pictures are, #965), from a
// data: URL, and still can't prompt the agent. The same through control's
// page is agent-images-control.spec.ts, in the cloud repository.

import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import type { ChildProcess } from "node:child_process";
import { expect, test, type Locator, type Page } from "@playwright/test";
import { bring, png, sessionOf } from "./doc-steps";
import { closeContexts, open, paneEl, ready } from "./helpers";
import { daemon } from "./phones";

const fake = fileURLToPath(new URL("../../crates/daemon/tests/fake_acp.py", import.meta.url));
// A picture big enough to see in the PR's screenshots.
const PNG = png(160, 96, [60, 140, 90]).toString("base64");
const OWNER = "me@example.com";
const FRIEND = "friend@example.com";

let dir = "";
let url = "";
let proc: ChildProcess | undefined;
test.use({ baseURL: async ({}, use) => use(url) });

test.beforeAll(async () => {
  dir = mkdtempSync(join(tmpdir(), "arugula-e2e-agent-images-"));
  ({ proc, url } = await daemon(join(dir, "state"), ["--owner", OWNER, "--tailscale-socket", "/nonexistent/sock"]));
});

test.afterAll(async ({ browser }) => {
  await closeContexts({ browser });
  proc?.kill("SIGKILL");
  if (dir) rmSync(dir, { recursive: true, force: true });
});

/** A picture pasted into `block`'s composer and sent with "look". */
async function sendImage(block: Locator) {
  await block.locator(".agent-composer textarea").evaluate((el, b64) => {
    const dt = new DataTransfer();
    dt.items.add(new File([Uint8Array.from(atob(b64), (c) => c.charCodeAt(0))], "shot.png", { type: "image/png" }));
    el.dispatchEvent(new ClipboardEvent("paste", { clipboardData: dt, bubbles: true, cancelable: true }));
  }, PNG);
  await expect(block.locator(".agent-attached-file img")).toHaveCount(1);
  await block.locator(".agent-composer textarea").fill("look");
  await block.locator(".agent-composer textarea").press("Enter");
  await expect(block.locator(".agent-msg").last()).toHaveText("Saw 1 image(s) ['image/png']; text []");
}

/** The kept image in `block`'s last prompt: shown, from a data: URL, and loaded. */
async function seesImage(block: Locator) {
  const img = block.locator(".agent-user").last().locator(".agent-image img");
  await expect(img).toBeVisible({ timeout: 15_000 });
  await expect(img).toHaveAttribute("src", /^data:image\/png;base64,/);
  await expect.poll(() => img.evaluate((i: HTMLImageElement) => i.complete && i.naturalWidth)).toBe(160);
}

async function agent(page: Page): Promise<number> {
  const term = await page.evaluate(() => window.__arugula.client.state!.panes[0].id);
  await ready(page, term);
  const id = await page.evaluate(
    ({ fake, term }) =>
      window.__arugula.client.openBlock({ type: "agent", config: { agent: "acp", command: ["python3", fake, "--images"], cwd: "/tmp", prompt: "hello" }, split: term, local: true } as never),
    { fake, term },
  );
  expect(id).not.toBeNull();
  await bring(page, id!);
  await expect(paneEl(page, id!).locator(".agent-msg").last()).toHaveText("Hello! I am fake.", { timeout: 15_000 });
  return id!;
}

test("a viewer sees the images an agent block keeps, and can't prompt it", async ({ browser, page }) => {
  await open(page);
  const id = await agent(page);
  await sendImage(paneEl(page, id));
  await seesImage(paneEl(page, id));
  const session = await sessionOf(page, id);
  expect((await page.request.post("/api/acl", { data: { session, principal: `tailnet:${FRIEND}`, role: "viewer" } })).ok()).toBe(true);

  const friend = await (await browser.newContext({ baseURL: url, extraHTTPHeaders: { "tailscale-user-login": FRIEND } })).newPage();
  await friend.goto("/");
  await expect.poll(() => friend.evaluate(() => window.__arugula?.client.role())).toBe("viewer");
  await bring(friend, id);
  await seesImage(paneEl(friend, id));
  // Reading is theirs; prompting the agent isn't.
  const send = await friend.request.post(`/api/blocks/${id}/call/send`, { data: { text: "hi", files: [] } });
  expect(send.status()).toBe(403);
  await friend.close();
});
