// Fountain blocks (#942): the person's Fountain account. The first tab is
// its conversations, every one (started here, by the CLI, a script or
// another conversation), newest activity first, with live status from the
// account stream, an unread dot, a label filter (chips, Fountain's
// ?label=) and a fan-out's tree; a row opens the conversation in an agent
// block beside this one. The second tab is the account's agents as a
// read-only catalog: a card per agent with a filter bar, *Run on Fountain*
// and *Spec*. Above both: the account's open permission requests, each
// with Allow and Deny (they're cards and phone pushes too, on the daemon
// that follows the account). Everything drawn comes from the daemon's
// state; viewers get the lists without the buttons.

import { render } from "preact";
import { useEffect, useRef, useState } from "preact/hooks";
import type { Client } from "../client";
import type { PaneId } from "../proto";
import { askText } from "../ui/menu";
import type { BlockRenderer, BlockView } from "./view";

type Source = "agent-specs" | "hand" | "app";
interface Card {
  id: string; name: string; description: string; runtime: string; model: string; skills: string[]; mcp: string[];
  environment: string | null; provider: string; mode: string | null; conversations: number; updated_at: string | null;
  source: Source | null; app: string | null;
}
interface Filter { query?: string; sources?: Source[]; runtimes?: string[]; providers?: string[] }
interface Row {
  id: string; title: string | null; status: string; live: string | null; agent_id: string | null; agent: string | null;
  labels: Record<string, string>; unread: boolean; parent_id: string | null; children: number; runtime: string;
  turn_count: number; last_active_at: string | null; asking: number;
}
interface Asking {
  id: string; conversation_id: string; tool: string | null; name: string | null; summary: string | null; expires_ms: number;
  at_ms: number; title: string; conversation: string; here: PaneId | null;
}
interface TreeNode { id: string; parent_id: string | null; depth: number; status: string | null; title: string | null }
export interface FountainState {
  tab?: "conversations" | "agents";
  conversations?: Row[]; conv_error?: string | null; conv_unreadable?: number; labels?: string[];
  label_counts?: { label: string; count: number }[]; limit?: number; more?: boolean;
  tree?: { root: string; nodes: TreeNode[] } | null; requests?: Asking[]; card?: string | null; stream?: boolean; live?: boolean;
  profile: string | null; profiles: string[]; base_url: string | null; key_from: "arugula" | "env" | "file" | null;
  loading: boolean; error: string | null; agents: Card[]; total: number; unreadable?: number; unreadable_note?: string | null; filter: Filter;
  counts: { source: Record<string, number>; runtime: Record<string, number>; provider: Record<string, number> };
  specs: string | null; specs_why: string | null; updated_ms: number; polls: number; watching?: boolean; said: string | null;
  /** *Spec* for an agent agent-specs doesn't declare: the agent as Fountain returns it (literal values redacted). */
  spec?: { agent: string; json: unknown; note: string | null } | null;
}

const SOURCES: { key: Source; label: string }[] = [
  { key: "agent-specs", label: "agent-specs" },
  { key: "hand", label: "hand-made" },
  { key: "app", label: "app-made" },
];

function ago(iso: string | null): string {
  if (!iso) return "";
  const t = Date.parse(iso);
  if (Number.isNaN(t)) return "";
  const s = Math.max(0, (Date.now() - t) / 1000);
  if (s < 3600) return `${Math.max(1, Math.round(s / 60))}m ago`;
  if (s < 86400) return `${Math.round(s / 3600)}h ago`;
  return `${Math.round(s / 86400)}d ago`;
}

function hostOf(u: string | null): string {
  if (!u) return "";
  try {
    return new URL(u).host;
  } catch {
    return u;
  }
}

function sourceLabel(c: Card): string {
  if (c.source === "app") return c.app ? `app: ${c.app}` : "app-made";
  if (c.source === "hand") return "hand-made";
  return c.source ?? "";
}

