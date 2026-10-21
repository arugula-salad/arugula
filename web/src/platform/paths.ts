// The platform page's routes (M150), as paths: real paths, not the shell's hash
// fragments ("Platform URLs are ids with cosmetic slugs"). A path names
// things by id, with a slug after it for people: `/p/prj_8f2k…-arugula`.
// Only the id is read, so a rename never breaks a link, and links are
// written with the current slug. Secrets never ride in these paths ("Secrets
// stay in fragments"): they stay in the shell's fragments.
//
//   /                              the projects
//   /p/:project                    a project's home
//   /p/:project/chat               its chat
//   /p/:project/chat/:room         a room in it (`?msg=N`: at that message)
//   /p/:project/chat/activity      Activity
//   /p/:project/chat/search?q=…    search
//   /dm/:room                      a direct message
//   /p/:project/workspaces         its Cloud workspaces
//   /p/:project/workspaces/:id     a Cloud workspace (`/:tab` for a tab)
//   /p/:project/past/:id           a past run (`run_…`) or a closed tab
//                                  (`cwc_…`), read only
//   /p/:project/settings           its settings: linked repos (`?link=OWNER/REPO`
//                                  fills in one to link)
//   /installed?state=…             back from installing the GitHub App (its
//                                  setup URL): on to the project's settings
//   /agents                        the cloud agent library
//   /settings                      account settings
//
// And addresses that name only an id, for links from places that don't
// know its project (the shell, an old link, a push): the page finds the
// project once it knows the reader's, and puts the full path in their place.
//
//   /chat, /chat/:room             chat (the first team's), a room in it
//   /chat/activity, /chat/search?q=…
//   /workspaces, /workspaces/:id[/:tab], /past/:id
//                                  the reader's own Cloud workspaces
//
// Routes for the inbox, issues and changes wait for steps 4 and 6 rather
// than shipping empty.

export type Route =
  | { page: "home" }
  | { page: "project"; project: string }
  | { page: "chat"; project: string; room?: string; msg?: number; view?: "activity" | "search"; q?: string }
  | { page: "dm"; room: string; msg?: number }
  | { page: "workspaces"; project: string }
  | { page: "workspace"; project: string; workspace: string; tab?: string }
  | { page: "past"; project: string; id: string }
  | { page: "project-settings"; project: string; link?: string }
  | { page: "installed"; project?: string; repo?: string }
  | { page: "agents" }
  | { page: "settings" }
  /** An id-only address, to be put where it belongs (`resolve`). */
  | { page: "find"; what: "chat"; room?: string; msg?: number; view?: "activity" | "search"; q?: string }
  | { page: "find"; what: "workspaces"; workspace?: string; tab?: string }
  | { page: "find"; what: "past"; id: string }
  | { page: "missing"; path: string };

/** An id at the front of a path segment, before its slug: `prj_8f2k`,
 * `ch_…`, `dm_…`, `cw_…`, `cwc_…`. */
const ID = /^([a-z]{2,3}_[0-9a-f]{4,64})(?:-.*)?$/;

/** `OWNER/NAME` on GitHub. */
export const REPO = /^[A-Za-z0-9_-][A-Za-z0-9_.-]{0,99}\/[A-Za-z0-9_-][A-Za-z0-9_.-]{0,99}$/;

/** The id in a path segment, or null when it doesn't start with one. */
export function idOf(segment: string): string | null {
  let s = segment;
  try {
    s = decodeURIComponent(segment);
  } catch {
    // as it came
  }
  return ID.exec(s)?.[1] ?? null;
}

/** A name as a slug: lowercase words joined by hyphens, at most 40
 * characters, "" when nothing's left. */
export function slugOf(name: string): string {
  return name
    .normalize("NFKD")
    .replace(/[̀-ͯ]/g, "")
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "")
    .slice(0, 40)
    .replace(/-+$/, "");
}

/** An id with its slug, for a path. */
export function withSlug(id: string, name?: string | null): string {
  const slug = name ? slugOf(name) : "";
  return slug ? `${id}-${slug}` : id;
}

