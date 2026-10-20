// Cloud agents, through control. Nothing about them is remembered locally:
// content and authorization always come from control, independently of any
// daemon. Cloud runs are retired (S40, M119): past ones stay readable at
// `#cloud=run_…`, and Cloud workspaces (below) are where agents run now.
import { createContext } from "preact";
import { useContext, useEffect, useState } from "preact/hooks";
import type { FleetPane } from "./fleet";
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
 * the block's `cloud` method, which the daemon sends on as the machine. */
export type CloudApi = <T>(path: string, method?: string, body?: unknown) => Promise<T>;
export const CloudApiContext = createContext<CloudApi>(cloudApi);
export const useCloudApi = () => useContext(CloudApiContext);
const listeners = new Set<() => void>();
const changed = () => listeners.forEach((fn) => fn());
/** The shown past run (`#cloud=run_…`) or past tab (`#cloud=cwc_…`). */
export const cloudRoute = () => /^#cloud=((?:run|cwc)_[0-9a-f]+)$/.exec(location.hash)?.[1] ?? null;
addEventListener("hashchange", changed);
export const cloudPanes = {
  account: "", enabled: false,
  subscribe(fn: () => void) { listeners.add(fn); return () => { listeners.delete(fn); }; },
  /** Show a past run or a past tab. */
  open(id: string) { location.hash = `cloud=${id}`; changed(); },
  /** Back to the machine: leaves a past run or a Cloud workspace. */
  local() { if (cloudRoute() || cloudWorkspaceRoute()) { history.replaceState(null, "", location.pathname + location.search); changed(); } },
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
/** Every cloud pane the swarm and the attention drawer draw: the Cloud
 * workspaces' tabs. */
export const cloudFleet = (): FleetPane[] => cloudWorkspaces.fleet();
/** Re-renders on Cloud workspaces and the route; returns the shown past
 * run, if one is. */
export function useCloudPanes() {
  const [, tick] = useState(0);
  useEffect(() => cloudPanes.subscribe(() => tick((n) => n + 1)), []);
  return cloudRoute();
}

// ---- Cloud workspaces (S40, M116): a session in the shell backed by a
// Managoat workspace. Control keeps them and their tabs; nothing about them
// is remembered locally. The route is `#cloud-ws` (the list), or
// `#cloud-ws=cw_…` with `/cwc_…` for a tab.

/** A setup step as people read it: "setup_script" → "setup script". */
export const setupStepName = (step: string) => step.replaceAll("_", " ");
/** "Rebuild needed · 2 changes", or null when the machine is as saved. */
export function cloudWorkspaceRebuildBadge(w: CloudWorkspace): string | null {
  if (!w.stale) return null;
  const n = w.pending?.length ?? 0;
  return n ? `Rebuild needed · ${n} change${n === 1 ? "" : "s"}` : "Rebuild needed";
}
export function cloudWorkspaceRoute(): { workspace: string | null; tab: string | null } | null {
  const m = /^#cloud-ws(?:=(cw_[0-9a-f]+)(?:\/(cwc_[0-9a-f]+))?)?$/.exec(location.hash);
  return m ? { workspace: m[1] ?? null, tab: m[2] ?? null } : null;
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
  open(workspace?: string, tab?: string) { location.hash = workspace ? `cloud-ws=${workspace}${tab ? `/${tab}` : ""}` : "cloud-ws"; changed(); },
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
  fleet(): FleetPane[] {
    return this.list.flatMap((w) => (this.tabs.get(w.id) ?? []).map((t): FleetPane => {
      const attention = cloudTabAttention(t);
      const id = Number.parseInt(t.id.slice(4, 17), 16);
      return {
        key: `cloud:${t.id}`, host: "Cloud", id, cloudTab: { workspace: w.id, tab: t.id },
        info: { id, epoch: 0, cwd: t.cwd, command: t.agent_name, running: attention === "working", policy: { kind: "none" }, current: null, last: null,
          attention, integration: false, type: "agent", host: null, kind: "agent", title: `${t.agent_name} in ${w.name}`,
          reason: attention === "needs_input" ? { kind: "input", since_ms: t.created, headline: `${t.agent_name} in ${w.name} waits for you`, actions: [] }
            : t.status === "failed" ? { kind: "failed", since_ms: t.created, headline: t.error ?? `${t.agent_name} in ${w.name} failed`, actions: [] } : null },
        session: { id: Number.parseInt(w.id.slice(3, 16), 16), name: w.name }, stale: !!this.error, person: { id: "me", name: "me", kind: "me" }, driver: null, watchers: [], unread: 0, mention: false,
      };
    }));
  },
};
