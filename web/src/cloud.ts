// Account-owned agent panes. Only pane IDs are remembered locally; content
// and authorization always come from control, independently of any daemon.
import { useEffect, useState } from "preact/hooks";
import type { FleetPane } from "./fleet";
export type Definition = { name: string; instructions: string; mcp_servers: Record<string, unknown>; skills: { name: string; content: string }[]; secret_names: string[] };
export type CloudAgent = { id: string; version: number; curated: boolean; definition: Definition };
export type CloudRun = { id: string; state: string; created: number; spec: { agent_id: string; prompt: string; repository: string; git_ref: string; repository_secret?: string | null; agent: Definition }; result: Record<string, unknown> | null };
export const cloudTerminal = (run: CloudRun) => ["completed", "failed", "cancelled", "expired"].includes(run.state);
export async function cloudApi<T>(path: string, method = "GET", body?: unknown): Promise<T> {
  const r = await fetch(`/api/cloud/${path}`, { method, credentials: "same-origin", headers: body === undefined ? {} : { "Content-Type": "application/json" }, body: body === undefined ? undefined : JSON.stringify(body) });
  const value = await r.json();
  if (!r.ok) throw new Error(value.error ?? `Request failed (${r.status})`);
  return value as T;
}
const listeners = new Set<() => void>();
const changed = () => listeners.forEach((fn) => fn());
export const cloudRoute = () => /^#cloud=(run_[0-9a-f]+)$/.exec(location.hash)?.[1] ?? null;
addEventListener("hashchange", changed);
export const cloudPanes = {
  account: "", enabled: false, runs: [] as CloudRun[], opened: [] as string[], error: "",
  subscribe(fn: () => void) { listeners.add(fn); return () => { listeners.delete(fn); }; },
  remember() { try { localStorage.setItem(`arugula.cloud-panes.${this.account}`, JSON.stringify(this.opened)); } catch { /* private browsing */ } },
  open(id: string) { if (!this.opened.includes(id)) this.opened.push(id); this.remember(); location.hash = `cloud=${id}`; changed(); },
  close(id: string) { this.opened = this.opened.filter((x) => x !== id); this.remember(); if (cloudRoute() === id) this.local(); changed(); },
  /** Back to the machine: leaves a cloud run or a Cloud workspace. */
  local() { if (cloudRoute() || cloudWorkspaceRoute()) { history.replaceState(null, "", location.pathname + location.search); changed(); } },
  connect(account: string) {
    this.account = account; this.enabled = false; this.runs = []; this.error = "";
    try { this.opened = JSON.parse(localStorage.getItem(`arugula.cloud-panes.${account}`) ?? "[]").filter((v: unknown) => typeof v === "string" && /^run_[0-9a-f]+$/.test(v)); } catch { this.opened = []; }
    let active = true, timer: ReturnType<typeof setTimeout>;
    const poll = async () => {
      try { const v = await cloudApi<{ runs: CloudRun[] }>("runs"); if (!active) return; this.runs = v.runs; this.error = ""; }
      catch (e) { if (!active) return; this.error = (e as Error).message; }
      changed(); if (active) timer = setTimeout(() => void poll(), 3000);
    };
    if (account) { const selected = cloudRoute(); if (selected && !this.opened.includes(selected)) this.opened.push(selected); void poll(); void cloudApi("agents").then(() => { if (active) { this.enabled = true; changed(); } }, () => {}); } changed();
    return () => { active = false; clearTimeout(timer); this.account = ""; this.enabled = false; this.runs = []; this.opened = []; changed(); };
  },
  fleet(): FleetPane[] {
    return this.runs.filter((r) => !cloudTerminal(r) || this.opened.includes(r.id)).map((r): FleetPane => ({
      key: `cloud:${r.id}`, host: "Cloud", id: Number.parseInt(r.id.slice(4, 17), 16), cloudRun: r.id,
      info: { id: Number.parseInt(r.id.slice(4, 17), 16), epoch: 0, cwd: null, command: r.spec.agent.name, running: !cloudTerminal(r), policy: { kind: "none" }, current: null, last: null,
        attention: r.state === "needs_input" ? "needs_input" : cloudTerminal(r) ? "done" : "working", integration: false, type: "agent", host: null, kind: "agent", title: r.spec.prompt,
        reason: r.state === "needs_input" ? { kind: "input", since_ms: r.created, headline: r.spec.prompt, actions: [] } : null },
      session: null, stale: !!this.error, person: { id: "me", name: "me", kind: "me" }, driver: null, watchers: [], unread: 0, mention: false,
    }));
  },
};
/** Every cloud pane the swarm and the attention drawer draw: runs, then
 * Cloud workspace tabs. */
