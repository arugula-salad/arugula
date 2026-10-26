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
//   /p/:project/chat/saved         Saved
//   /p/:project/chat/search?q=…    search
//   /p/:project/dm/:room           a direct message, in its team's project
//                                  (UI audit #169)
//   /p/:project/issues             its issues (M201): `?state=closed`,
//                                  `?q=…`, `?label=…`, `?assignee=…` narrow it
//   /p/:project/issues/new         a new issue
//   /p/:project/issues/:issue      an issue (`?msg=N`: at that comment)
//   /p/:project/boards             its boards (live status board), when the
//                                  `boards` Labs flag is on
//   /p/:project/boards/:board      a board (`brd_…`)
//   /p/:project/runs               its runs (M215): `?state=…`, `?trigger=…`,
//                                  `?spec=…` narrow it
//   /p/:project/runs/:run          a run (`?msg=N`: at that message in its
//                                  thread)
//   /p/:project/sessions/:ses      an agent session (M257): its card and
//                                  history
//   /p/:project/workspaces         its hosted workspaces
//   /p/:project/workspaces/:id     a hosted workspace (`hw_…`)
//   /p/:project/settings           its settings: linked repos (`?link=OWNER/REPO`
//                                  fills in one to link), environments (M252)
//   /p/:project/deploys/new        "Deploy…" (M252): plan a deploy and ask for
//                                  its approval (`?env=env_…` picks one)
//   /installed?state=…             back from installing the GitHub App (its
//                                  setup URL): on to the project's settings
//   /inbox                         the reader's inbox (M171): items that want
//                                  something of them, and drafts waiting
//   /settings                      account settings
//   /deploys/:deploy               a deploy (M218): its card, approvals
//                                  and log; a push for one opens here
//
// And addresses that name only an id, for links from places that don't
// know its project (the shell, an old link, a push): the page finds the
// project once it knows the reader's, and puts the full path in their place.
//
//   /chat, /chat/:room             chat (the first team's), a room in it
//   /chat/activity, /chat/saved, /chat/search?q=…
//   /dm/:room                      a direct message (its team's project)
//   /issues/:issue                 an issue (`?msg=N`: at that comment)
//   /runs/:run                     a run (a push for one that failed)
//
// Routes for changes wait for step 6 rather than shipping empty.

/** The first path segments the routes above start with (and `/`). The
 * platform server answers any other with this page as a 404 (UI audit
 * #193): its copy is `PAGES` in crates/platform/src/page.rs, which a test
 * there checks against this list and `parse`. A new top-level route goes
 * in both. */
export const TOP = ["p", "chat", "dm", "issues", "runs", "installed", "inbox", "settings", "deploys"] as const;

export type Route =
  | { page: "home" }
  | { page: "project"; project: string }
  | { page: "issues"; project: string; state?: "closed"; q?: string; label?: string; assignee?: string }
  | { page: "new-issue"; project: string }
  | { page: "issue"; project: string; issue: string; msg?: number }
  | { page: "boards"; project: string }
  | { page: "board"; project: string; board: string }
  | { page: "runs"; project: string; state?: string; trigger?: string; spec?: string }
  | { page: "run"; project: string; run: string; msg?: number }
  | { page: "session"; project: string; session: string }
  | { page: "chat"; project: string; room?: string; msg?: number; thread?: number; view?: "activity" | "saved" | "search" | "threads"; q?: string }
  | { page: "dm"; project: string; room: string; msg?: number; thread?: number }
  | { page: "workspaces"; project: string }
  | { page: "workspace"; project: string; workspace: string }
  | { page: "project-settings"; project: string; link?: string }
  | { page: "new-deploy"; project: string; env?: string }
  | { page: "installed"; project?: string; repo?: string }
  | { page: "inbox" }
  | { page: "settings" }
  | { page: "deploy"; deploy: string }
  /** An id-only address, to be put where it belongs (`resolve`). */
  | { page: "find"; what: "chat"; room?: string; msg?: number; thread?: number; view?: "activity" | "saved" | "search" | "threads"; q?: string }
  | { page: "find"; what: "issue"; issue: string; msg?: number }
  | { page: "find"; what: "run"; run: string; msg?: number }
  | { page: "missing"; path: string };

