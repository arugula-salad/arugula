// #497 in WebKit, the desktop app's engine (pop-out-steps.ts).
import { test } from "@playwright/test";
import { closeContexts } from "./helpers";
import { popOutSpecs } from "./pop-out-steps";

// Playwright's WebKit stops loading the page once the service worker is registered (platform-rail.webkit.spec.ts).
test.use({ serviceWorkers: "block" });
test.afterAll(closeContexts);
popOutSpecs("webkit");
