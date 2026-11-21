// The platform page's routes (M150), as paths: real paths, not the shell's hash
// fragments ("Platform URLs are ids with cosmetic slugs"). A path names
// things by id, with a slug after it for people: `/p/prj_8f2k…-arugula`.
// Only the id is read, so a rename never breaks a link, and links are
// written with the current slug. Secrets never ride in these paths ("Secrets
// stay in fragments"): they stay in the shell's fragments.
//
//   /                              the projects (`?to=docs`, `boards`, `chat`,
//                                  `machines` or `projects`: the shell's
//                                  buttons, on to that place in the current
//                                  project)
//   /p/:project                    a project's home: the Overview, at its
//                                  Project level (Arugula #1287)
//   /p/:project/board/:board       the Overview zoomed to a board (its
//                                  Board level)
//   /p/:project/board/:board/card/:row
//                                  zoomed to a card's work (its Work level)
//   /p/:project/work/:session      zoomed to a session on no card, by its
//                                  key on the Overview
//   /p/:project/chat               its chat
//   /p/:project/chat/:room         a room in it (`?msg=N`: at that message)
//   /p/:project/chat/activity      Activity
//   /p/:project/chat/saved         Saved
//   /p/:project/chat/search?q=…    search (`&all=1`: every team's)
//   /p/:project/dm/:room           a direct message, in its team's project
//                                  (UI audit #169)
//   /p/:project/issues             its issues (M201): `?state=closed`,
//                                  `?q=…`, `?label=…`, `?assignee=…` narrow it
//   /p/:project/issues/new         a new issue
//   /p/:project/issues/:issue      an issue (`?msg=N`: at that comment)
//   /p/:project/triage             the Boards page, scrolled to Not on a board (#292, #656)
//   /p/:project/boards             its boards
//   /p/:project/boards/:board      a board (`brd_…`)
//   /boards                        every project's boards, a group a project
//   /boards/:board                 the same, that board selected (Arugula #1131)
//   /docs                          every Doc the reader can read, their own
//                                  private ones first (#418)
//   /p/:project/docs               its Docs (aru-118)
//   /p/:project/docs/:doc          a Doc (`pdc_…`)
//   /p/:project/runs               its runs (M215): `?state=…`, `?trigger=…`,
//                                  `?spec=…` narrow it
//   /p/:project/runs/:run          a run (`?msg=N`: at that message in its
//                                  thread)
//   /p/:project/sessions           its Boards: the Sessions page folded
//                                  into the boards' rows (Arugula #1082)
//   /p/:project/sessions/:ses      an agent session (M257): its card and
//                                  history
//   /p/:project/workspaces         its hosted workspaces
//   /p/:project/workspaces/:id     a hosted workspace (`hw_…`)
//   /p/:project/settings           its settings: now `/settings/project/:project`
//   /p/:project/settings/move      "Move to project…" (Arugula #971): some of
//                                  its work into a new or existing project
//   /p/:project/deploys            its deploys, newest first (Discovery 5)
//   /p/:project/deploys/new        "Deploy…" (M252): plan a deploy and ask for
//                                  its approval (`?env=env_…` picks one)
//   /installed?state=…             back from installing the GitHub App (its
//                                  setup URL): on to the project's settings
//   /inbox                         the reader's inbox (M171): items that want
//                                  something of them, and drafts waiting
//   /settings                      Settings (Arugula #1104): one page of four
//                                  scopes, each a list of sections and the
//                                  one chosen beside it. You, at `/settings`;
//   /settings/you/:section         a section of yours
//   /settings/machine/:machine     a machine's (`/settings/machine`: the
//                                  first machine), with `/:section` after
//   /settings/project/:project     a project's (`?link=OWNER/REPO` fills in
//                                  one to link, in Linked repos)
//   /settings/team/:owner          a team's, or Personal's (`personal`):
//                                  members, plan and billing, defaults
//                                  (each with `/:section` after it)
//   /design                        the design reference (#693): every shared
//                                  piece of the UI, drawn by the real
//                                  components (`#type`, `#chips`… are its
//                                  sections); no project, no data
//   /deploys/:deploy               a deploy (M218): its card, approvals
//                                  and log; a push for one opens here
//   /t/:team/chat                  a team's own page (Arugula #895): its
//   /t/:team/members               channels and its members; the team's
//                                  id, with its slug
//
// Old addresses for Settings land in its scopes: `/p/:project/settings`,
// `/settings/personal` and `/t/:team/settings` (the page puts the new path
// in their place).
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

