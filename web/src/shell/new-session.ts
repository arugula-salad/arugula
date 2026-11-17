// The project a new session starts in (Arugula #1163): the app's machine
// page is in a project, and a session made there (New session, a phone's,
// the command palette's) starts in it, set by hand, so the page shows it.
// The app says which (`newSessionsIn`); the daemon's own page names none,
// and its sessions go where the daemon places them. A daemon from before
// ignores the field.

import type { Intent, ProjectRef } from "../shared/proto.gen";

let here: ProjectRef | null = null;

/** New sessions start in `project` from now on, or where the daemon
 * places them (`null`). */
export function newSessionsIn(project: ProjectRef | null) {
  here = project;
}

/** `intent` as it's sent: a `new_session` that names no project starts in
 * the page's; anything else goes as it is. */
export function withProject(intent: Intent): Intent {
  if (intent.op !== "new_session" || intent.project || !here) return intent;
  return { ...intent, project: here };
}