function plain(s: FountainState | null): string {
  if (!s) return "";
  const lines = [`Fountain${s.base_url ? ` on ${s.base_url}` : ""}`];
  if (s.error) lines.push(s.error);
  for (const r of s.requests ?? []) lines.push(`${r.conversation} wants to run ${r.title}`);
  for (const r of s.conversations ?? []) lines.push(`${r.unread ? "● " : ""}${rowName(r)} [${r.live ?? r.status}]`);
  lines.push(`${s.agents.length} of ${s.total}`);
  for (const c of s.agents) lines.push(`${c.name} [${c.runtime}] ${sourceLabel(c)}${c.skills.length ? ` skills: ${c.skills.join(", ")}` : ""}`);
  return lines.join("\n");
}

function Chips({ kind, counts, chosen, labels, toggle, disabled }: {
  kind: string; counts: Record<string, number>; chosen: string[]; labels?: Record<string, string>; toggle: (k: string) => void; disabled: boolean;
}) {
  const keys = Object.keys(counts);
  if (keys.length < 2 && chosen.length === 0) return null;
  return (
    <span class="fountain-chips" data-chips={kind}>
      {keys.map((k) => (
        <button
          key={k}
          class={`fountain-chip${chosen.includes(k) ? " on" : ""}`}
          data-chip={k}
          aria-pressed={chosen.includes(k)}
          disabled={disabled}
          onClick={() => toggle(k)}
        >
          {labels?.[k] ?? k} <span class="dim">{counts[k]}</span>
        </button>
      ))}
    </span>
  );
}

/** The account's open permission requests: Allow and Deny answer them
 * through the same door as their cards (the owner's alone). */
function Requests({ client, id, s, mayOwn }: { client: Client; id: PaneId; s: FountainState; mayOwn: boolean }) {
  const reqs = s.requests ?? [];
  if (reqs.length === 0) return null;
  const answer = (r: Asking, action: "allow" | "deny") => void client.act({ action, pane: id, id: r.id });
  return (
    <ul class="fountain-requests" data-fountain-requests>
      {reqs.map((r) => (
        <li key={r.id} class={`fountain-request${s.card === r.id ? " card" : ""}`} data-request={r.id} data-conversation={r.conversation_id}>
          <span class="fountain-request-what">
            <b>{r.conversation}</b> wants to run <code>{r.title}</code>
            {r.here !== null && <span class="dim"> (asking in %{r.here})</span>}
          </span>
          <span class="dim fountain-request-left">{Math.max(0, Math.round((r.expires_ms - Date.now()) / 60000))}m left</span>
          {mayOwn && (
            <span class="ws-actions">
              <button class="pri" data-allow={r.id} onClick={() => answer(r, "allow")}>
                Allow
              </button>
              <button data-deny={r.id} onClick={() => answer(r, "deny")}>
                Deny
              </button>
            </span>
          )}
        </li>
      ))}
    </ul>
  );
}

function rowName(r: Row): string {
  return r.title ?? (r.agent ? `${r.agent} conversation` : r.id.slice(0, 8));
}

/** The conversations tab: label chips, the list (children under their
 * parent when both are listed), a tree, and opening one. */
