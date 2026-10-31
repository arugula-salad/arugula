// #429: an agent's post in chat names its session (cases in chat-agent-session-cases.ts).
// WebKit version (runs on Playwright's WebKit engine).

import { agentSessionTests } from "./chat-agent-session-cases";

// Playwright's WebKit stops loading the page once the service worker is registered (as in teams-presigned.webkit.spec.ts).
agentSessionTests("chat-agent-session-webkit", { serviceWorkers: "block" });
