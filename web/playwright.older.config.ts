// The page at HEAD against released daemons (Arugula #808): `just
// older-daemons`. Control serves its newest page to every machine, so a
// page change that breaks an older daemon's rows breaks every machine not
// yet updated (0.66.12's Docs page against 0.66.10). One project per daemon,
// from OLDER_DAEMONS (scripts/older-daemons.mjs's list: by default the
// oldest and the newest release of the last 30 days, #807). The specs are
// e2e/*.older.ts, which `just e2e`'s config doesn't pick up.

import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { defineConfig } from "@playwright/test";
import "./e2e/scrub-env.ts";
import { keyHeaders } from "./e2e/local-token.ts";
import type { OlderDaemon } from "./e2e/older-daemon.ts";

const daemons = JSON.parse(process.env.OLDER_DAEMONS || "[]") as OlderDaemon[];
if (!daemons.length) throw new Error("OLDER_DAEMONS is empty: run `just older-daemons`, which fetches them");

// Agents' adapters and Claude's directory: the run's own, as the suite's.
if (!process.env.CLAUDE_CONFIG_DIR) {
  const dir = mkdtempSync(join(tmpdir(), "arugula-older-claude-"));
  process.env.CLAUDE_CONFIG_DIR = dir;
  process.on("exit", () => rmSync(dir, { recursive: true, force: true }));
}

export default defineConfig({
  testDir: "e2e",
  testMatch: /\.older\.ts$/,
  timeout: 60_000,
  expect: { timeout: 10_000 },
  retries: 0,
  fullyParallel: false,
  workers: 1,
  use: {
    viewport: { width: 1000, height: 640 },
    extraHTTPHeaders: keyHeaders,
    // The page's files come from web/dist by route (e2e/older-daemon.ts):
    // a service worker would fetch around it.
    serviceWorkers: "block",
    // A page answered by a route has no address, so Chrome takes it for a
    // public one asking for loopback.
    permissions: ["local-network-access"],
    screenshot: "only-on-failure",
    channel: process.env.E2E_CHROMIUM ? "chromium" : "chrome",
  },
  projects: daemons.map((d) => ({ name: `daemon ${d.version}`, use: { older: d } })),
});
