// Your agent tokens (M230), agents with no machine: bearer agent grant tokens for an agent
// that runs somewhere without `arugulad` (a hosted service's), reaching
// the platform's MCP endpoint. Each has powers over projects you read,
// never approves anything, and its agent's writes are labelled
// "<you>'s agent on <name> (MCP)". The token shows once, right after it's made. Listed with what each
// one's agent did lately, and revoked at once (`/api/agent-tokens`, the
// platform's). Made only here, in a signed-in browser (the CLI lists and
// revokes); every new one pushes the account's devices a notice. A token's
// name is shown inside <bdi>, so it can't turn the text around it.

import { useEffect, useState } from "preact/hooks";
import { platformFetch } from "../../shared/maintenance";
import { ConfirmRemove } from "../../shared/ui/confirm";
import { useSubscribe } from "../../shared/ui/hooks";
import { Time } from "../../shared/ui/time";
import { session } from "../session";
import { toneClass } from "../swarm";
import { Section } from "./section";

type TokenGrant = { id: string; power: string; project: { id: string; name: string | null }; live: boolean };
export type AgentToken = {
  id: string;
  name: string;
  created_at: number;
  last_used_at: number | null;
  revoked_at: number | null;
  live: boolean;
  grants: TokenGrant[];
};
type Call = { id: string; tool: string; project: string | null; outcome: string; detail: string; at: number };

// What a token can hold, and what each lets its agent do.
export const TOKEN_POWERS: [string, string][] = [
  ["threads.read", "read threads and issues"],
  ["threads.post", "post in threads, comment on issues"],
  ["issues.write", "file and change issues"],
  ["runs.read", "read runs"],
  ["runs.start", "start run specs"],
  ["runs.command", "start commands of its own"],
  ["deploys.plan", "plan deploys and ask for approval"],
  ["boards.read", "read boards"],
  ["boards.write", "read boards, set their notes and statuses and move rows"],
  ["trackers.write", "create, comment on, close, reopen and edit Plane work items (each may wait for a person)"],
  ["secrets.write", "set project secrets by name (never read them)"],
];

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

// Each power in a few plain words, as the grants' switches say them (UI
// audit #168): a card says these, the form the powers' own names.
const POWER_WORDS: Record<string, string> = {
  "threads.read": "read threads",
  "threads.post": "post in threads",
  "issues.write": "write issues",
  "runs.read": "read runs",
  "runs.start": "start run specs",
  "runs.command": "run commands",
  "deploys.plan": "plan deploys",
  "boards.read": "read boards",
  "boards.write": "write boards",
  "trackers.write": "write to Plane",
  "secrets.write": "set secrets",
};

// "Write issues, plan deploys on Acme; read runs on Web".
export function reachOf(t: AgentToken): string {
  const by = new Map<string, string[]>();
  for (const g of t.grants) {
    const p = g.project.name ?? g.project.id;
    by.set(p, [...(by.get(p) ?? []), POWER_WORDS[g.power] ?? g.power]);
  }
  const line = [...by].map(([p, ps]) => `${ps.join(", ")} on ${p}`).join("; ");
  return line.charAt(0).toUpperCase() + line.slice(1);
}

function Calls({ id }: { id: string }) {
  const [calls, setCalls] = useState<Call[] | null>(null);
  useEffect(() => {
    call<{ calls: Call[] }>(`/api/agent-tokens/${encodeURIComponent(id)}/calls?limit=20`).then(
      (v) => setCalls(v.calls),
      () => setCalls([]),
    );
  }, [id]);
  if (calls === null) return null;
  if (calls.length === 0) return <p class="dim">Its agent hasn't called anything yet.</p>;
  return (
    <ul class="agent-token-calls" data-agent-token-calls={id}>
      {calls.map((c) => (
        <li key={c.id}>
          <Time class="dim" at={c.at} /> <code>{c.tool}</code> <span class={`agent-call-${c.outcome}`}>{c.outcome}</span> {c.detail}
        </li>
      ))}
    </ul>
  );
}

