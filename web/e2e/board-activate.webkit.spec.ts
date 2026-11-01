// #472, WebKit: see board-activate-cases.ts.
import { boardActivateCases } from "./board-activate-cases";

// Playwright's WebKit stops loading the page once the service worker is registered (as in chat-attach-key.webkit.spec.ts).
boardActivateCases("board-activate-webkit", (browser, t) => t.person(browser, "alice", { serviceWorkers: "block" }));