/** An id at the front of a path segment, before its slug: `prj_8f2k`,
 * `ch_…`, `dm_…`, `hw_…`, `iss_…`, `run_…`. */
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
  // #106: the thread a message is in, beside its room.
  const inThread = q.get("thread");
  const thread = inThread !== null && /^\d+$/.test(inThread) ? { thread: Number(inThread) } : {};
  const missing: Route = { page: "missing", path };
  if (parts.length === 0) return { page: "home" };
  if (parts.length === 1 && parts[0] === "inbox") return { page: "inbox" };
  if (parts.length === 1 && parts[0] === "settings") return { page: "settings" };
  if (parts[0] === "chat" && parts.length <= 2) {
    if (parts.length === 1) return { page: "find", what: "chat" };
    if (parts[1] === "activity" || parts[1] === "saved") return { page: "find", what: "chat", view: parts[1] };
    if (parts[1] === "threads") return { page: "find", what: "chat", view: "threads" };
    if (parts[1] === "search") return { page: "find", what: "chat", view: "search", q: q.get("q") ?? "" };
    const room = idOf(parts[1]);
    return room ? { page: "find", what: "chat", room, ...msg, ...thread } : missing;
  }
  if (parts[0] === "issues" && parts.length === 2) {
    const issue = idOf(parts[1]);
    return issue?.startsWith("iss_") ? { page: "find", what: "issue", issue, ...msg } : missing;
  }
  if (parts[0] === "runs" && parts.length === 2) {
    const run = idOf(parts[1]);
    return run?.startsWith("run_") ? { page: "find", what: "run", run, ...msg } : missing;
  }
  if (parts.length === 1 && parts[0] === "installed") {
    // The state linking handed GitHub: `<project>:<OWNER/NAME>`.
    const [p, repo] = (q.get("state") ?? "").split(/:(.*)/s);
    const project = idOf(p ?? "");
    return project && repo && REPO.test(repo) ? { page: "installed", project, repo } : { page: "installed" };
  }
  if (parts[0] === "deploys" && parts.length === 2 && /^dep_[0-9a-f]{32}$/.test(parts[1])) return { page: "deploy", deploy: parts[1] };
  if (parts[0] === "dm" && parts.length === 2) {
    // An id-only DM (UI audit #169): its team's project says where.
    const room = idOf(parts[1]);
    return room ? { page: "find", what: "chat", room, ...msg, ...thread } : missing;
  }
  if (parts[0] !== "p" || parts.length < 2) return missing;
  const project = idOf(parts[1]);
  if (!project) return missing;
  if (parts.length === 2) return { page: "project", project };
  if (parts.length === 3 && parts[2] === "settings") {
    const link = q.get("link");
    return { page: "project-settings", project, ...(link && REPO.test(link) ? { link } : {}) };
  }
  if (parts.length === 4 && parts[2] === "deploys" && parts[3] === "new") {
    const env = q.get("env");
    return { page: "new-deploy", project, ...(env && /^env_[0-9a-f]{32}$/.test(env) ? { env } : {}) };
  }
  if (parts[2] === "chat") {
    if (parts.length === 3) return { page: "chat", project };
    if (parts.length === 4 && (parts[3] === "activity" || parts[3] === "saved")) return { page: "chat", project, view: parts[3] };
    if (parts.length === 4 && parts[3] === "threads") return { page: "chat", project, view: "threads" };
    if (parts.length === 4 && parts[3] === "search") return { page: "chat", project, view: "search", q: q.get("q") ?? "" };
    const room = parts.length === 4 ? idOf(parts[3]) : null;
    return room ? { page: "chat", project, room, ...msg, ...thread } : missing;
  }
  if (parts[2] === "dm" && parts.length === 4) {
    const room = idOf(parts[3]);
    return room ? { page: "dm", project, room, ...msg, ...thread } : missing;
  }
  if (parts[2] === "issues") return issueRoute(project, parts.slice(3), q, msg) ?? missing;
  if (parts[2] === "boards" && parts.length === 3) return { page: "boards", project };
  if (parts[2] === "boards" && parts.length === 4) {
    const board = idOf(parts[3]);
    return board?.startsWith("brd_") ? { page: "board", project, board } : missing;
  }
  if (parts[2] === "runs" && parts.length === 3) {
    const word = (k: string, ok: RegExp) => {
      const v = q.get(k);
      return v && ok.test(v) ? { [k]: v } : {};
    };
    return { page: "runs", project, ...word("state", /^[a-z]{1,16}$/), ...word("trigger", /^[a-z]{1,16}$/), ...word("spec", /^[a-z]{2,4}_[0-9a-f]{4,64}$/) };
  }
  if (parts[2] === "runs" && parts.length === 4) {
    const run = idOf(parts[3]);
    return run?.startsWith("run_") ? { page: "run", project, run, ...msg } : missing;
  }
  if (parts[2] === "sessions" && parts.length === 4) {
    const session = idOf(parts[3]);
    return session?.startsWith("ses_") ? { page: "session", project, session } : missing;
  }
  if (parts[2] === "workspaces" && parts.length === 3) return { page: "workspaces", project };
  if (parts[2] === "workspaces" && parts.length === 4) {
    const workspace = idOf(parts[3]);
    return workspace?.startsWith("hw_") ? { page: "workspace", project, workspace } : missing;
  }
  return missing;
}