import { machinesPath, parseMachines, type MachineAt } from "../shared/machine-paths.ts";
import type { OverviewZoom } from "../shared/platform-api.tsx";

/** The first path segments the routes above start with (and `/`). The
 * platform server answers any other with this page as a 404 (UI audit
 * #193): its copy is `PAGES` in crates/platform-api/src/page.rs, which a test
 * there checks against this list and `parse`. A new top-level route goes
 * in both. */
export const TOP = ["p", "chat", "dm", "issues", "runs", "installed", "inbox", "settings", "deploys", "boards", "docs", "design", "t", "machines"] as const;

export type Route =
  /** `to`: a shell button's place, to open in the current project (./current-project). */
  | { page: "home"; to?: "projects" | "docs" | "boards" | "chat" | "machines" }
  /** The Overview; with `board` and `card`, that card's drawer open over
   * it; with `zoom`, zoomed in to a board or some work (Arugula #1287). */
  | { page: "project"; project: string; board?: string; card?: string; zoom?: OverviewZoom }
  | { page: "issues"; project: string; state?: "closed"; q?: string; label?: string; assignee?: string }
  | { page: "new-issue"; project: string }
  | { page: "issue"; project: string; issue: string; msg?: number; thread?: number }
  | { page: "triage"; project: string }
  | { page: "boards"; project: string }
  | { page: "board"; project: string; board: string }
  | { page: "docs"; project?: string; all?: true }
  | { page: "doc"; project: string; doc: string }
  | { page: "runs"; project: string; state?: string; trigger?: string; spec?: string }
  | { page: "run"; project: string; run: string; msg?: number; thread?: number }
  | { page: "sessions"; project: string }
  | { page: "session"; project: string; session: string }
  | { page: "chat"; project: string; room?: string; msg?: number; thread?: number; view?: "activity" | "saved" | "search" | "threads"; q?: string; all?: boolean }
  | { page: "dm"; project: string; room: string; msg?: number; thread?: number }
  | { page: "workspaces"; project: string }
  | { page: "workspace"; project: string; workspace: string }
  | { page: "move"; project: string }
  | { page: "deploys"; project: string }
  | { page: "new-deploy"; project: string; env?: string }
  | { page: "installed"; project?: string; repo?: string }
  | { page: "inbox"; all?: true }
  | { page: "all-boards"; board?: string; all?: true }
  /** Settings (Arugula #1104), at one of its scopes and, with `section`,
   * that section of it. */
  | SettingsRoute
  | { page: "design" }
  | { page: "deploy"; deploy: string }
  | { page: "team"; team: string; tab: TeamTab }
  /** The machines, a machine's page or Swarm, in a project (Arugula #989). */
  | { page: "machines"; project: string; at: MachineAt }
  /** An id-only address, to be put where it belongs (`resolve`). */
  | { page: "find"; what: "chat"; room?: string; msg?: number; thread?: number; view?: "activity" | "saved" | "search" | "threads"; q?: string; all?: boolean }
  | { page: "find"; what: "issue"; issue: string; msg?: number; thread?: number }
  | { page: "find"; what: "run"; run: string; msg?: number; thread?: number }
  | { page: "find"; what: "machines"; at: MachineAt }
  | { page: "missing"; path: string };