/** The route a location names. */
export function parse(path: string, search = ""): Route {
  const parts = path.split("/").filter(Boolean);
  const q = new URLSearchParams(search);
  const at = q.get("msg");
  // A message to show, when the link names one.
  const msg = at !== null && /^-?\d+$/.test(at) ? { msg: Number(at) } : {};
  const missing: Route = { page: "missing", path };
  if (parts.length === 0) return { page: "home" };
  if (parts.length === 1 && parts[0] === "agents") return { page: "agents" };
  if (parts.length === 1 && parts[0] === "settings") return { page: "settings" };
  if (parts[0] === "chat" && parts.length <= 2) {
    if (parts.length === 1) return { page: "find", what: "chat" };
    if (parts[1] === "activity") return { page: "find", what: "chat", view: "activity" };
    if (parts[1] === "search") return { page: "find", what: "chat", view: "search", q: q.get("q") ?? "" };
    const room = idOf(parts[1]);
    return room ? { page: "find", what: "chat", room, ...msg } : missing;
  }
  if (parts[0] === "workspaces" && parts.length <= 3) {
    const workspace = parts[1] === undefined ? undefined : idOf(parts[1]);
    const tab = parts[2] === undefined ? undefined : idOf(parts[2]);
    if (workspace === null || tab === null) return missing;
    return { page: "find", what: "workspaces", ...(workspace ? { workspace } : {}), ...(tab ? { tab } : {}) };
  }
  if (parts[0] === "past" && parts.length === 2) {
    const id = idOf(parts[1]);
    return id && /^(run|cwc)_/.test(id) ? { page: "find", what: "past", id } : missing;
  }
  if (parts.length === 1 && parts[0] === "installed") {
    // The state linking handed GitHub: `<project>:<OWNER/NAME>`.
    const [p, repo] = (q.get("state") ?? "").split(/:(.*)/s);
    const project = idOf(p ?? "");
    return project && repo && REPO.test(repo) ? { page: "installed", project, repo } : { page: "installed" };
  }
  if (parts[0] === "dm" && parts.length === 2) {
    const room = idOf(parts[1]);
    return room ? { page: "dm", room, ...msg } : missing;
  }
  if (parts[0] !== "p" || parts.length < 2) return missing;
  const project = idOf(parts[1]);
  if (!project) return missing;
  if (parts.length === 2) return { page: "project", project };
  if (parts.length === 3 && parts[2] === "settings") {
    const link = q.get("link");
    return { page: "project-settings", project, ...(link && REPO.test(link) ? { link } : {}) };
  }
  if (parts[2] === "chat") {
    if (parts.length === 3) return { page: "chat", project };
    if (parts.length === 4 && parts[3] === "activity") return { page: "chat", project, view: "activity" };
    if (parts.length === 4 && parts[3] === "search") return { page: "chat", project, view: "search", q: q.get("q") ?? "" };
    const room = parts.length === 4 ? idOf(parts[3]) : null;
    return room ? { page: "chat", project, room, ...msg } : missing;
  }
  if (parts[2] === "workspaces" && parts.length === 3) return { page: "workspaces", project };
  if (parts[2] === "past" && parts.length === 4) {
    const id = idOf(parts[3]);
    return id && /^(run|cwc)_/.test(id) ? { page: "past", project, id } : missing;
  }
  if (parts[2] === "workspaces" && (parts.length === 4 || parts.length === 5)) {
    const workspace = idOf(parts[3]);
    const tab = parts[4] === undefined ? undefined : idOf(parts[4]);
    if (!workspace || tab === null) return missing;
    return { page: "workspace", project, workspace, ...(tab ? { tab } : {}) };
  }
  return missing;
}

/** What the page's links name things with: each id's current name, for
 * its slug. */
export type Names = (id: string) => string | null | undefined;

/** The path for a route, with each id's current slug. */
export function pathOf(r: Route, names: Names = () => null): string {
  const s = (id: string) => encodeURI(withSlug(id, names(id)));
  switch (r.page) {
    case "home":
      return "/";
    case "project":
      return `/p/${s(r.project)}`;
    case "chat": {
      const base = `/p/${s(r.project)}/chat`;
      if (r.view === "activity") return `${base}/activity`;
      if (r.view === "search") return `${base}/search${r.q ? `?q=${encodeURIComponent(r.q)}` : ""}`;
      return r.room ? `${base}/${s(r.room)}${r.msg !== undefined ? `?msg=${r.msg}` : ""}` : base;
    }
    case "dm":
      return `/dm/${s(r.room)}${r.msg !== undefined ? `?msg=${r.msg}` : ""}`;
    case "workspaces":
      return `/p/${s(r.project)}/workspaces`;
    case "workspace":
      return `/p/${s(r.project)}/workspaces/${s(r.workspace)}${r.tab ? `/${s(r.tab)}` : ""}`;
    case "past":
      return `/p/${s(r.project)}/past/${s(r.id)}`;
    case "project-settings":
      return `/p/${s(r.project)}/settings${r.link ? `?link=${encodeURIComponent(r.link)}` : ""}`;
    case "installed":
      return r.project && r.repo ? `/installed?state=${encodeURIComponent(`${r.project}:${r.repo}`)}` : "/installed";
    case "agents":
      return "/agents";
    case "settings":
      return "/settings";
    case "find":
      if (r.what === "chat") {
        if (r.view === "activity") return "/chat/activity";
        if (r.view === "search") return `/chat/search${r.q ? `?q=${encodeURIComponent(r.q)}` : ""}`;
        return r.room ? `/chat/${s(r.room)}${r.msg !== undefined ? `?msg=${r.msg}` : ""}` : "/chat";
      }
      if (r.what === "past") return `/past/${s(r.id)}`;
      return `/workspaces${r.workspace ? `/${s(r.workspace)}${r.tab ? `/${s(r.tab)}` : ""}` : ""}`;
    case "missing":
      return r.path;
  }
}