export const cloudFleet = (): FleetPane[] => [...cloudPanes.fleet(), ...cloudWorkspaces.fleet()];
/** Re-renders on cloud runs, Cloud workspaces and the route; returns the
 * shown run, if one is. */
export function useCloudPanes() {
  const [, tick] = useState(0);
  useEffect(() => cloudPanes.subscribe(() => tick((n) => n + 1)), []);
  return cloudRoute();
}

// ---- Cloud workspaces (S40, M116): a session in the shell backed by a
// Managoat workspace. Control keeps them and their tabs; nothing about them
// is remembered locally. The route is `#cloud-ws` (the list), or
// `#cloud-ws=cw_…` with `/cwc_…` for a tab.

export type CloudWorkspaceRepository = { url: string; ref?: string; mount?: string; secret?: string | null };
export type CloudWorkspaceSpec = {
  name: string; repositories: CloudWorkspaceRepository[]; packages: { apt?: string[]; npm?: string[] };
  setup_script?: string; setup_timeout_seconds?: number | null; env_vars?: Record<string, string>;
  networking?: { type: "unrestricted" | "limited"; allowed_hosts?: string[] } | null; secrets?: string[];
};
export type CloudWorkspace = {
  id: string; name: string; saved_spec: CloudWorkspaceSpec; built_spec: CloudWorkspaceSpec | null;
  status: "provisioning" | "ready" | "failed" | "deleting"; sandbox_status: "pending" | "starting" | "ready" | "suspended" | null;
  error: string | null; revision: string | null; built_from_revision: string | null; stale: boolean; changed_fields: string[]; created: number;
  /** What a rebuild changes (M117), and whether Managoat has the saved settings yet. */
  pending: CloudWorkspaceChange[]; settings_applied: boolean;
  /** A rebuild or retry waiting for Managoat to have a machine free (M120), and why the last one that waited didn't happen. */
  rebuild_queued: boolean; rebuild_error: string | null;
};
/** A saved setting the machine wasn't built with: `before` is built (null when not known), `after` saved. `env_vars` are names only. */
export type CloudWorkspaceChange = { field: string; before: unknown; after: unknown };

/** "Rebuild needed · 2 changes", or null when the machine is as saved. */
export function cloudWorkspaceRebuildBadge(w: CloudWorkspace): string | null {
  if (!w.stale) return null;
  const n = w.pending?.length ?? 0;
  return n ? `Rebuild needed · ${n} change${n === 1 ? "" : "s"}` : "Rebuild needed";
}
export type CloudTab = {
  id: string; cloud_workspace: string; agent_id: string; agent_name: string; runtime: string; model: string; cwd: string;
  status: string; error: string | null; waiting_for_setup: boolean; created: number;
};
export type CloudModel = { runtime: string; model: string };

export function cloudWorkspaceRoute(): { workspace: string | null; tab: string | null } | null {
  const m = /^#cloud-ws(?:=(cw_[0-9a-f]+)(?:\/(cwc_[0-9a-f]+))?)?$/.exec(location.hash);
  return m ? { workspace: m[1] ?? null, tab: m[2] ?? null } : null;
}

/** What the sidebar and the header say of a Cloud workspace, and its dot. */
export function cloudWorkspaceState(w: CloudWorkspace): { word: string; dot: "needs_input" | "working" | "done" | "idle" | "offline" } {
  if (w.status === "deleting") return { word: "Deleting", dot: "offline" };
  if (w.status === "failed") return { word: "Failed", dot: "offline" };
  if (w.status === "provisioning") return { word: "Setting up", dot: "working" };
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

/** The mounts a tab may start in: each repository's, else the home. */
export const cloudWorkspaceMounts = (w: CloudWorkspace) =>
  w.saved_spec.repositories.length ? w.saved_spec.repositories.map((r) => r.mount || "/home/sprite") : ["/home/sprite"];

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
        const v = await cloudApi<{ cloud_workspaces: CloudWorkspace[] }>("workspaces");
        const tabs = await Promise.all(v.cloud_workspaces.map((w) => cloudApi<{ cloud_tabs: CloudTab[] }>(`workspaces/${w.id}/tabs`).then((t) => t.cloud_tabs, () => this.tabs.get(w.id) ?? [])));
        if (!active) return;
        this.list = v.cloud_workspaces; this.tabs = new Map(v.cloud_workspaces.map((w, i) => [w.id, tabs[i]])); this.error = ""; this.loaded = true;
      } catch (e) {
        if (!active) return;
        // Not in the beta, or no cloud here: ask seldom.
        this.error = (e as Error).message; this.loaded = true; wait = 15000;
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