/** Settings' scopes (Arugula #1104): yours, a machine's, a project's, and
 * an owner's (a team's or Personal's), each with the section chosen. */
export type SettingsRoute =
  | { page: "settings"; scope: "you"; section?: string }
  /** No `machine`: the first one the page knows. */
  | { page: "settings"; scope: "machine"; machine?: string; section?: string }
  /** `link`: a repo to fill in, to link (back from installing the GitHub App). */
  | { page: "settings"; scope: "project"; project: string; section?: string; link?: string }
  /** `owner`: a team's id, or `personal`. */
  | { page: "settings"; scope: "team"; owner: string; section?: string };

export type SettingsScopeKey = SettingsRoute["scope"];

/** A section's id, as the address says it. */
const SECTION = /^[a-z][a-z0-9-]{0,47}$/;

/** A machine's id at the front of a path segment, before its slug: a
 * daemon's id is hex. */
const MACHINE_ID = /^([0-9a-f]{6,64})(?:-.*)?$/;

/** A team page's tabs. Its Settings are Settings' team scope (Arugula
 * #1104), a tab that goes there. */
export const TEAM_TABS = ["chat", "members"] as const;
export type TeamTab = (typeof TEAM_TABS)[number];

/** A team's id at the front of a path segment, before its slug: a team's
 * id has no prefix (docs/data-model.md). */
const TEAM = /^([A-Za-z0-9_]{1,64})(?:-.*)?$/;

/** An id at the front of a path segment, before its slug: `prj_8f2k`,
 * `ch_…`, `dm_…`, `hw_…`, `iss_…`, `run_…`. */
const ID = /^([a-z]{2,3}_[0-9a-f]{4,64})(?:-.*)?$/;
/** A board's and a row's ids, as the Overview's open card names them. */
const BOARD_ID = /^brd_[\w-]{1,64}$/;
const ROW_ID = /^row_[\w-]{1,64}$/;

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

/** The slug `path` has after `id`, if it has one. */
export function slugIn(path: string, id: string): string | undefined {
  const segment = path.split("/").find((s) => idOf(s) === id);
  return segment?.slice(id.length + 1) || undefined;
}

/** The route a location names. `hash`: only Settings reads it, an old
 * link's section (`/settings#tokens`) where the path names none. */
