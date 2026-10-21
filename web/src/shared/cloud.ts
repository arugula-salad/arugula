// Cloud agents, through control. Nothing about them is remembered locally:
// content and authorization always come from control, independently of any
// daemon. Cloud runs are retired (S40, M119): past ones stay readable at
// `#cloud=run_…`, and Cloud workspaces (below) are where agents run now.
import { useEffect, useState } from "preact/hooks";
import { maintenance, platformFetch, refusalOf } from "./maintenance";
import type { CloudRun, CloudTab, CloudTabs, CloudWorkspace, CloudWorkspaces } from "./proto.gen";
// Control's `/api/cloud/` types are generated from crates/cloud-wire
// (proto.gen.ts, `just proto-ts`, M137).
export type {
  AgentDefinition as Definition, CloudAgent, CloudModel, CloudRun, CloudSetupLog, CloudTab, CloudWorkspace, CloudWorkspaceChange,
  CloudWorkspaceRepository, CloudWorkspaceSpec, PastCloudTab,
} from "./proto.gen";
export const cloudTerminal = (run: CloudRun) => ["completed", "failed", "cancelled", "expired"].includes(run.state);
/** Control's `/api/cloud/`, waiting out a maintenance window (M141). */
export async function cloudApi<T>(path: string, method = "GET", body?: unknown): Promise<T> {
  const r = await platformFetch(`/api/cloud/${path}`, { method, credentials: "same-origin", headers: body === undefined ? {} : { "Content-Type": "application/json" }, body: body === undefined ? undefined : JSON.stringify(body) });
  const value = await r.json().catch(() => ({}));
  if (!r.ok) throw new Error(refusalOf(r.status, value));
  return value as T;
}
/** How a Cloud workspace view reaches control's `/api/cloud/`: on
 * control's page, `cloudApi` itself; in a Cloud workspace block (#1117),
 * the block's `cloud` method, which the daemon sends on as the machine.
 * Views get it from the page (`useCloudApi`, ./platform-api). */
export type CloudApi = <T>(path: string, method?: string, body?: unknown) => Promise<T>;
const listeners = new Set<() => void>();
const changed = () => listeners.forEach((fn) => fn());

/** Where Cloud's views are, by the page's own routes (M153): the shell's
 * `#cloud=…` and `#cloud-ws…` fragments (`hashRoutes`, the default), or
 * the platform page's paths, which it sets with `setCloudRoutes`. */
export interface CloudRoutes {
  /** The Cloud workspace (and tab) shown, both null for the list; null
   * when no Cloud workspace view is. */
  workspace(): { workspace: string | null; tab: string | null } | null;
  /** The past run (`run_…`) or past tab (`cwc_…`) shown. */
  past(): string | null;
  /** Show the list, a workspace, or one of its tabs. */
  openWorkspace(workspace?: string, tab?: string): void;
  /** Show a past run or a past tab. */
  openPast(id: string): void;
  /** Leave Cloud: back to the machine, or the project. */
  leave(): void;
}

/** The shell's: `#cloud=run_…`/`#cloud=cwc_…` for a past run or tab, and
 * `#cloud-ws`, `#cloud-ws=cw_…`, `#cloud-ws=cw_…/cwc_…`. */
export const hashRoutes: CloudRoutes = {
  workspace() {
    const m = /^#cloud-ws(?:=(cw_[0-9a-f]+)(?:\/(cwc_[0-9a-f]+))?)?$/.exec(location.hash);
    return m ? { workspace: m[1] ?? null, tab: m[2] ?? null } : null;
  },
  past: () => /^#cloud=((?:run|cwc)_[0-9a-f]+)$/.exec(location.hash)?.[1] ?? null,
  openWorkspace(workspace, tab) {
    location.hash = workspace ? `cloud-ws=${workspace}${tab ? `/${tab}` : ""}` : "cloud-ws";
  },
  openPast(id) {
    location.hash = `cloud=${id}`;
  },
  leave() {
    if (hashRoutes.past() || hashRoutes.workspace()) history.replaceState(null, "", location.pathname + location.search);
  },
};

let routes: CloudRoutes = hashRoutes;
/** The page's routes for Cloud (the platform page's paths), once, before
 * anything's drawn. Its own route changes call `cloudRouteChanged`. */
export function setCloudRoutes(r: CloudRoutes) {
  routes = r;
}
/** Tell Cloud's views the route changed. */
export const cloudRouteChanged = () => changed();

/** The shown past run or past tab. */
export const cloudRoute = () => routes.past();
addEventListener("hashchange", changed);
export const cloudPanes = {
  account: "", enabled: false,
  subscribe(fn: () => void) { listeners.add(fn); return () => { listeners.delete(fn); }; },
  /** Show a past run or a past tab. */
  open(id: string) { routes.openPast(id); changed(); },
  /** Back to the machine: leaves a past run or a Cloud workspace. */
  local() { if (cloudRoute() || cloudWorkspaceRoute()) { routes.leave(); changed(); } },
  connect(account: string) {
    this.account = account; this.enabled = false;
    forgetOpenedRuns();
    let active = true, timer: ReturnType<typeof setTimeout>;
    // Asked again after a maintenance window (M141) that outlasted the wait.
    const ask = () => void cloudApi("agents").then(() => { if (active) { this.enabled = true; changed(); } }, () => { if (active && maintenance.down) timer = setTimeout(ask, 15_000); });
    if (account) ask();
    changed();
    return () => { active = false; clearTimeout(timer); this.account = ""; this.enabled = false; changed(); };
  },
};
/** Runs opened as panes were once remembered per account
 * (`arugula.cloud-panes.<account>`); nothing reads them now. */
