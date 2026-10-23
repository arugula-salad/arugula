// A project's agents with no machine and its own secrets (M230), for its
// owners: every agent token over the project, whoever minted it, with a
// revoke that takes its reach here back; and the secrets set by name, by
// agents (`set_secret`) or by an owner here (M236), each with who last set
// it and when, never a value. Beside each secret, which registered clients
// (studio, M236) may read it for their apps in the project, and how often
// they have: an owner's grant, by name.

import { useEffect, useState } from "preact/hooks";
import { platformFetch } from "../../shared/maintenance";
import type { PlatformProject } from "../session";
import { ago, reachOf, type AgentToken } from "./agent-tokens";
import { roleIn } from "./environments";

async function call<T>(path: string, body?: unknown, method?: string): Promise<T> {
  const r = await platformFetch(path, {
    method: method ?? (body === undefined ? "GET" : "POST"),
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

/** A registered client's grant to read one of the project's secrets (M236). */
type Grant = { client: string; name: string; granted_at: number; reads: number; last_read_at: number | null };
type Clients = { clients: string[]; grants?: Grant[] };

export function ProjectAgents({ p }: { p: PlatformProject }) {
  const [tokens, setTokens] = useState<Over[] | null>(null);
  const [secrets, setSecrets] = useState<{ secrets: Secret[]; max: number } | null>(null);
  const [clients, setClients] = useState<Clients | null>(null);
  const [err, setErr] = useState("");
  const [name, setName] = useState("");
  const [value, setValue] = useState("");
  const [busy, setBusy] = useState(false);
  const base = `/api/projects/${encodeURIComponent(p.id)}`;
  const load = () =>
    Promise.all([
      call<{ tokens: Over[] }>(`${base}/agent-tokens`),
      call<{ secrets: Secret[]; max: number }>(`${base}/secrets`),
      call<Clients>(`${base}/clients`).catch(() => null),
    ]).then(
      ([t, s, c]) => {
        setTokens(t.tokens);
        setSecrets(s);
        setClients(c);
      },
      (e: Error) => setErr(e.message),
    );
  const owner = roleIn(p) === "owner";
  useEffect(() => void (owner && load()), [p.id, owner]);
  if (!owner) return null;
  const act = async (f: () => Promise<unknown>) => {
    setErr("");
    setBusy(true);
    try {
      await f();
    } catch (e) {
      setErr((e as Error).message);
    }
    await load();
    setBusy(false);
  };
  const setSecret = (e: Event) => {
    e.preventDefault();
    const n = name.trim();
    if (!n || !value) return;
    const v = value;
    // Never kept on the page, whatever happens.
    setValue("");
    void act(async () => {
      await call(`${base}/secrets/${encodeURIComponent(n)}`, { value: v }, "PUT");
      setName("");
    });
  };
  const grant = (client: string, secret: string, on: boolean) =>
    act(() => call(`${base}/clients/${encodeURIComponent(client)}/secrets/${encodeURIComponent(secret)}`, undefined, on ? "PUT" : "DELETE"));
  const grantOf = (client: string, secret: string) => clients?.grants?.find((g) => g.client === client && g.name === secret);
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
        Secrets set by name for this project, by its owners here or by agents with <code>secrets.write</code>: who set each last, and when. Their
        values are never shown.
        {secrets ? ` At most ${secrets.max}.` : ""}
        {clients?.clients.length
          ? " A product you let read one (studio's apps, for their egress) gets its value for its apps in this project, and nothing else; each read is counted here."
          : ""}
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
            </span>{" "}
            <button class="control-linkish" data-project-secret-clear={s.name} disabled={busy} onClick={() => void act(() => call(`${base}/secrets/${encodeURIComponent(s.name)}`, undefined, "DELETE"))}>
              Clear
            </button>
            {clients?.clients.map((c) => {
              const g = grantOf(c, s.name);
              return (
                <label key={c} class="platform-mirror">
                  <input
                    type="checkbox"
                    data-project-secret-grant={`${c}/${s.name}`}
                    checked={Boolean(g)}
                    disabled={busy}
                    onChange={() => void grant(c, s.name, !g)}
                  />{" "}
                  Let <b>{c}</b> read it for its apps here
                  {g ? <span class="dim"> · {g.reads ? `read ${g.reads} ${g.reads === 1 ? "time" : "times"}, last ${ago(g.last_read_at ?? g.granted_at)}` : "not read yet"}</span> : null}
                </label>
              );
            })}
          </li>
        ))}
      </ul>
      <form class="platform-link-repo" aria-label="Set a project secret" data-project-secret-form onSubmit={setSecret}>
        <input
          required
          aria-label="Secret name"
          placeholder="NAME"
          spellcheck={false}
          value={name}
          data-project-secret-name
          onInput={(e) => setName(e.currentTarget.value.toUpperCase())}
        />
        <input
          required
          type="password"
          autoComplete="new-password"
          spellcheck={false}
          aria-label="Secret value"
          placeholder="Value"
          value={value}
          data-project-secret-value
          onInput={(e) => setValue(e.currentTarget.value)}
        />
        <button class="primary" disabled={busy || !name.trim() || !value} data-project-secret-set>
          Set
        </button>
      </form>
      {err ? <p class="control-error">{err}</p> : null}
    </div>
  );
}