function Conversations({ s, call, busy, mayAct, mayOwn }: {
  s: FountainState; call: (m: string, a: unknown, f: string) => Promise<boolean>;
  busy: string | null; mayAct: boolean; mayOwn: boolean;
}) {
  const rows = s.conversations ?? [];
  const labels = s.labels ?? [];
  const setLabels = (next: string[]) => void call("labels", { labels: next }, "couldn't filter");
  const toggle = (l: string) => setLabels(labels.includes(l) ? labels.filter((x) => x !== l) : [...labels, l]);
  const counts = s.label_counts ?? [];
  const chips = [...labels.filter((l) => !counts.some((c) => c.label === l)).map((label) => ({ label, count: 0 })), ...counts];
  // Children right after their parent, when both are on the page.
  const ids = new Set(rows.map((r) => r.id));
  const kids = new Map<string, Row[]>();
  for (const r of rows) if (r.parent_id && ids.has(r.parent_id)) kids.set(r.parent_id, [...(kids.get(r.parent_id) ?? []), r]);
  const ordered: { r: Row; depth: number }[] = [];
  const walk = (r: Row, depth: number) => {
    if (ordered.some((o) => o.r.id === r.id)) return;
    ordered.push({ r, depth });
    for (const k of kids.get(r.id) ?? []) walk(k, depth + 1);
  };
  for (const r of rows) if (!r.parent_id || !ids.has(r.parent_id)) walk(r, 0);
  return (
    <>
      {chips.length > 0 && (
        <div class="fountain-filters">
          <span class="fountain-chips" data-chips="labels">
            {chips.map((c) => (
              <button key={c.label} class={`fountain-chip${labels.includes(c.label) ? " on" : ""}`} data-label={c.label} aria-pressed={labels.includes(c.label)} disabled={!mayAct} onClick={() => toggle(c.label)}>
                {c.label} {c.count > 0 && <span class="dim">{c.count}</span>}
              </button>
            ))}
          </span>
          {labels.length > 0 && mayAct && (
            <button class="fountain-clear" data-labels-clear onClick={() => setLabels([])}>
              Clear
            </button>
          )}
        </div>
      )}
      {s.conv_error && (
        <div class="browser-card" data-fountain-conv-error>
          <p>Can't read the conversations</p>
          <p class="dim">{s.conv_error}</p>
        </div>
      )}
      {s.said && <p class="dim fountain-said">{s.said}</p>}
      {s.tree && (
        <div class="browser-card fountain-tree" data-fountain-tree={s.tree.root}>
          <div class="fountain-card-head">
            <b>What it started</b>
            {mayAct && (
              <button class="fountain-link" data-fountain-tree-close onClick={() => void call("untree", {}, "couldn't close it")}>
                Close
              </button>
            )}
          </div>
          <ul class="fountain-convs">
            {s.tree.nodes.map((n) => (
              <li key={n.id} class="fountain-conv" style={{ paddingLeft: `${8 + n.depth * 16}px` }} data-node={n.id}>
                <span class="fountain-conv-title">{n.title ?? n.id.slice(0, 8)}</span>
                <span class={`ws-tag fountain-status ${n.status ?? ""}`}>{n.status ?? ""}</span>
              </li>
            ))}
          </ul>
        </div>
      )}
      <div class="review-body">
        {rows.length === 0 && !s.conv_error && <p class="dim fountain-empty">{labels.length ? "Nothing has those labels" : "No conversations on this account"}</p>}
        <ul class="fountain-convs">
          {ordered.map(({ r, depth }) => {
            const status = r.live ?? r.status;
            return (
              <li key={r.id} class={`fountain-conv${r.unread ? " unread" : ""}`} data-conversation={r.id} style={depth ? { paddingLeft: `${8 + depth * 16}px` } : undefined}>
                <span class="fountain-dot-slot">{r.unread && <span class="fountain-dot" title="unread" data-unread />}</span>
                <button class="fountain-conv-title fountain-link-plain" title={r.id} disabled={!mayOwn || busy !== null} data-open={r.id} onClick={() => void call("open_conversation", { id: r.id }, "couldn't open it")}>
                  {rowName(r)}
                </button>
                <span class={`ws-tag fountain-status ${status}`} data-status={status}>
                  {status}
                </span>
                {r.asking > 0 && <span class="ws-tag fountain-asking">asks</span>}
                <span class="dim fountain-meta">
                  {r.agent ?? ""}
                  {r.last_active_at && ` · ${ago(r.last_active_at)}`}
                </span>
                {Object.entries(r.labels).map(([k, v]) => (
                  <span key={k} class="ws-tag fountain-label">
                    {k}:{v}
                  </span>
                ))}
                <span class="ws-actions">
                  {r.children > 0 && mayAct && (
                    <button data-tree={r.id} title="What it started" onClick={() => void call("tree", { id: r.id }, "couldn't read its tree")}>
                      +{r.children}
                    </button>
                  )}
                  {r.unread && mayOwn && (
                    <button data-read={r.id} title="Mark read" onClick={() => void call("mark_read", { id: r.id }, "couldn't mark it read")}>
                      ✓
                    </button>
                  )}
                </span>
              </li>
            );
          })}
        </ul>
        {s.more && mayAct && (
          <button class="fountain-more" data-fountain-more disabled={busy !== null} onClick={() => void call("more", {}, "couldn't read more")}>
            More
          </button>
        )}
      </div>
    </>
  );
}