export function AgentTokens() {
  useSubscribe((fn) => session.subscribe(fn));
  const [tokens, setTokens] = useState<AgentToken[] | null>(null);
  const [url, setUrl] = useState("");
  const [err, setErr] = useState("");
  const [making, setMaking] = useState(false);
  const [name, setName] = useState("");
  const [projects, setProjects] = useState<string[]>([]);
  const [powers, setPowers] = useState<string[]>(["threads.read", "issues.write"]);
  const [made, setMade] = useState<{ token: string; name: string } | null>(null);
  const [open, setOpen] = useState("");
  const [busy, setBusy] = useState(false);
  // Revoke asks first (UI audit #158): the token is dead for good.
  const [revoking, setRevoking] = useState<AgentToken | null>(null);
  const load = () =>
    call<{ tokens: AgentToken[]; mcp_url: string }>("/api/agent-tokens").then(
      (v) => {
        setTokens(v.tokens);
        setUrl(v.mcp_url);
      },
      (e: Error) => setErr(e.message),
    );
  useEffect(() => void load(), []);
  if (tokens === null) return err ? <p class="control-error">{err}</p> : null;
  const toggle = (list: string[], v: string, on: boolean) => (on ? [...list.filter((x) => x !== v), v] : list.filter((x) => x !== v));
  const mint = async (e: Event) => {
    e.preventDefault();
    setErr("");
    setBusy(true);
    try {
      const v = await call<{ token: string; agent_token: AgentToken }>("/api/agent-tokens", { name, projects, powers });
      setMade({ token: v.token, name: v.agent_token.name });
      setMaking(false);
      setName("");
      await load();
    } catch (x) {
      setErr((x as Error).message);
    } finally {
      setBusy(false);
    }
  };
  const revoke = async (id: string) => {
    setErr("");
    try {
      await call(`/api/agent-tokens/${encodeURIComponent(id)}/revoke`, {});
    } catch (x) {
      setErr((x as Error).message);
    }
    await load();
  };
  return (
    <Section
      id="tokens"
      title="Your agent tokens"
      count={tokens.length}
      lede="Tokens for agents that run somewhere without Arugula, reaching your projects through the platform's MCP endpoint."
      more={
        <p>
          A token's agent acts as yours, labelled with the token's name, never past what you can do. It never approves anything: a deploy
          it plans waits for an owner's phone or browser.
        </p>
      }
    >
      {url ? (
        <p class="settings-endpoint">
          <span class="dim">Endpoint</span> <code data-agent-mcp-url>{url}</code>
        </p>
      ) : null}
      {made && (
        <div class="deploy-form agent-token-made" data-agent-token-made>
          <h3>
            <bdi>{made.name}</bdi>'s token
          </h3>
          <p>
            Copy it now: it's shown this once, and only its hash is kept.
          </p>
          <code class="deploy-digest" data-agent-token-value>
            {made.token}
          </code>
          <p class="dim">
            Give it to the agent as an MCP server over HTTP, with the header <code>Authorization: Bearer …</code>. For Claude Code:{" "}
            <code>
              claude mcp add --transport http arugula {url} --header "Authorization: Bearer {made.token}"
            </code>
          </p>
          <div class="prompt-buttons">
            <button class="secondary" type="button" onClick={() => void navigator.clipboard?.writeText(made.token)}>
              Copy
            </button>
            <button type="button" class="primary" onClick={() => setMade(null)}>
              Done
            </button>
          </div>
        </div>
      )}
      {tokens.length === 0 && !making && <p class="dim">None yet.</p>}
      {tokens.length > 0 && (
        <ul class="platform-list sw-cards settings-cards agent-tokens">
          {/* The bar: live, or revoked. */}
          {tokens.map((t) => (
            <li key={t.id} data-agent-token={t.id} class={`sw-card ${toneClass(t.live ? "accent" : "muted")}`}>
              <div class="sw-head">
                <b class="sw-name">
                  <bdi>{t.name}</bdi>
                </b>
                <span class="settings-card-act">
                  <button class="secondary small" data-agent-token-calls-open={t.id} onClick={() => setOpen(open === t.id ? "" : t.id)}>
                    {open === t.id ? "Hide calls" : "Calls"}
                  </button>
                  {t.live && (
                    <button class="secondary danger small" data-agent-token-revoke={t.id} onClick={() => setRevoking(t)}>
                      Revoke
                    </button>
                  )}
                </span>
              </div>
              <span class="agent-token-reach">{reachOf(t)}</span>
              <span class="dim">
                {t.live ? (
                  t.last_used_at ? (
                    <>
                      Used <Time at={t.last_used_at} />
                    </>
                  ) : (
                    "Never used"
                  )
                ) : (
                  <>
                    Revoked <Time at={t.revoked_at ?? 0} />
                  </>
                )}
              </span>
              {open === t.id && <Calls id={t.id} />}
            </li>
          ))}
        </ul>
      )}
      {!making ? (
        <button class="secondary settings-new" data-agent-token-new onClick={() => setMaking(true)}>
          New agent token
        </button>
      ) : (
        <form class="deploy-form" aria-label="New agent token" data-agent-token-form onSubmit={(e) => void mint(e)}>
          <h3>New agent token</h3>
          <label>
            Name <span class="dim">(its agent is "your agent on NAME (MCP)")</span>
            <input required maxLength={64} placeholder="redone" value={name} data-agent-token-name onInput={(e) => setName(e.currentTarget.value)} />
          </label>
          <fieldset class="deploy-secret-names">
            <legend>Projects</legend>
            {session.projects.map((p) => (
              <label class="control-confirm" key={p.id}>
                <input
                  type="checkbox"
                  checked={projects.includes(p.id)}
                  data-agent-token-project={p.id}
                  onChange={(e) => setProjects(toggle(projects, p.id, e.currentTarget.checked))}
                />{" "}
                {p.team ? p.name : "Your personal project"}
              </label>
            ))}
          </fieldset>
          <fieldset class="deploy-secret-names">
            <legend>What its agent may do there</legend>
            {TOKEN_POWERS.map(([p, what]) => (
              <label class="control-confirm" key={p}>
                <input
                  type="checkbox"
                  checked={powers.includes(p)}
                  data-agent-token-power={p}
                  onChange={(e) => setPowers(toggle(powers, p, e.currentTarget.checked))}
                />{" "}
                <code>{p}</code> <span class="dim">{what}</span>
              </label>
            ))}
          </fieldset>
          {powers.includes("threads.read") && projects.some((id) => session.projects.find((p) => p.id === id)?.team) && (
            <p class="control-warning" data-agent-token-warn-threads>
              <code>threads.read</code> over a team's project reaches every one of that team's channels, not only its issues.
            </p>
          )}
          {powers.includes("runs.command") && (
            <p class="control-warning" data-agent-token-warn-command>
              <code>runs.command</code> runs anything on the project's hosted boxes, so its agent can read what a box holds: its
              environment and secrets included.
            </p>
          )}
          <p class="dim">The token is shown once, and your devices get a notice that it was made.</p>
          <div class="prompt-buttons">
            <button class="secondary" type="button" onClick={() => setMaking(false)}>
              Cancel
            </button>
            <button class="primary" disabled={busy || !projects.length || !powers.length} data-agent-token-make>
              Make token
            </button>
          </div>
        </form>
      )}
      {err ? <p class="control-error">{err}</p> : null}
      {revoking && (
        <ConfirmRemove
          title={`Revoke ${revoking.name}?`}
          label="Revoke"
          cancel={() => setRevoking(null)}
          go={() => {
            const id = revoking.id;
            setRevoking(null);
            void revoke(id);
          }}
        >
          <p>Its agent is refused from now on, everywhere. A token can't be revoked and used again: give the agent a new one.</p>
        </ConfirmRemove>
      )}
    </Section>
  );
}
