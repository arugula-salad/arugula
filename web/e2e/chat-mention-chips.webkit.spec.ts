// #438: a mention is missed only when it reached nobody (cases in chat-mention-chips-cases.ts).
// WebKit version (runs on Playwright's WebKit engine).

import { test } from "@playwright/test";
import { mentionChipsTests } from "./chat-mention-chips-cases";

// Playwright's Linux WebKit asserts on the platform page's origin (#402); macOS WebKit runs these.
test.skip(process.platform === "linux", "Playwright's Linux WebKit asserts on the platform page (#402)");
// Playwright's WebKit stops loading the page once the service worker is registered (as in teams-presigned.webkit.spec.ts).
mentionChipsTests("chat-mention-chips-webkit", { serviceWorkers: "block" });