function forgetOpenedRuns() {
  try {
    for (let i = localStorage.length - 1; i >= 0; i--) {
      const key = localStorage.key(i);
      if (key?.startsWith("arugula.cloud-panes.")) localStorage.removeItem(key);
    }
  } catch { /* private browsing */ }
}
/** Re-renders on Cloud workspaces and the route; returns the shown past
 * run, if one is. */
export function useCloudPanes() {
  const [, tick] = useState(0);
  useEffect(() => cloudPanes.subscribe(() => tick((n) => n + 1)), []);
  return cloudRoute();
}

// ---- Cloud workspaces (S40, M116): a session in the shell backed by a
// Managoat workspace. Control keeps them and their tabs; nothing about them
// is remembered locally. The shell's route is `#cloud-ws` (the list), or
// `#cloud-ws=cw_…` with `/cwc_…` for a tab; the platform page's are paths
// (`CloudRoutes`).

/** A setup step as people read it: "setup_script" → "setup script". */
export const setupStepName = (step: string) => step.replaceAll("_", " ");
/** "Rebuild needed · 2 changes", or null when the machine is as saved. */
export function cloudWorkspaceRebuildBadge(w: CloudWorkspace): string | null {
  if (!w.stale) return null;
  const n = w.pending?.length ?? 0;
  return n ? `Rebuild needed · ${n} change${n === 1 ? "" : "s"}` : "Rebuild needed";
}
export function cloudWorkspaceRoute(): { workspace: string | null; tab: string | null } | null {
  return routes.workspace();
}

/** What the sidebar and the header say of a Cloud workspace, and its dot. */
export function cloudWorkspaceState(w: CloudWorkspace): { word: string; dot: "needs_input" | "working" | "done" | "idle" | "offline" } {
  if (w.status === "deleting") return { word: "Deleting", dot: "offline" };
  if (w.status === "failed") return { word: "Failed", dot: "offline" };
  if (w.status === "provisioning") return { word: w.setup_step ? `Setting up · ${setupStepName(w.setup_step)}` : "Setting up", dot: "working" };
  if (w.stale) return { word: cloudWorkspaceRebuildBadge(w)!, dot: "done" };
  if (w.sandbox_status === "suspended") return { word: "Parked", dot: "idle" };
  return { word: "Ready", dot: "idle" };
}

/** A tab's attention, from its conversation's status. */
export function cloudTabAttention(t: CloudTab): "needs_input" | "working" | "done" | "idle" {
  if (t.status === "needs_input") return "needs_input";
  if (["opening", "pending", "running", "closing"].includes(t.status)) return "working";
  if (t.status === "failed") return "done";
  return "idle";
}

/** The mounts a tab may start in: each repository's on the machine, else
 * the home. A repository saved since isn't on its disk until a rebuild, so
 * these come from the built settings (the saved ones until the first build). */
export const cloudWorkspaceMounts = (w: CloudWorkspace) => {
  const built = w.built_spec ?? w.saved_spec;
  return built.repositories.length ? built.repositories.map((r) => r.mount || "/home/sprite") : ["/home/sprite"];
};

export const cloudWorkspaces = {
  account: "", list: [] as CloudWorkspace[], tabs: new Map<string, CloudTab[]>(), loaded: false, error: "",
  poke: () => {},
  subscribe(fn: () => void) { listeners.add(fn); return () => { listeners.delete(fn); }; },
  find(id: string | null) { return this.list.find((w) => w.id === id); },
  /** Show the list, a Cloud workspace, or one of its tabs. */
  open(workspace?: string, tab?: string) { routes.openWorkspace(workspace, tab); changed(); },
  /** Read again now (after a create, an open or a delete). */
  refresh() { this.poke(); },
  connect(account: string) {
    this.account = account; this.list = []; this.tabs = new Map(); this.loaded = false; this.error = "";
    let active = true, timer: ReturnType<typeof setTimeout>, running = false, again = false;
    const poll = async () => {
      clearTimeout(timer);
      if (running) { again = true; return; }
      running = true;
      let wait = 3000;
      try {
        const v = await cloudApi<CloudWorkspaces>("workspaces");
        const tabs = await Promise.all(v.cloud_workspaces.map((w) => cloudApi<CloudTabs>(`workspaces/${w.id}/tabs`).then((t) => t.cloud_tabs, () => this.tabs.get(w.id) ?? [])));
        if (!active) return;
        this.list = v.cloud_workspaces; this.tabs = new Map(v.cloud_workspaces.map((w, i) => [w.id, tabs[i]])); this.error = ""; this.loaded = true;
      } catch (e) {
        if (!active) return;
        this.loaded = true;
        // A maintenance window (M141) that outlasted the wait: what was read
        // stays, the page's note says why, and it asks again soon.
        if (maintenance.down) wait = 10000;
        // Not in the beta, or no cloud here: ask seldom.
        else { this.error = (e as Error).message; wait = 15000; }
      } finally { running = false; }
      changed();
      if (active) timer = setTimeout(() => void poll(), again ? 0 : wait);
      again = false;
    };
    this.poke = () => void poll();
    if (account) void poll();
    changed();
    return () => { active = false; clearTimeout(timer); this.poke = () => {}; this.account = ""; this.list = []; this.tabs = new Map(); this.loaded = false; changed(); };
  },
};

