// The platform page's browser-tab titles (UI audit #159): what the page is,
// then the project it's in, then Arugula, so tabs and history read apart
// ("Runs · Acme · Arugula", "#142 Fix the bar · Arugula"). Plain functions
// over what the page knows so far, tested in scripts/platform-titles.test.mjs:
// a name not yet known is left out, never guessed.

import type { Route } from "./paths";

/** What the page knows of the things a route names, so far. */
export interface Known {
  /** A project's name. */
  project(id: string): string | null | undefined;
  /** An issue's number and title. */
  issue(id: string): { number: number; title: string } | null | undefined;
  /** A run's title. */
  run(id: string): string | null | undefined;
  /** A chat room's title (`#general`, a person's name). */
  room(id: string): string | null | undefined;
  /** A project Doc's title (aru-118). */
  doc?(id: string): string | null | undefined;
  /** The project the bar shows for a route that names none (a deploy's). */
  shown?: string;
}

const APP = "Arugula";
const CHAT_VIEWS = { activity: "Activity", saved: "Saved", search: "Search", threads: "Threads" } as const;

/** The tab's title for `r`. */
export function titleOf(r: Route, k: Known): string {
  const p = "project" in r && r.project ? k.project(r.project) : k.shown ? k.project(k.shown) : null;
  const parts = (...xs: (string | null | undefined)[]) => [...xs, APP].filter(Boolean).join(" · ");
  switch (r.page) {
    case "home":
      return parts("Projects");
    case "project":
      return parts(p);
    case "issues":
      return parts("Issues", p);
    case "new-issue":
      return parts("New issue", p);
    case "issue": {
      const i = k.issue(r.issue);
      return i ? parts(`#${i.number} ${i.title}`, p) : parts("Issue", p);
    }
    case "boards":
      return parts("Boards", p);
    case "board":
      return parts("Board", p);
    case "docs":
      return parts("Docs", p);
    case "doc": {
      const title = k.doc?.(r.doc);
      return parts(title ?? "Doc", p);
    }
    case "runs":
      return parts("Runs", p);
    case "run":
      return parts(k.run(r.run) ?? "Run", p);
    case "session":
      return parts("Agent session", p);
    case "chat":
      if (r.view) return parts(CHAT_VIEWS[r.view], "Chat", p);
      return parts(r.room ? (k.room(r.room) ?? "Chat") : "Chat", p);
    case "dm":
      return parts(k.room(r.room) ?? "Direct message", p);
    case "workspaces":
      return parts("Cloud workspaces", p);
    case "workspace":
      return parts("Cloud workspace", p);
    case "project-settings":
      return parts("Settings", p);
    case "deploys":
      return parts("Deploys", p);
    case "new-deploy":
      return parts("Deploy…", p);
    case "deploy":
      return parts("Deploy", p);
    case "installed":
      return parts("GitHub App installed", p);
    case "inbox":
      return parts("Inbox");
    case "settings":
      return parts("Account settings");
    case "missing":
      return parts("Not found");
    case "find":
      return APP;
  }
}
