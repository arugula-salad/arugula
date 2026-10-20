// #1067: an agent asks to share its Doc. Claude Code (here, its requests by
// hand over /mcp) finishes a Doc and calls `doc share`; the owner's page
// shows a card on the Doc block itself, with what sharing means and Share or
// Not now, on a desktop and on a Pixel 7. Not now is said to the agent once;
// Share runs the owner's own share (this daemon hasn't joined control, so it
// fails, and the agent reads why). The upload, the owner-only answer and the
// restart are the daemon's integration tests (doc_share_ask.rs).

import { mkdirSync } from "node:fs";
import { expect, test, type APIRequestContext, type Page } from "@playwright/test";
import { bring } from "./doc-steps";
import { closeContexts, reset } from "./helpers";
import { pixel7 } from "./phones";

test.afterAll(closeContexts);

const SHOTS = "test-results/1067";

/** One MCP session over Streamable HTTP (mcp.spec.ts's). */
async function mcp(request: APIRequestContext, name: string) {
  const base = { "Content-Type": "application/json", Accept: "application/json, text/event-stream" };
  const messages = (body: string) =>
    body
      .split("\n\n")
      .map((e) => e.split("\n").filter((l) => l.startsWith("data:")).map((l) => l.slice(5).trim()).join(""))
      .filter((d) => d)
      .map((d) => JSON.parse(d));
  const init = await request.post("/mcp", {
    headers: base,
    data: { jsonrpc: "2.0", id: 1, method: "initialize", params: { protocolVersion: "2025-06-18", capabilities: {}, clientInfo: { name, version: "1" } } },
  });
  expect(init.status()).toBe(200);
  const session = init.headers()["mcp-session-id"];
  const headers = { ...base, "Mcp-Session-Id": session, "MCP-Protocol-Version": "2025-06-18" };
  await request.post("/mcp", { headers, data: { jsonrpc: "2.0", method: "notifications/initialized" } });
  let id = 2;
  return async (tool: string, args: object) => {
    const res = await request.post("/mcp", {
      headers,
      data: { jsonrpc: "2.0", id: ++id, method: "tools/call", params: { name: tool, arguments: args } },
      timeout: 30_000,
    });
    const answer = messages(await res.text()).find((m) => m.id === id);
    expect(answer?.result?.isError, JSON.stringify(answer)).toBeFalsy();
    return answer.result.structuredContent;
  };
}

/** Claude Code makes a Doc and asks to share it: the card is up. */
async function asked(page: Page, request: APIRequestContext) {
  await reset(page);
  const call = await mcp(request, "claude-code");
  const made = await call("doc", { kind: "new", title: "Quarterly plan", sections: [{ heading: "Goals", intent: "what we commit to" }] });
  const doc: number = made.block;
  await call("doc", { kind: "fill", block: doc, section: "s1", markdown: "Ship the Doc block." });
  const first = await call("doc", { kind: "share", block: doc });
  expect(first.status).toBe("waiting");
  await bring(page, doc);
  const card = page.locator(".pane-ask", { hasText: "Share 'Quarterly plan' with a link anyone can open?" });
  await expect(card).toBeVisible();
  await expect(card).toContainText("Anyone with the link can read it, without an account. You can update or unshare it from the Doc's menu.");
  await expect(card.getByRole("button", { name: "Share", exact: true })).toBeVisible();
  await expect(card.getByRole("button", { name: "Not now" })).toBeVisible();
  return { call, doc, card };
}

test("the card on the Doc: Not now is said once, and Share is the owner's own share", async ({ page, request }) => {
  const { call, doc, card } = await asked(page, request);
  mkdirSync(SHOTS, { recursive: true });
  await page.screenshot({ path: `${SHOTS}/desktop.png` });

  // Asking again while it waits is no second card.
  expect((await call("doc", { kind: "share", block: doc })).status).toBe("waiting");
  await expect(page.locator(".pane-ask")).toHaveCount(1);

  await card.getByRole("button", { name: "Not now" }).click();
  await expect(card).toHaveCount(0);
  expect((await call("doc", { kind: "share", block: doc })).status).toBe("declined");

  // Asking after a decline raises a new card; Share fails here (no control)
  // and the agent is told why, once.
  expect((await call("doc", { kind: "share", block: doc })).status).toBe("waiting");
  await expect(card).toBeVisible();
  await card.getByRole("button", { name: "Share", exact: true }).click();
  await expect(card).toHaveCount(0);
  await expect.poll(async () => (await call("doc", { kind: "share", block: doc })).status).toBe("failed");
});

test.describe("on a Pixel 7", () => {
  test.use(pixel7);

  test("the card is on the Doc, and in the phone's needs-you list", async ({ page, request }) => {
    const { doc, card } = await asked(page, request);
    mkdirSync(SHOTS, { recursive: true });
    await page.screenshot({ path: `${SHOTS}/pixel7.png` });
    const ask = await page.evaluate((d) => window.__arugula.client.info(d)?.ask ?? null, doc);
    expect(ask?.source).toBe("doc");
    await card.getByRole("button", { name: "Not now" }).click();
    await expect(card).toHaveCount(0);
  });
});
