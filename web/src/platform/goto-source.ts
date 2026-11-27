// What the app's pages put in Go to… (shared/kit/frame/goto.tsx, Lift 3,
// #1479): your projects, each at the place you left it (Arugula #989) with
// what's for you there; the project you're in's places; every project's
// boards and Docs, read when Go to… opens (once a minute at most); and the
// account's commands. Its `projects` scope is the switcher the rail's
// project tile and a phone's bar used to open.

import type { GoItem, GoSource } from "../shared/kit/frame/goto";
import { boards } from "../shared/boards/store";
import { projectDocs } from "../shared/docs/store";
import { SECTION_LABEL, chatPath, sectionOf } from "./bar";
import { currentProject } from "./current-project";
import { openPlatformFeedback } from "./feedback";
import { inboxCounts, subscribeInbox } from "./inbox";
import { names } from "./links";
import { settingsPlaces } from "./settings/scopes";
import { ownedProjects, openNewProject } from "./new-project";
import { owners } from "./owners";
import { parse } from "./paths";
import { placeOf } from "./places-kept";
import { current, go, pathOf, type Route } from "./router";
import { session } from "./session";
import { signOut } from "./signin";
import { barProject } from "./where";
import { recentPlaces } from "../shared/kit/frame/stack";

/** Each project's boards and Docs as last read, and when. */
const lists = new Map<string, { at: number; boards: { id: string; title: string }[]; docs: { id: string; title: string }[] }>();
const listeners = new Set<() => void>();

function read(project: string) {
  const had = lists.get(project);
  if (had && Date.now() - had.at < 60_000) return;
  const now = { at: Date.now(), boards: had?.boards ?? [], docs: had?.docs ?? [] };
  lists.set(project, now);
  const heard = () => listeners.forEach((fn) => fn());
  boards.list(project).then((l) => ((now.boards = l.boards.map((b) => ({ id: b.id, title: b.title }))), heard()), () => {});
  projectDocs.list(project).then((l) => ((now.docs = l.docs.map((d) => ({ id: d.id, title: d.title }))), heard()), () => {});
}

/** Where you were in a project, in a few words: its place, or the board or
 * Doc you had open. */
export function leftAt(path: string | null): string {
  if (!path) return SECTION_LABEL.home;
  const [pathname, search] = path.split("?");
  const r = parse(pathname!, search ? `?${search}` : "");
  if (r.page === "board") return boards.title(r.board) ?? SECTION_LABEL.boards;
  if (r.page === "doc") return projectDocs.title(r.doc) ?? SECTION_LABEL.docs;
  if (r.page === "machines") return "Machines";
  const s = sectionOf(r);
  return s ? SECTION_LABEL[s] : SECTION_LABEL.home;
}

const at = (page: string, project: string) => pathOf({ page, project } as Route, names);

function items(scope: "all" | "projects"): GoItem[] {
  if (session.state !== "ready") return [];
  const counts = inboxCounts();
  const out: GoItem[] = [];
  const project = (p: { id: string; name: string }, archived = false): GoItem => {
    const n = counts.by.get(p.id) ?? 0;
    const kept = placeOf(p.id);
    return {
      key: `project/${p.id}`,
      kind: archived ? "archived" : "project",
      project: p.name,
      title: leftAt(kept),
      state: n ? "asks" : undefined,
      note: n ? `${n} for you` : undefined,
      run: () => go(kept ?? at("project", p.id)),
    };
  };
  const all = owners(session.projects);
  for (const o of all) out.push(...o.projects.map((p) => project(p)));
  for (const o of all) out.push(...o.archived.map((p) => project(p, true)));
  const here = session.project(barProject(current()) ?? currentProject() ?? undefined);
  const owned = ownedProjects().length > 0;
  const commands: GoItem[] = [
    // Every project's list: the rail's mark too, but a phone has no rail.
    { key: "project/all", kind: "list", title: "All projects", note: counts.all ? `${counts.all} for you` : undefined, run: () => go("/") },
    { key: "project/new", kind: "command", title: "New project…", command: true, run: () => openNewProject("new", here?.id) },
    ...(owned ? [{ key: "project/new-from", kind: "command", title: "New project from…", command: true, run: () => openNewProject("from", here?.id) }] : []),
  ];
  if (scope === "projects") return [...out, ...commands];

  // Where you were lately, across projects (the back stack), newest first.
  const recent: GoItem[] = recentPlaces()
    .slice(0, 9)
    .map((r) => ({ key: `recent/${r.path}`, kind: "recent", project: session.project(r.project ?? undefined)?.name, title: leftAt(r.path), recent: true, run: () => go(r.path) }));

  // The project you're in: its places.
  if (here) {
    const place = (key: string, title: string, to: string) => out.push({ key: `place/${here.id}/${key}`, kind: "place", project: here.name, title, run: () => go(to) });
    place("overview", "Overview", at("project", here.id));
    place("boards", "Boards", at("boards", here.id));
    place("docs", "Docs", at("docs", here.id));
    place("machines", "Machines", pathOf({ page: "machines", project: here.id, at: { at: "machines" } }, names));
    place("chat", "Chat", chatPath(here.id));
  }
  // Each of Settings' scopes, here's project and its team (Lift 7).
  for (const sp of settingsPlaces()) out.push({ key: `place/settings/${sp.key}`, kind: "place", title: sp.title, run: () => go(pathOf(sp.to, names)) });
  out.push({ key: "place/inbox", kind: "place", title: "Inbox, every project", note: counts.all ? `${counts.all} for you` : undefined, run: () => go("/inbox") });
  // Every project's boards and Docs.
  for (const o of all)
    for (const p of o.projects) {
      read(p.id);
      const l = lists.get(p.id)!;
      for (const b of l.boards) out.push({ key: `board/${b.id}`, kind: "board", project: p.name, title: b.title, run: () => go(pathOf({ page: "board", project: p.id, board: b.id }, names)) });
      for (const d of l.docs) out.push({ key: `doc/${d.id}`, kind: "doc", project: p.name, title: d.title, run: () => go(pathOf({ page: "doc", project: p.id, doc: d.id }, names)) });
    }
  out.unshift(...recent);
  out.push(
    ...commands,
    { key: "account/feedback", kind: "command", title: "Send feedback…", command: true, run: openPlatformFeedback },
    { key: "account/sign-out", kind: "command", title: "Sign out of this page", command: true, danger: true, run: () => void signOut().then(() => location.reload()) },
  );
  return out;
}

export const platformGoTo: GoSource = {
  items,
  subscribe: (fn) => {
    listeners.add(fn);
    const off = [session.subscribe(fn), subscribeInbox(fn)];
    return () => {
      listeners.delete(fn);
      off.forEach((f) => f());
    };
  },
};
