// A project's agent tokens (agents with no machine) and its own secrets
// (M230), for its owners: every agent token over the project, whoever
// minted it, with a revoke that takes its reach here back; and the secrets set by name, by
// agents (`set_secret`) or by an owner here (M236), each with who last set
// it and when, never a value. Beside each secret, which registered clients
// (studio, M236) may read it for their apps in the project, and how often
// they have: an owner's grant, by name, a switch each. Two sections
// (./section): the tokens as cards, as /settings's are (the bar: live or
// revoked), and the secrets as rows.

import { useEffect, useState } from "preact/hooks";
import { platformFetch } from "../../shared/maintenance";
import { ConfirmRemove } from "../../shared/ui/confirm";
import type { PlatformProject } from "../session";
import { names } from "../links";
import { follow, pathOf } from "../router";
import { toneClass } from "../swarm";
import { ago, reachOf, type AgentToken } from "./agent-tokens";
import { roleIn } from "./environments";
import { Section } from "./section";

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

/** A registered client's ask, and an owner's grant, to send one of the
 * project's secrets for one of its apps to one host (M236). */
type Grant = {
  client: string;
  name: string;
  app: string;
  host: string;
  granted_at: number | null;
  reads: number;
  last_read_at: number | null;
  stale: boolean;
  changed_since_grant: boolean;
  value_set_by: string | null;
  value_set_by_agent: string | null;
};
type Clients = { clients: string[]; apps: { client: string; app: string; title: string }[]; grants?: Grant[] };

