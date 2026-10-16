import { defineConfig } from "@playwright/test";
export default defineConfig({
  testDir: "e2e", testMatch: "cloud.spec.ts", workers: 1, timeout: 60000,
  use: { channel: "chrome", viewport: { width: 1000, height: 800 } },
});
