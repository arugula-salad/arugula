// #428: chat's @ picker lists the machines whose agents hear the room (cases in chat-mention-machines-cases.ts).
// WebKit version (runs on Playwright's WebKit engine).

import { mentionMachinesTests } from "./chat-mention-machines-cases";

// Playwright's WebKit stops loading the page once the service worker is registered (as in teams-presigned.webkit.spec.ts).
mentionMachinesTests("chat-mention-machines-webkit", { serviceWorkers: "block" });
