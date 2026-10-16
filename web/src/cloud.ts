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
  local() { if (cloudRoute()) { history.replaceState(null, "", location.pathname + location.search); changed(); } },
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
export function useCloudPanes() {
  const [, tick] = useState(0);
  useEffect(() => cloudPanes.subscribe(() => tick((n) => n + 1)), []);
  return cloudRoute();
}