/** `/p/:project/issues…`, past the project, or null. */
function issueRoute(project: string, rest: string[], q: URLSearchParams, msg: { msg?: number }): Route | null {
  if (rest.length === 0) {
    const filters = {
      ...(q.get("state") === "closed" ? { state: "closed" as const } : {}),
      ...(q.get("q") ? { q: q.get("q")! } : {}),
      ...(q.get("label") ? { label: q.get("label")! } : {}),
      ...(q.get("assignee") ? { assignee: q.get("assignee")! } : {}),
    };
    return { page: "issues", project, ...filters };
  }
  if (rest.length !== 1) return null;
  if (rest[0] === "new") return { page: "new-issue", project };
  const issue = idOf(rest[0]);
  return issue?.startsWith("iss_") ? { page: "issue", project, issue, ...msg } : null;
}

/** What the page's links name things with: each id's current name, for
 * its slug. */
export type Names = (id: string) => string | null | undefined;

/** A room's `?msg=N&thread=P`: the message to show, and the thread beside it (#106). */
function at(r: { msg?: number; thread?: number }): string {
  const q = [r.msg !== undefined ? `msg=${r.msg}` : "", r.thread !== undefined ? `thread=${r.thread}` : ""].filter(Boolean);
  return q.length ? `?${q.join("&")}` : "";
}

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
      if (r.view === "activity" || r.view === "saved" || r.view === "threads") return `${base}/${r.view}`;
      if (r.view === "search") return `${base}/search${r.q ? `?q=${encodeURIComponent(r.q)}` : ""}`;
      return r.room ? `${base}/${s(r.room)}${at(r)}` : base;
    }
    case "issues": {
      const q = new URLSearchParams();
      if (r.state) q.set("state", r.state);
      if (r.q) q.set("q", r.q);
      if (r.label) q.set("label", r.label);
      if (r.assignee) q.set("assignee", r.assignee);
      const qs = q.toString();
      return `/p/${s(r.project)}/issues${qs ? `?${qs}` : ""}`;
    }
    case "new-issue":
      return `/p/${s(r.project)}/issues/new`;
    case "boards":
      return `/p/${s(r.project)}/boards`;
    case "board":
      return `/p/${s(r.project)}/boards/${s(r.board)}`;
    case "runs": {
      const q = new URLSearchParams();
      if (r.state) q.set("state", r.state);
      if (r.trigger) q.set("trigger", r.trigger);
      if (r.spec) q.set("spec", r.spec);
      const qs = q.toString();
      return `/p/${s(r.project)}/runs${qs ? `?${qs}` : ""}`;
    }
    case "run":
      return `/p/${s(r.project)}/runs/${s(r.run)}${r.msg !== undefined ? `?msg=${r.msg}` : ""}`;
    case "session":
      return `/p/${s(r.project)}/sessions/${r.session}`;
    case "issue":
      return `/p/${s(r.project)}/issues/${s(r.issue)}${r.msg !== undefined ? `?msg=${r.msg}` : ""}`;
    case "dm":
      return `/p/${s(r.project)}/dm/${s(r.room)}${at(r)}`;
    case "workspaces":
      return `/p/${s(r.project)}/workspaces`;
    case "workspace":
      return `/p/${s(r.project)}/workspaces/${s(r.workspace)}`;
    case "project-settings":
      return `/p/${s(r.project)}/settings${r.link ? `?link=${encodeURIComponent(r.link)}` : ""}`;
    case "new-deploy":
      return `/p/${s(r.project)}/deploys/new${r.env ? `?env=${r.env}` : ""}`;
    case "installed":
      return r.project && r.repo ? `/installed?state=${encodeURIComponent(`${r.project}:${r.repo}`)}` : "/installed";
    case "inbox":
      return "/inbox";
    case "settings":
      return "/settings";
    case "deploy":
      return `/deploys/${r.deploy}`;
    case "find":
      if (r.what === "chat") {
        if (r.view === "activity" || r.view === "saved" || r.view === "threads") return `/chat/${r.view}`;
        if (r.view === "search") return `/chat/search${r.q ? `?q=${encodeURIComponent(r.q)}` : ""}`;
        return r.room ? `/chat/${s(r.room)}${at(r)}` : "/chat";
      }
      if (r.what === "issue") return `/issues/${s(r.issue)}${r.msg !== undefined ? `?msg=${r.msg}` : ""}`;
      return `/runs/${s(r.run)}${r.msg !== undefined ? `?msg=${r.msg}` : ""}`;
    case "missing":
      return r.path;
  }
}
