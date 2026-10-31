// #438: a mention is missed only when it reached nobody (cases in chat-mention-chips-cases.ts).
// WebKit version (runs on Playwright's WebKit engine).

import { mentionChipsTests } from "./chat-mention-chips-cases";

// Playwright's WebKit stops loading the page once the service worker is registered (as in teams-presigned.webkit.spec.ts).
mentionChipsTests("chat-mention-chips-webkit", { serviceWorkers: "block" });