export function parse(path: string, search = "", hash = ""): Route {
  const parts = path.split("/").filter(Boolean);
  const q = new URLSearchParams(search);
  const at = q.get("msg");
  // A message to show, when the link names one.
  const msg = at !== null && /^-?\d+$/.test(at) ? { msg: Number(at) } : {};
  // #106: the thread a message is in, beside its room.
  const inThread = q.get("thread");
  const thread = inThread !== null && /^\d+$/.test(inThread) ? { thread: Number(inThread) } : {};
  const missing: Route = { page: "missing", path };
  if (parts.length === 0) {
    const to = q.get("to");
    return to === "projects" || to === "docs" || to === "boards" || to === "chat" || to === "machines" ? { page: "home", to } : { page: "home" };
  }
  // `?all=1` opens these across every project, not the current one's (Arugula #881).
  const all = q.get("all") === "1" ? { all: true as const } : {};
  if (parts.length === 1 && parts[0] === "inbox") return { page: "inbox", ...all };
  if (parts.length === 1 && parts[0] === "boards") return { page: "all-boards", ...all };
  if (parts.length === 2 && parts[0] === "boards") {
    const board = idOf(parts[1]);
    return board?.startsWith("brd_") ? { page: "all-boards", board, ...all } : missing;
  }
  if (parts.length === 1 && parts[0] === "docs") return { page: "docs", ...all };
  if (parts[0] === "settings") return withOldSection(settingsRoute(parts.slice(1), q), hash) ?? missing;
  if (parts.length === 1 && parts[0] === "design") return { page: "design" };
  if (parts[0] === "chat" && parts.length <= 2) {
    if (parts.length === 1) return { page: "find", what: "chat" };
    if (parts[1] === "activity" || parts[1] === "saved") return { page: "find", what: "chat", view: parts[1] };
    if (parts[1] === "threads") return { page: "find", what: "chat", view: "threads" };
    if (parts[1] === "search") return { page: "find", what: "chat", view: "search", q: q.get("q") ?? "", ...(q.get("all") === "1" && { all: true }) };
    const room = idOf(parts[1]);
    return room ? { page: "find", what: "chat", room, ...msg, ...thread } : missing;
  }
  if (parts[0] === "issues" && parts.length === 2) {
    const issue = idOf(parts[1]);
    return issue?.startsWith("iss_") ? { page: "find", what: "issue", issue, ...msg, ...thread } : missing;
  }
  if (parts[0] === "runs" && parts.length === 2) {
    const run = idOf(parts[1]);
    return run?.startsWith("run_") ? { page: "find", what: "run", run, ...msg, ...thread } : missing;
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
  if (parts[0] === "machines") {
    const at = parseMachines(parts.slice(1), q);
    return at ? { page: "find", what: "machines", at } : missing;
  }
  if (parts[0] === "t" && parts.length === 3) {
    const team = TEAM.exec(parts[1])?.[1];
    // Its Settings tab, from before they were Settings' (Arugula #1104).
    if (team && parts[2] === "settings") return { page: "settings", scope: "team", owner: team };
    const tab = TEAM_TABS.find((t) => t === parts[2]);
    return team && tab ? { page: "team", team, tab } : missing;
  }
  if (parts[0] !== "p" || parts.length < 2) return missing;
  const project = idOf(parts[1]);
  if (!project) return missing;
  // The Overview's own name for it, as the rail has it (Arugula #989).
  if (parts.length === 2 || (parts.length === 3 && parts[2] === "overview")) {
    const board = q.get("board");
    const card = q.get("card");
    return board && card && BOARD_ID.test(board) && ROW_ID.test(card) ? { page: "project", project, board, card } : { page: "project", project };
  }
  // The Overview, zoomed in (Arugula #1287).
  if (parts[2] === "board" && (parts.length === 4 || (parts.length === 6 && parts[4] === "card"))) {
    // An id with its slug, or any board id the query form takes.
    const board = idOf(parts[3]) ?? (BOARD_ID.test(parts[3]) ? parts[3] : null);
    if (!board?.startsWith("brd_")) return missing;
    if (parts.length === 4) return { page: "project", project, zoom: { board } };
    return ROW_ID.test(parts[5]) ? { page: "project", project, zoom: { board, card: parts[5] } } : missing;
  }
  if (parts[2] === "work" && parts.length === 4) {
    let session = "";
    try {
      session = decodeURIComponent(parts[3]);
    } catch {
      // Not a key, then.
    }
    return session && session.length <= 200 ? { page: "project", project, zoom: { session } } : missing;
  }
  if (parts[2] === "machines") {
    const at = parseMachines(parts.slice(3), q);
    return at ? { page: "machines", project, at } : missing;
  }
  // Its settings, from before they were Settings' (Arugula #1104).
  if (parts.length === 3 && parts[2] === "settings") return withOldSection(settingsRoute(["project", project], q), hash) ?? missing;
  if (parts.length === 4 && parts[2] === "settings" && parts[3] === "move") return { page: "move", project };
  if (parts.length === 3 && parts[2] === "deploys") return { page: "deploys", project };
  if (parts.length === 4 && parts[2] === "deploys" && parts[3] === "new") {
    const env = q.get("env");
    return { page: "new-deploy", project, ...(env && /^env_[0-9a-f]{32}$/.test(env) ? { env } : {}) };
  }
  if (parts[2] === "chat") {
    if (parts.length === 3) return { page: "chat", project };
    if (parts.length === 4 && (parts[3] === "activity" || parts[3] === "saved")) return { page: "chat", project, view: parts[3] };
    if (parts.length === 4 && parts[3] === "threads") return { page: "chat", project, view: "threads" };
    if (parts.length === 4 && parts[3] === "search") return { page: "chat", project, view: "search", q: q.get("q") ?? "", ...(q.get("all") === "1" && { all: true }) };
    const room = parts.length === 4 ? idOf(parts[3]) : null;
    return room ? { page: "chat", project, room, ...msg, ...thread } : missing;
  }
  if (parts[2] === "dm" && parts.length === 4) {
    const room = idOf(parts[3]);
    return room ? { page: "dm", project, room, ...msg, ...thread } : missing;
  }
  if (parts[2] === "issues") return issueRoute(project, parts.slice(3), q, { ...msg, ...thread }) ?? missing;
  if (parts[2] === "triage" && parts.length === 3) return { page: "triage", project };
  if (parts[2] === "boards" && parts.length === 3) return { page: "boards", project };
  if (parts[2] === "boards" && parts.length === 4) {
    const board = idOf(parts[3]);
    return board?.startsWith("brd_") ? { page: "board", project, board } : missing;
  }
  if (parts[2] === "docs" && parts.length === 3) return { page: "docs", project };
  if (parts[2] === "docs" && parts.length === 4) {
    const doc = idOf(parts[3]);
    return doc?.startsWith("pdc_") ? { page: "doc", project, doc } : missing;
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
    return run?.startsWith("run_") ? { page: "run", project, run, ...msg, ...thread } : missing;
  }
  // An old link to the Sessions page: the page sends it on to Boards.
  if (parts[2] === "sessions" && parts.length === 3) return { page: "sessions", project };
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

/** `/settings…`, past `settings`, or null. */
function settingsRoute(rest: string[], q: URLSearchParams): SettingsRoute | null {
  const [scope, ...more] = rest;
  const section = (s: string | undefined): { section?: string } | null => (s === undefined ? {} : SECTION.test(s) ? { section: s } : null);
  if (scope === undefined) return { page: "settings", scope: "you" };
  // Personal's, from before (Arugula #897).
  if (scope === "personal" && more.length === 0) return { page: "settings", scope: "team", owner: "personal" };
  if (scope === "you" && more.length <= 1) {
    const s = section(more[0]);
    return s && youSection(s.section);
  }
  if (more.length > 2) return null;
  const s = section(more[1]);
  if (!s) return null;
  if (scope === "machine") {
    if (more.length === 0) return { page: "settings", scope: "machine" };
    const machine = MACHINE_ID.exec(more[0])?.[1];
    return machine ? { page: "settings", scope: "machine", machine, ...s } : null;
  }
  if (scope === "project") {
    const project = more[0] === undefined ? null : idOf(more[0]);
    if (!project) return null;
    // A repo to link opens Linked repos, to link it.
    const link = q.get("link");
    return link && REPO.test(link) ? { page: "settings", scope: "project", project, section: s.section ?? "repos", link } : { page: "settings", scope: "project", project, ...s };
  }
  if (scope === "team") {
    const owner = more[0] === undefined ? null : TEAM.exec(more[0])?.[1];
    return owner ? { page: "settings", scope: "team", owner, ...s } : null;
  }
  return null;
}

/** A section of yours, or where it went: your machines' agents are each
 * machine's now (Arugula #1107), in Machines' Agents, and Preferences
 * is part of Appearance (Arugula #1318). */
function youSection(section?: string): SettingsRoute {
  if (section === "agents") return { page: "settings", scope: "machine", section };
  if (section === "preferences") return { page: "settings", scope: "you", section: "appearance" };
  return { page: "settings", scope: "you", ...(section ? { section } : {}) };
}

/** An old link's `#section`, where the path names none: the page puts the
 * section in the path, in its place. A project's or a machine's section
 * that's part of another now lands on that one. */
function withOldSection(r: SettingsRoute | null, hash: string): SettingsRoute | null {
  const section = /^#([a-z][a-z0-9-]{0,47})$/.exec(hash)?.[1];
  const at = r && !r.section && section ? (r.scope === "you" ? youSection(section) : { ...r, section }) : r;
  const moved = at?.scope === "project" || at?.scope === "machine" ? MOVED[at.scope] : undefined;
  return at?.section && moved && Object.hasOwn(moved, at.section) ? ({ ...at, section: moved[at.section] } as SettingsRoute) : at;
}

/** Sections that are parts of others now: a project's (Arugula #1105),
 * and a machine's, in Machines' four (the Settings Doc's Machines tab).
 * The section each is in now. */
const MOVED: Record<"project" | "machine", Record<string, string>> = {
  project: {
    plane: "repos",
    boards: "issues",
    size: "runs",
    finish: "runs",
    "agent-model": "agents",
    environments: "secrets",
  },
  machine: {
    "guest-ssh": "access",
    "remote-sessions": "access",
    rules: "agents",
    grants: "agents",
    reports: "advanced",
    developer: "advanced",
  },
};

/** The path for a settings route, with each id's current slug (`s`). */
function settingsPath(r: SettingsRoute, s: (id: string) => string): string {
  const at = r.section ? `/${r.section}` : "";
  switch (r.scope) {
    case "you":
      return r.section ? `/settings/you${at}` : "/settings";
    case "machine":
      // No machine yet (the page picks the first): the section waits in the hash.
      return r.machine ? `/settings/machine/${s(r.machine)}${at}` : `/settings/machine${r.section ? `#${r.section}` : ""}`;
    case "project":
      return `/settings/project/${s(r.project)}${at}${r.link ? `?link=${encodeURIComponent(r.link)}` : ""}`;
    case "team":
      return `/settings/team/${r.owner === "personal" ? "personal" : s(r.owner)}${at}`;
  }
}

/** `/p/:project/issues…`, past the project, or null. */
function issueRoute(project: string, rest: string[], q: URLSearchParams, msg: { msg?: number; thread?: number }): Route | null {
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

/** An id-only chat view's route in a project: its view, and search's query
 * and `all` with it (Arugula #908). */
export function chatViewIn(r: Extract<Route, { page: "find"; what: "chat" }>, project: string): Route {
  if (!r.view) return { page: "chat", project };
  return { page: "chat", project, view: r.view, q: r.q, ...(r.all && { all: true }) };
}

/** Search's query string: what was looked for, and `all` for every team's. */
function searchQuery(r: { q?: string; all?: boolean }): string {
  const parts = [r.q ? `q=${encodeURIComponent(r.q)}` : "", r.all ? "all=1" : ""].filter(Boolean);
  return parts.length ? `?${parts.join("&")}` : "";
}

/** A room's `?msg=N&thread=P`: the message to show, and the thread beside it
 * (#106); an issue's or a run's comments too. */
function at(r: { msg?: number; thread?: number }): string {
  const q = [r.msg !== undefined ? `msg=${r.msg}` : "", r.thread !== undefined ? `thread=${r.thread}` : ""].filter(Boolean);
  return q.length ? `?${q.join("&")}` : "";
}

/** A machine's page's slug: its name, when known. */
function machineSlug(at: MachineAt, names: Names): string | null {
  const name = at.at === "machine" ? names(at.machine) : null;
  return name ? slugOf(name) || null : null;
}

/** The path for a route, with each id's current slug. */
export function pathOf(r: Route, names: Names = () => null): string {
  const s = (id: string) => encodeURI(withSlug(id, names(id)));
  switch (r.page) {
    case "home":
      return "/";
    case "project":
      if (r.zoom && "session" in r.zoom) return `/p/${s(r.project)}/work/${encodeURIComponent(r.zoom.session)}`;
      // A board's slug only where its id reads back from it (`idOf`).
      if (r.zoom) return `/p/${s(r.project)}/board/${idOf(r.zoom.board) === r.zoom.board ? s(r.zoom.board) : encodeURIComponent(r.zoom.board)}${r.zoom.card ? `/card/${encodeURIComponent(r.zoom.card)}` : ""}`;
      return `/p/${s(r.project)}${r.board && r.card ? `?board=${encodeURIComponent(r.board)}&card=${encodeURIComponent(r.card)}` : ""}`;
    case "chat": {
      const base = `/p/${s(r.project)}/chat`;
      if (r.view === "activity" || r.view === "saved" || r.view === "threads") return `${base}/${r.view}`;
      if (r.view === "search") return `${base}/search${searchQuery(r)}`;
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
    case "triage":
      return `/p/${s(r.project)}/triage`;
    case "boards":
      return `/p/${s(r.project)}/boards`;
    case "board":
      return `/p/${s(r.project)}/boards/${s(r.board)}`;
    case "docs":
      return r.project ? `/p/${s(r.project)}/docs` : r.all ? "/docs?all=1" : "/docs";
    case "doc":
      return `/p/${s(r.project)}/docs/${s(r.doc)}`;
    case "runs": {
      const q = new URLSearchParams();
      if (r.state) q.set("state", r.state);
      if (r.trigger) q.set("trigger", r.trigger);
      if (r.spec) q.set("spec", r.spec);
      const qs = q.toString();
      return `/p/${s(r.project)}/runs${qs ? `?${qs}` : ""}`;
    }
    case "run":
      return `/p/${s(r.project)}/runs/${s(r.run)}${at(r)}`;
    case "sessions":
      return `/p/${s(r.project)}/sessions`;
    case "session":
      return `/p/${s(r.project)}/sessions/${r.session}`;
    case "issue":
      return `/p/${s(r.project)}/issues/${s(r.issue)}${at(r)}`;
    case "dm":
      return `/p/${s(r.project)}/dm/${s(r.room)}${at(r)}`;
    case "workspaces":
      return `/p/${s(r.project)}/workspaces`;
    case "workspace":
      return `/p/${s(r.project)}/workspaces/${s(r.workspace)}`;
    case "move":
      return `/p/${s(r.project)}/settings/move`;
    case "deploys":
      return `/p/${s(r.project)}/deploys`;
    case "new-deploy":
      return `/p/${s(r.project)}/deploys/new${r.env ? `?env=${r.env}` : ""}`;
    case "installed":
      return r.project && r.repo ? `/installed?state=${encodeURIComponent(`${r.project}:${r.repo}`)}` : "/installed";
    case "inbox":
      return r.all ? "/inbox?all=1" : "/inbox";
    case "all-boards":
      return `/boards${r.board ? `/${s(r.board)}` : ""}${r.all ? "?all=1" : ""}`;
    case "settings":
      return settingsPath(r, s);
    case "design":
      return "/design";
    case "deploy":
      return `/deploys/${r.deploy}`;
    case "team":
      return `/t/${s(r.team)}/${r.tab}`;
    case "machines":
      return `/p/${s(r.project)}${machinesPath(r.at, machineSlug(r.at, names))}`;
    case "find":
      if (r.what === "chat") {
        if (r.view === "activity" || r.view === "saved" || r.view === "threads") return `/chat/${r.view}`;
        if (r.view === "search") return `/chat/search${searchQuery(r)}`;
        return r.room ? `/chat/${s(r.room)}${at(r)}` : "/chat";
      }
      if (r.what === "issue") return `/issues/${s(r.issue)}${at(r)}`;
      if (r.what === "machines") return machinesPath(r.at, machineSlug(r.at, names));
      return `/runs/${s(r.run)}${at(r)}`;
    case "missing":
      return r.path;
  }
}
