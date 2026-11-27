// Go to… (Lift 3, #1479) in WebKit (Safari's engine, and the desktop app's
// on macOS), and on an iPhone. The checks are in goto-steps.ts, shared with
// goto.spec.ts.

import { devices, test } from "@playwright/test";
import { desktop, docs, phone } from "./goto-steps";

// Playwright's WebKit stops loading the page once the service worker is
// registered (real Safari doesn't), so none here.
test.use({ serviceWorkers: "block" });

test("Ctrl+Shift+K opens it from a terminal; it runs the pane menu's kept actions", async ({ page }) => {
  await desktop(page);
});

test("the Docs page is a place, and a Doc that was closed opens by its title", async ({ page }) => {
  await docs(page);
});

test.describe("on an iPhone", () => {
  const { viewport, userAgent, deviceScaleFactor, isMobile, hasTouch } = devices["iPhone 13"];
  test.use({ viewport, userAgent, deviceScaleFactor, isMobile, hasTouch });
  test("from the sheet's Go to… button", async ({ page }) => {
    await phone(page);
  });
});