export function ProjectAgents({ p }: { p: PlatformProject }) {
  const [tokens, setTokens] = useState<Over[] | null>(null);
  const [secrets, setSecrets] = useState<{ secrets: Secret[]; max: number } | null>(null);
  const [clients, setClients] = useState<Clients | null>(null);
  const [err, setErr] = useState("");
  const [name, setName] = useState("");
  const [value, setValue] = useState("");
  const [busy, setBusy] = useState(false);
  // Revoke and Clear ask first (UI audit #158).
  const [revoking, setRevoking] = useState<Over | null>(null);
  const [clearing, setClearing] = useState<string | null>(null);
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
  const grant = (g: Grant, on: boolean) =>
    act(() =>
      call(
        `${base}/clients/${encodeURIComponent(g.client)}/apps/${encodeURIComponent(g.app)}/secrets/${encodeURIComponent(g.name)}`,
        undefined,
        on ? "PUT" : "DELETE",
      ),
    );
  const asksFor = (secret: string) => clients?.grants?.filter((g) => g.name === secret) ?? [];
  const titleOf = (g: Grant) => clients?.apps.find((a) => a.client === g.client && a.app === g.app)?.title ?? g.app;
  const revoke = async (id: string) => {
    setErr("");
    try {
      await call(`${base}/agent-tokens/${encodeURIComponent(id)}/revoke`, {});
    } catch (e) {
      setErr((e as Error).message);
    }
    await load();
  };
  // Where an owner makes their own tokens: the account's settings, at
  // that section (UI audit #166).
  const own = `${pathOf({ page: "settings" }, names)}#tokens`;
  return (
    <>
      <Section
        id="agents"
        title="Agent tokens"
        count={tokens?.length}
        hooks={{ "data-project-agents": "" }}
        lede="Every agent token over this project, whoever made it: agents with no machine."
        more={
          <p>
            Revoke takes a token's reach over this project back at once; its owner keeps it for the rest. Owners make them in{" "}
            <a href={own} onClick={(e) => follow(e, own)} data-project-agents-own>
              their own settings
            </a>
            .
          </p>
        }
      >
        {tokens?.length === 0 && <p class="dim">None yet.</p>}
        {tokens?.length ? (
          <ul class="platform-list sw-cards settings-cards agent-tokens">
            {/* The bar: live, or revoked. */}
            {tokens.map((t) => (
              <li key={t.id} class={`sw-card ${toneClass(t.live ? "accent" : "muted")}`} data-project-agent-token={t.id}>
                <div class="sw-head">
                  <b class="sw-name">
                    <bdi>{t.name}</bdi>
                  </b>
                  <span class="dim">
                    <bdi>{t.owner.name ?? t.owner.account}</bdi>'s agent token
                  </span>
                  <span class="settings-card-act">
                    <button class="secondary danger small" data-project-agent-revoke={t.id} onClick={() => setRevoking(t)}>
                      Revoke
                    </button>
                  </span>
                </div>
                <span class="agent-token-reach">{reachOf(t)}</span>
                <span class="dim">{t.last_used_at ? `Used ${ago(t.last_used_at)}` : "Never used"}</span>
              </li>
            ))}
          </ul>
        ) : null}
        {revoking && (
          <ConfirmRemove
            title={`Revoke ${revoking.name} here?`}
            label="Revoke"
            cancel={() => setRevoking(null)}
            go={() => {
              const id = revoking.id;
              setRevoking(null);
              void revoke(id);
            }}
          >
            <p>Its agent loses its reach over this project at once. {revoking.owner.name ?? revoking.owner.account} keeps the token for the rest.</p>
          </ConfirmRemove>
        )}
      </Section>
      <Section
        id="secrets"
        title="Project secrets"
        count={secrets?.secrets.length}
        lede="Secrets set by name for this project, by its owners or their agents. Their values are never shown."
        more={
          <>
            <p>
              Owners set them here, and agents with <code>secrets.write</code> by name; each shows who set it last, and when.
              {secrets ? ` At most ${secrets.max}.` : ""}
            </p>
            {clients?.clients.length ? (
              <p>
                A product asks to send one for one of its apps to one host (studio's egress). Once you allow that, it gets the value for that app
                and host only, each read is counted here, and an agent changing the value stops it until you allow it again.
              </p>
            ) : null}
          </>
        }
      >
        {secrets?.secrets.length === 0 && <p class="dim">None yet.</p>}
        <ul class="platform-list settings-rows secret-rows" data-project-secrets>
          {secrets?.secrets.map((s) => (
            <li key={s.name} data-project-secret={s.name}>
              <div class="secret-head">
                <code class="secret-name">{s.name}</code>
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
                <span class="settings-card-act">
                  <button class="secondary danger small" data-project-secret-clear={s.name} disabled={busy} onClick={() => setClearing(s.name)}>
                    Clear
                  </button>
                </span>
              </div>
              {asksFor(s.name).map((g) => (
                <label key={`${g.client}/${g.app}`} class="settings-switch">
                  <input
                    type="checkbox"
                    role="switch"
                    class="sw-switch"
                    data-project-secret-grant={`${g.client}/${g.app}/${g.name}`}
                    checked={g.granted_at !== null && !g.stale}
                    disabled={busy}
                    onChange={() => void grant(g, g.granted_at === null || g.stale)}
                  />
                  <span>
                    Let <b>{g.client}</b> send it for <bdi>{titleOf(g)}</bdi> to <code>{g.host}</code>
                    {g.stale ? (
                      <span class="control-error"> · an agent changed it since you allowed this: allow it again</span>
                    ) : g.granted_at === null ? (
                      <span class="dim"> · asked, not allowed yet</span>
                    ) : (
                      <span class="dim">
                        {" "}
                        · {g.reads ? `read ${g.reads} ${g.reads === 1 ? "time" : "times"}, last ${ago(g.last_read_at ?? g.granted_at)}` : "not read yet"}
                        {g.changed_since_grant ? ` · changed since by ${s.updated_by?.name ?? s.updated_by?.account ?? g.value_set_by ?? "someone"}` : ""}
                      </span>
                    )}
                  </span>
                </label>
              ))}
            </li>
          ))}
        </ul>
        <form class="platform-link-repo" aria-label="Set a project secret" data-project-secret-form onSubmit={setSecret}>
          <input
            required
            aria-label="Secret name"
            placeholder="Name"
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
        {clearing && (
          <ConfirmRemove
            title={`Clear ${clearing}?`}
            label="Clear"
            cancel={() => setClearing(null)}
            go={() => {
              const n = clearing;
              setClearing(null);
              void act(() => call(`${base}/secrets/${encodeURIComponent(n)}`, undefined, "DELETE"));
            }}
          >
            <p>Its value is deleted for good: nothing that reads it gets it any more, until someone sets it again.</p>
          </ConfirmRemove>
        )}
      </Section>
    </>
  );
}
