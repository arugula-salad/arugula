// The Copy link button (#411) in WebKit: the clipboard can't be read in a test, so
// the button's `data-link` and its "Copied ✓" stand in (copy-link-steps.ts).
import { test } from "@playwright/test";
import { copyLinkSpecs } from "./copy-link-steps";

// Playwright's WebKit stops loading the page once the service worker is registered (platform-rail.webkit.spec.ts).
test.use({ serviceWorkers: "block" });
// Playwright's Linux WebKit asserts on the platform page's origin (#402); macOS WebKit runs these.
test.skip(process.platform === "linux", "Playwright's Linux WebKit asserts on the platform page (#402)");
copyLinkSpecs("copylink-webkit", false);
