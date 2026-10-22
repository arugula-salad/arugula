// Agents in conversations (M197): per machine, a switch per project (and
// one for your DMs) that lets its agents read and post in the threads
// there, and write the project's issues. Off until you turn one on, and
// only over what you read; what they write is labelled as your agent's,
// and every room they reach says so. The same grants as `arugula grants
// add` (`/api/grants`, the platform's).

import { useEffect, useState } from "preact/hooks";
import { platformFetch } from "../../shared/maintenance";
import { useSubscribe } from "../../shared/ui/hooks";
import { session } from "../session";
import { liveOver, machinesOf, powersFor, type AgentScope, type Grant } from "./grants";

async function call<T>(path: string, body?: unknown): Promise<T> {
  const r = await platformFetch(path, {
    method: body === undefined ? "GET" : "POST",
    credentials: "same-origin",
    headers: body === undefined ? {} : { "Content-Type": "application/json" },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  const v = await r.json().catch(() => ({}));
  if (!r.ok) throw new Error((v as { error?: string }).error ?? `Request failed (${r.status})`);
  return v as T;
}

export function AgentGrants() {
  useSubscribe((fn) => session.subscribe(fn));
  const [grants, setGrants] = useState<Grant[] | null>(null);
  const [err, setErr] = useState("");
  const [busy, setBusy] = useState("");
  const load = () =>
    call<{ grants: Grant[] }>("/api/grants").then(
      (v) => setGrants(v.grants),
      (e: Error) => setErr(e.message),
    );
  useEffect(() => void load(), []);
  if (grants === null) return err ? <p class="control-error">{err}</p> : null;
  const machines = machinesOf(grants);
  const scopes: AgentScope[] = [
    ...session.projects.map((p) => ({ kind: "project" as const, id: p.id, name: p.team ? `${p.name}'s channels and issues` : "Your personal project" })),
    { kind: "account", name: "Your DMs" },
  ];
  const turn = async (machine: string, s: AgentScope, on: boolean) => {
    setErr("");
    setBusy(`${machine}/${s.id ?? "dms"}`);
    try {
      if (on) {
        const scope = s.kind === "project" ? { kind: "project", id: s.id } : { kind: "account" };
        for (const power of powersFor(s.kind)) await call("/api/grants", { daemon: machine, power, scope });
      } else {
        for (const g of liveOver(grants, machine, s)) await call(`/api/grants/${encodeURIComponent(g.id)}/revoke`, {});
      }
    } catch (e) {
      setErr((e as Error).message);
    }
    await load();
    setBusy("");
  };
  return (
    <>
      <h2>Agents in conversations</h2>
      <p class="dim">
        Let the agents on one of your machines read and post in a project's threads (a team's channels) and write its issues, or read and post
        in your DMs. Off until you turn it on, and never more than you can read. What they write is labelled as your agent's on that machine,
        and every room they reach says so to everyone in it.
      </p>
      {machines.length === 0 && <p class="dim">No machines on your account yet.</p>}
      {machines.map((m) => (
        <div class="agent-grants" data-agent-grants={m.id} key={m.id}>
          <h3>{m.name}</h3>
          {scopes.map((s) => {
            const live = liveOver(grants, m.id, s);
            const key = `${m.id}/${s.id ?? "dms"}`;
            const all = powersFor(s.kind).every((p) => live.some((g) => g.power === p));
            return (
              <label class="control-confirm" key={key}>
                <input
                  type="checkbox"
                  role="switch"
                  data-agent-switch={key}
                  checked={live.length > 0}
                  disabled={busy === key}
                  onChange={(e) => void turn(m.id, s, (e.target as HTMLInputElement).checked)}
                />{" "}
                {s.name}
                {live.length > 0 && !all && <span class="dim"> (only {live.map((g) => g.power).join(", ")})</span>}
              </label>
            );
          })}
        </div>
      ))}
      {err ? <p class="control-error">{err}</p> : null}
    </>
  );
}
