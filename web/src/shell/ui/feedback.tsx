// Send feedback from the shell (Arugula #1217): the shared sheet
// (shared/ui/feedback.tsx), sending through `client`'s daemon.

import type { Client } from "../client";
import { desktopApp } from "../../shared/desktop";
import { pagePath } from "../../shared/feedback";
import { openFeedbackSheet } from "../../shared/ui/feedback";
import { sendFeedback } from "../feedback";
import { getSignedIn } from "../signed-in";

/** Whether an account is signed in on this page (control's page, the app,
 * the desktop app signed in to control). A daemon's own page has none. */
const accountSignedIn = () => getSignedIn()?.phase === "ready" || !!desktopApp()?.signedIn;

/** Open Send feedback, sending through `client`'s daemon. */
export function openFeedback(client: Client) {
  openFeedbackSheet({
    signedIn: accountSignedIn(),
    send: (w) => sendFeedback(client, { ...w, source: "person", surface: desktopApp() ? "desktop" : "web", route: pagePath() }),
  });
}