function FountainBlock({ client, id, s }: { client: Client; id: PaneId; s: FountainState | null }) {
  const [busy, setBusy] = useState<string | null>(null);
  const [query, setQuery] = useState(s?.filter.query ?? "");
  const typing = useRef<number | null>(null);
  const session = client.sessionOfTab(client.tabOfPane(id)?.id ?? -1) ?? null;
  const role = client.role(session);
  const mayAct = role !== "viewer";
  const mayOwn = role === "owner";
  // Someone else's filter (another client, the phone) shows here too,
  // unless this one is typing.
  useEffect(() => {
    if (typing.current === null) setQuery(s?.filter.query ?? "");
  }, [s?.filter.query]);

  const call = async (method: string, args: unknown, failure: string) => {
    setBusy(method);
    const ok = await client.api(`/api/blocks/${id}/call/${method}`, args, failure);
    setBusy(null);
    return ok;
  };
  if (!s || (s.loading && !s.updated_ms)) {
    return (
      <div class="review ws fountain" data-fountain-block={id}>
        <div class="browser-card dim">Reading your Fountain account…</div>
      </div>
    );
  }
  const f = s.filter;
  const filter = (args: Record<string, unknown>) => void call("filter", args, "couldn't filter");
  const onQuery = (v: string) => {
    setQuery(v);
    if (typing.current !== null) clearTimeout(typing.current);
    typing.current = window.setTimeout(() => {
      typing.current = null;
      filter({ query: v });
    }, 200);
  };
  const toggle = (key: "sources" | "runtimes" | "providers", arg: string) => (k: string) => {
    const now = (f[key] ?? []) as string[];
    filter({ [arg]: now.includes(k) ? now.filter((x) => x !== k) : [...now, k] });
  };
  const pickSpecs = async () => {
    const dir = await askText("Your agent-specs checkout", s.specs ?? "", "~/dev/…/agent-specs");
    if (dir?.trim()) await call("specs", { dir: dir.trim() }, "couldn't use that checkout");
  };
  const filtered = !!(f.query?.trim() || f.sources?.length || f.runtimes?.length || f.providers?.length);
  const sourceCounts = Object.fromEntries(SOURCES.filter((x) => s.counts.source[x.key]).map((x) => [x.key, s.counts.source[x.key]]));
  const tab = s.tab ?? "conversations";
  const convs = s.conversations ?? [];
  const unread = convs.filter((c) => c.unread).length;
  const tabs = (
    <span class="fountain-tabs" role="tablist">
      <button role="tab" class={`fountain-tab${tab === "conversations" ? " on" : ""}`} data-fountain-tab="conversations" aria-selected={tab === "conversations"} disabled={!mayAct} onClick={() => void call("tab", { tab: "conversations" }, "couldn't switch")}>
        Conversations <span class="dim">{convs.length}</span>
        {unread > 0 && <span class="fountain-dot" title={`${unread} unread`} />}
      </button>
      <button role="tab" class={`fountain-tab${tab === "agents" ? " on" : ""}`} data-fountain-tab="agents" aria-selected={tab === "agents"} disabled={!mayAct} onClick={() => void call("tab", { tab: "agents" }, "couldn't switch")}>
        Agents <span class="dim">{s.total}</span>
      </button>
    </span>
  );
  const head = (
    <span class="review-path" title={s.base_url ?? ""}>
      <b>Fountain</b> {hostOf(s.base_url)}
      {s.profile && s.profile !== "default" ? ` (${s.profile})` : ""}
    </span>
  );
  const asks = <Requests client={client} id={id} s={s} mayOwn={mayOwn} />;
  if (tab === "conversations") {
    return (
      <div class="review ws fountain" data-fountain-block={id}>
        <div class="review-bar">
          {head}
          {tabs}
          {mayAct && (
            <button title="Read them again" data-fountain-refresh disabled={busy !== null} onClick={() => void call("refresh", {}, "couldn't read the account")}>
              {busy === "refresh" ? "…" : "↻"}
            </button>
          )}
          <span class={`review-live ${s.live ? "on" : ""}`} title={s.live ? "following the account stream" : "read when drawn"}>
            {s.live ? "live" : s.watching ? "polled" : "paused"}
          </span>
        </div>
        {s.error && (
          <div class="browser-card" data-fountain-error>
            <p>Can't read your Fountain account</p>
            <p class="dim">{s.error}</p>
          </div>
        )}
        {asks}
        <Conversations s={s} call={call} busy={busy} mayAct={mayAct} mayOwn={mayOwn} />
      </div>
    );
  }
  return (
    <div class="review ws fountain" data-fountain-block={id}>
      <div class="review-bar">
        {head}
        {tabs}
        <span class="dim" data-fountain-count>
          {filtered ? `${s.agents.length} of ${s.total}` : `${s.total}`}
        </span>
        {mayOwn && s.profiles.length > 1 && (
          <select
            class="fountain-profile"
            title="Credentials profile"
            value={s.profile ?? "default"}
            disabled={busy !== null}
            onChange={(e) => void call("profile", { name: e.currentTarget.value }, "couldn't use that profile")}
          >
            {s.profiles.map((p) => (
              <option key={p} value={p}>
                {p}
              </option>
            ))}
          </select>
        )}
        {mayAct && (
          <button title="Read them again" data-fountain-refresh disabled={busy !== null} onClick={() => void call("refresh", {}, "couldn't read the agents")}>
            {busy === "refresh" ? "…" : "↻"}
          </button>
        )}
        <span class={`review-live ${s.watching ? "on" : ""}`}>{s.watching ? "live" : "paused"}</span>
      </div>
      {asks}
      <div class="fountain-filters">
        <input
          class="fountain-search"
          type="search"
          placeholder="Search names, descriptions, skills, MCP servers"
          value={query}
          disabled={!mayAct}
          autocomplete="off"
          spellcheck={false}
          onInput={(e) => onQuery(e.currentTarget.value)}
        />
        <Chips kind="source" counts={sourceCounts} chosen={f.sources ?? []} labels={Object.fromEntries(SOURCES.map((x) => [x.key, x.label]))} toggle={toggle("sources", "source")} disabled={!mayAct} />
        <Chips kind="runtime" counts={s.counts.runtime} chosen={f.runtimes ?? []} toggle={toggle("runtimes", "runtime")} disabled={!mayAct} />
        <Chips kind="provider" counts={s.counts.provider} chosen={f.providers ?? []} toggle={toggle("providers", "provider")} disabled={!mayAct} />
        {filtered && mayAct && (
          <button class="fountain-clear" data-fountain-clear onClick={() => { setQuery(""); filter({ clear: true }); }}>
            Clear
          </button>
        )}
      </div>
      {s.error && (
        <div class="browser-card" data-fountain-error>
          <p>Can't read your Fountain agents</p>
          <p class="dim">{s.error}</p>
          {mayAct && <button onClick={() => void call("refresh", {}, "couldn't read the agents")}>Try again</button>}
        </div>
      )}
      {s.unreadable_note && (
        <p class="fountain-said" data-fountain-unreadable>
          {s.unreadable_note}
        </p>
      )}
      {s.said && <p class="dim fountain-said">{s.said}</p>}
      {mayOwn && !s.specs && s.specs_why && (
        <p class="dim fountain-said" data-fountain-specs-why>
          {s.specs_why} <button class="fountain-link" onClick={() => void pickSpecs()}>Pick it…</button>
        </p>
      )}
      {s.spec && (
        <div class="browser-card fountain-spec" data-fountain-spec={s.spec.agent}>
          <div class="fountain-card-head">
            <b>{s.spec.agent}</b>
            <span class="dim">as Fountain has it</span>
            {mayAct && (
              <button class="fountain-link" data-fountain-spec-close onClick={() => void call("unspec", {}, "couldn't close it")}>
                Close
              </button>
            )}
          </div>
          {s.spec.note && <p class="dim">{s.spec.note}</p>}
          <pre class="fountain-spec-json">{JSON.stringify(s.spec.json, null, 2)}</pre>
        </div>
      )}
      <div class="review-body">
        {s.agents.length === 0 && !s.error && <p class="dim fountain-empty">{filtered ? "Nothing matches" : "No agents on this account"}</p>}
        <ul class="fountain-cards">
          {s.agents.map((c) => (
            <li key={c.id} class="fountain-card" data-agent={c.name} data-source={c.source ?? ""}>
              <div class="fountain-card-head">
                <b class="fountain-name" title={c.id}>{c.name}</b>
                <span class={`ws-tag fountain-src ${c.source ?? ""}`}>{sourceLabel(c)}</span>
              </div>
              <div class="dim fountain-meta">
                {c.runtime}
                {c.model && ` · ${c.model.replace(/^anthropic\//, "")}`}
                {c.environment && ` · env ${c.environment}`}
                {` · ${c.provider}${c.mode ? ` ${c.mode}` : ""}`}
                {` · ${c.conversations} conversation${c.conversations === 1 ? "" : "s"}`}
                {c.updated_at && ` · ${ago(c.updated_at)}`}
              </div>
              {c.description && <p class="fountain-desc">{c.description}</p>}
              {(c.skills.length > 0 || c.mcp.length > 0) && (
                <div class="ws-tags fountain-tags">
                  {c.skills.map((k) => (
                    <span key={`s-${k}`} class="ws-tag" title="skill">
                      {k}
                    </span>
                  ))}
                  {c.mcp.map((m) => (
                    <span key={`m-${m}`} class="ws-tag fountain-mcp" title="MCP server">
                      ⚙ {m}
                    </span>
                  ))}
                </div>
              )}
              {mayAct && (
                <div class="ws-actions fountain-actions">
                  <button class="pri" data-run={c.name} disabled={busy !== null} onClick={() => void call("run", { agent: c.name }, `couldn't run ${c.name}`)}>
                    Run on Fountain
                  </button>
                  <button data-spec={c.name} disabled={busy !== null} onClick={() => void call("spec", { agent: c.name }, `couldn't open ${c.name}'s spec`)}>
                    Spec
                  </button>
                </div>
              )}
            </li>
          ))}
        </ul>
      </div>
    </div>
  );
}

/** How to draw a Fountain block: the Labs entry hands it to the page. */
export const fountainBlock: BlockRenderer = (client, id): BlockView => {
  const host = document.createElement("div");
  host.className = "block block-fountain";
  let state: FountainState | null = null;
  const draw = () => render(<FountainBlock client={client} id={id} s={state} />, host);
  draw();
  const off = client.subscribe(draw);
  return {
    host,
    setVisible: (v) => (host.style.display = v ? "" : "none"),
    update: (s) => {
      state = s as FountainState;
      draw();
    },
    title: () => "Fountain",
    text: () => plain(state),
    focus: () => host.querySelector<HTMLElement>("input")?.focus(),
    dispose: () => {
      off();
      render(null, host);
      host.remove();
    },
  };
};
