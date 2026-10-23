// A project's agents with no machine and its own secrets (M230), for its
// owners: every agent token over the project, whoever minted it, with a
// revoke that takes its reach here back; and the secrets agents set by name
// (`set_secret`), each with who last set it and when, never a value.

import { useEffect, useState } from "preact/hooks";
import { platformFetch } from "../../shared/maintenance";
import type { PlatformProject } from "../session";
import { ago, reachOf, type AgentToken } from "./agent-tokens";
import { roleIn } from "./environments";

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

type Over = AgentToken & { owner: { account: string; name: string | null } };
type Secret = {
  name: string;
  updated_at: number;
  updated_by: { account: string; name: string | null } | null;
  agent: { id: string; name: string | null } | null;
};

export function ProjectAgents({ p }: { p: PlatformProject }) {
  const [tokens, setTokens] = useState<Over[] | null>(null);
  const [secrets, setSecrets] = useState<{ secrets: Secret[]; max: number } | null>(null);
  const [err, setErr] = useState("");
  const base = `/api/projects/${encodeURIComponent(p.id)}`;
  const load = () =>
    Promise.all([call<{ tokens: Over[] }>(`${base}/agent-tokens`), call<{ secrets: Secret[]; max: number }>(`${base}/secrets`)]).then(
      ([t, s]) => {
        setTokens(t.tokens);
        setSecrets(s);
      },
      (e: Error) => setErr(e.message),
    );
  const owner = roleIn(p) === "owner";
  useEffect(() => void (owner && load()), [p.id, owner]);
  if (!owner) return null;
  const revoke = async (id: string) => {
    setErr("");
    try {
      await call(`${base}/agent-tokens/${encodeURIComponent(id)}/revoke`, {});
    } catch (e) {
      setErr((e as Error).message);
    }
    await load();
  };
  return (
    <div data-project-agents>
      <h2>Agents with no machine</h2>
      <p class="dim">
        Every agent token over this project, whoever made it. Revoke takes its reach here back at once; its owner keeps it elsewhere.
      </p>
      {tokens?.length === 0 && <p class="dim">None.</p>}
      <ul class="platform-list agent-tokens">
        {tokens?.map((t) => (
          <li key={t.id} data-project-agent-token={t.id}>
            <strong>
              <bdi>{t.owner.name ?? t.owner.account}</bdi>'s agent token “<bdi>{t.name}</bdi>”
            </strong>{" "}
            <span class="dim">{reachOf(t)}</span> <span class="dim">{t.last_used_at ? `used ${ago(t.last_used_at)}` : "never used"}</span>{" "}
            <button class="control-linkish" data-project-agent-revoke={t.id} onClick={() => void revoke(t.id)}>
              Revoke here
            </button>
          </li>
        ))}
      </ul>
      <h2>Project secrets</h2>
      <p class="dim">
        Secrets set by name for this project, by agents with <code>secrets.write</code>: who set each last, and when. Their values are never shown.
        {secrets ? ` At most ${secrets.max}.` : ""}
      </p>
      {secrets?.secrets.length === 0 && <p class="dim">None.</p>}
      <ul class="platform-list" data-project-secrets>
        {secrets?.secrets.map((s) => (
          <li key={s.name} data-project-secret={s.name}>
            <code>{s.name}</code>{" "}
            <span class="dim">
              set {ago(s.updated_at)}
              {s.updated_by ? (
                <>
                  {" "}
                  by <bdi>{s.updated_by.name ?? s.updated_by.account}</bdi>
                  {s.agent ? (
                    <>
                      's agent token “<bdi>{s.agent.name?.replace(/ \(MCP\)$/, "") ?? s.agent.id}</bdi>”
                    </>
                  ) : null}
                </>
              ) : null}
            </span>
          </li>
        ))}
      </ul>
      {err ? <p class="control-error">{err}</p> : null}
    </div>
  );
}
