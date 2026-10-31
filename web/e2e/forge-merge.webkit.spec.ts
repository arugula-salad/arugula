// #427 in WebKit: Merge ▾ offers the repository's methods, and a merge takes a confirm click (forge-merge-steps.ts).
import { test } from "@playwright/test";
import { mergeSpecs } from "./forge-merge-steps";

// Playwright's WebKit stops loading the page once the service worker is registered (platform-rail.webkit.spec.ts).
test.use({ serviceWorkers: "block" });
mergeSpecs("webkit");
