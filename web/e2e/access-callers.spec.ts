// #663: grants go to a tailnet login, so two people signed in as one login
// share its role, and a tagged device has no login at all. Both are said:
// the friend's login, seen from two devices through `tailscale serve`, is
// named in the share dialog and `GET /api/acl`, and a tagged device that
// called is listed as refused. A stand-in for tailscaled's socket answers
// WhoIs for the addresses serve forwards (X-Forwarded-For).

import { spawn, type ChildProcess } from "node:child_process";
import { mkdtempSync, rmSync } from "node:fs";
import { request } from "node:http";
import { createServer, type Server } from "node:net";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { expect, test } from "@playwright/test";
import { closeContexts } from "./helpers";
import { ANY, daemonPort } from "./ports";
import { labs } from "./labs";

test.afterAll(closeContexts);

const OWNER = "me@example.com";
const FRIEND = "friend@example.com";
const TAILNET = "geek.tail1.ts.net";
// What the stand-in says of each address serve forwards.
const NODES: Record<string, object> = {
  "100.64.0.1": { Node: { ComputedName: "friend-laptop" }, UserProfile: { LoginName: FRIEND } },
  "100.64.0.2": { Node: { ComputedName: "family-ipad" }, UserProfile: { LoginName: FRIEND } },
  "100.64.0.3": { Node: { ComputedName: "ci-1", Tags: ["tag:ci"] }, UserProfile: { LoginName: "tagged-devices" } },
};

let base = "";
let daemon: ChildProcess;
let tailscaled: Server;
let dir: string;

test.use({ baseURL: async ({}, use) => use(base) });

test.beforeAll(async () => {
  dir = mkdtempSync(join(tmpdir(), "arugula-e2e-callers-"));
  const sock = join(dir, "ts.sock");
  tailscaled = createServer((c) => {
    c.once("data", (d) => {
      const path = d.toString().split(" ")[1] ?? "";
      const addr = new URL(path, "http://x").searchParams.get("addr") ?? "";
      const body = path.startsWith("/localapi/v0/status")
        ? { BackendState: "Running", TUN: true, Self: { DNSName: `${TAILNET}.`, UserID: 1, TailscaleIPs: ["100.64.0.10"] }, User: { "1": { LoginName: OWNER } } }
        : NODES[addr.split(":")[0]];
      const text = body ? JSON.stringify(body) : "";
      c.end(`HTTP/1.0 ${body ? "200 OK" : "404 Not Found"}\r\nContent-Length: ${Buffer.byteLength(text)}\r\n\r\n${text}`);
    });
  });
  await new Promise<void>((r) => tailscaled.listen(sock, r));
  daemon = spawn(
    "../target/debug/arugulad",
    [
      ...["--listen", ANY, "--state-dir", labs(dir), "--owner", OWNER],
      ...["--shell", "bash --norc --noprofile", "--no-manager-env", "--tailscale-socket", sock],
    ],
    { stdio: "ignore" },
  );
  base = `http://127.0.0.1:${await daemonPort(dir, daemon)}`;
  for (let i = 0; i < 100; i++) {
    try {
      if ((await fetch(`${base}/api/host`)).ok) return;
    } catch {
      // not yet
    }
    await new Promise((r) => setTimeout(r, 100));
  }
});

test.afterAll(() => {
  daemon?.kill("SIGKILL");
  tailscaled?.close();
  rmSync(dir, { recursive: true, force: true });
});

/** A request as serve passes it on, for the tailnet name: the caller's login (none: a tagged node), from
 * `ip`. Its status. (node:http, as fetch won't send another Host.) */
const viaServe = (path: string, ip: string, login: string | null) =>
  new Promise<number>((done, failed) => {
    const { hostname, port } = new URL(base);
    const headers = { host: TAILNET, "x-forwarded-for": ip, ...(login ? { "tailscale-user-login": login } : {}) };
    request({ hostname, port, path, headers }, (res) => {
      res.resume();
      done(res.statusCode ?? 0);
    })
      .on("error", failed)
      .end();
  });

test("one login on two devices shares one role, and a tagged device is refused, and both are said", async ({ page }) => {
  await page.goto("/");
  await expect.poll(() => page.evaluate(() => window.__arugula?.client.connected)).toBe(true);
  const session = await page.evaluate(() => window.__arugula.client.state!.sessions[0].id);
  const grant = await fetch(base + "/api/acl", {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ session, principal: `tailnet:${FRIEND}`, role: "viewer" }),
  });
  expect(grant.ok).toBe(true);

  // The friend's login from two devices: both let in, with the one role.
  expect(await viaServe("/", "100.64.0.1", FRIEND)).toBe(200);
  expect(await viaServe("/", "100.64.0.2", FRIEND)).toBe(200);
  // A tagged node, through serve: no login, refused.
  expect(await viaServe("/", "100.64.0.3", null)).toBe(403);

  const acl = await (await fetch(base + "/api/acl")).json();
  expect(acl.callers.shared).toEqual([
    {
      login: FRIEND,
      devices: [
        { device: "family-ipad", at: expect.any(Number) },
        { device: "friend-laptop", at: expect.any(Number) },
      ],
    },
  ]);
  expect(acl.callers.tagged).toEqual([{ device: "ci-1", tags: ["tag:ci"], at: expect.any(Number) }]);

  // The share dialog says it, beside the grant.
  await page.getByRole("button", { name: /share/i }).first().click();
  await expect(page.locator(`[data-shared-login="${FRIEND}"]`)).toHaveText(
    `${FRIEND} is signed in on 2 devices (family-ipad, friend-laptop): everyone on them has this role, and Arugula can't tell them apart.`,
  );
  await expect(page.locator('[data-tagged-caller="ci-1"]')).toContainText("ci-1 (tag:ci) is a tagged device: it has no login");
});
