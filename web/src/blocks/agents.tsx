// The agents block (M77): the team's agent catalog. The daemon asks every
// machine it reaches (the owner's, the team's, teammates' that offer agents)
// for its A2A cards; here they're grouped by machine, its owner named, an
// offline machine's shown as last seen. Below, this machine's recipes (Claude
// Code subagent files in a project): *Offer* and *Stop offering*, and *Run
// here* for one this machine offers. Those are the owner's; *Send a task*
// to another machine's agent comes with M78.

import { render } from "preact";
import { useState } from "preact/hooks";
import type { Client } from "../client";
import type { PaneId } from "../proto";
import { askText } from "../ui/menu";
import type { BlockRenderer, BlockView } from "./view";

interface Card {
  name: string;
  description: string;
  skills?: { tags?: string[] }[];
}
interface Shelf {
  machine: string;
  name: string;
  owner: string;
  team: string | null;
  here: boolean;
  online: boolean;
  agents: Card[];
  fetched_ms: number | null;
  note: string | null;
}
interface Recipe {
  name: string;
  description: string;
  path: string;
  model: string | null;
  offered_from: string | null;
}
export interface AgentsState {
  catalog: { machines: Shelf[]; refreshed_ms: number; note: string | null };
  loading: boolean;
  error: string | null;
  dir: string | null;
  recipes: Recipe[];
  recipes_error: string | null;
  said: string | null;
  updated_ms: number;
}

const model = (c: Card) => c.skills?.[0]?.tags?.find((t) => t !== "claude-code") ?? null;

function ago(ms: number | null): string {
  if (!ms) return "";
  const s = Math.max(0, (Date.now() - ms) / 1000);
  if (s < 90) return "just now";
  if (s < 3600) return `${Math.round(s / 60)}m ago`;
  if (s < 86400) return `${Math.round(s / 3600)}h ago`;
  return `${Math.round(s / 86400)}d ago`;
}

function plain(s: AgentsState | null): string {
  if (!s) return "Team agents";
  const lines = ["Team agents"];
  for (const m of s.catalog.machines) for (const c of m.agents) lines.push(`  ${c.name} on ${m.name}${m.owner ? ` (${m.owner})` : ""}: ${c.description}`);
  return lines.join("\n");
}

function AgentsBlock({ client, id, s }: { client: Client; id: PaneId; s: AgentsState | null }) {
  const [busy, setBusy] = useState<string | null>(null);
  const session = client.sessionOfTab(client.tabOfPane(id)?.id ?? -1) ?? null;
  const mayOwn = client.role(session) === "owner";
  const call = async (method: string, args: unknown, failure: string, key = method) => {
    setBusy(key);
    const ok = await client.api(`/api/blocks/${id}/call/${method}`, args, failure);
    setBusy(null);
    return ok;
  };
  if (!s || (s.loading && !s.updated_ms)) {
    return (
      <div class="review ws fountain agents" data-agents-block={id}>
        <div class="browser-card dim">Asking every machine for its agents…</div>
      </div>
    );
  }
  const shelves = s.catalog.machines.filter((m) => m.agents.length > 0 || m.note);
  const total = s.catalog.machines.reduce((n, m) => n + m.agents.length, 0);
  const pickDir = async () => {
    const dir = await askText("A project's recipes (.claude/agents)", s.dir ?? "~", "a folder on this machine");
    if (dir?.trim()) await call("recipes", { dir: dir.trim() }, "couldn't read its recipes");
  };
  const runHere = async (name: string) => {
    const prompt = await askText(`Ask ${name}`, "", "what to do (empty: just open it)");
    if (prompt === null) return;
    await call("run_here", { agent: name, prompt: prompt.trim() }, `couldn't run ${name} here`, `run:${name}`);
  };
  return (
    <div class="review ws fountain agents" data-agents-block={id}>
      <div class="review-bar">
        <span class="review-path">
          <b>Team agents</b>
        </span>
        <span class="dim" data-agents-count>
          {total} on {s.catalog.machines.length} machine{s.catalog.machines.length === 1 ? "" : "s"}
        </span>
        <span class="dim">{s.loading ? "asking…" : ago(s.catalog.refreshed_ms)}</span>
        <button title="Ask every machine again" data-agents-refresh disabled={busy !== null || s.loading} onClick={() => void call("refresh", {}, "couldn't refresh the catalog")}>
          {busy === "refresh" ? "…" : "↻"}
        </button>
      </div>
      {s.error && (
        <div class="browser-card" data-agents-error>
          <p class="dim">{s.error}</p>
        </div>
      )}
      {s.said && <p class="dim fountain-said">{s.said}</p>}
      {s.catalog.note && <p class="dim fountain-said">{s.catalog.note}</p>}
      <div class="review-body">
        {total === 0 && !s.error && <p class="dim fountain-empty">No agents offered on any machine you reach</p>}
        {shelves.map((m) => (
          <section key={m.machine} class="agents-shelf" data-shelf={m.name} data-online={String(m.online)}>
            <h4 class="agents-shelf-head">
              {m.name}
              <span class="dim">
                {m.here ? " · this machine" : ""}
                {m.owner ? ` · ${m.owner}'s` : ""}
                {m.team ? " · team" : ""}
                {!m.online ? ` · offline, as seen ${ago(m.fetched_ms)}` : ""}
              </span>
            </h4>
            {m.note && m.online && <p class="dim fountain-said">{m.note}</p>}
            <ul class="fountain-cards">
              {m.agents.map((c) => (
                <li key={`${m.machine}/${c.name}`} class={`fountain-card ${m.online ? "" : "dim"}`} data-agent={c.name} data-on={m.name}>
                  <div class="fountain-card-head">
                    <b class="fountain-name">{c.name}</b>
                    {model(c) && <span class="ws-tag">{model(c)}</span>}
                  </div>
                  {c.description && <p class="fountain-desc">{c.description}</p>}
                  <div class="ws-actions fountain-actions">
                    {m.here && mayOwn && (
                      <button class="pri" data-run-here={c.name} disabled={busy !== null} onClick={() => void runHere(c.name)}>
                        Run here
                      </button>
                    )}
                    {!m.here && (
                      <button data-send-task={c.name} disabled title="Sending another person's agent a task comes next (M78)">
                        Send a task
                      </button>
                    )}
                  </div>
                </li>
              ))}
            </ul>
          </section>
        ))}
        {mayOwn && (
          <section class="agents-recipes" data-agents-recipes>
            <h4 class="agents-shelf-head">
              Offer from this machine
              <button class="fountain-link" data-pick-dir onClick={() => void pickDir()}>
                {s.dir ? s.dir : "Pick a project…"}
              </button>
            </h4>
            {s.recipes_error && <p class="dim fountain-said">{s.recipes_error}</p>}
            {s.dir && !s.recipes_error && s.recipes.length === 0 && <p class="dim">No recipes in {s.dir}/.claude/agents or ~/.claude/agents</p>}
            <ul class="agents-recipe-list">
              {s.recipes.map((r) => (
                <li key={r.path} data-recipe={r.name} data-offered={String(!!r.offered_from)}>
                  <b>{r.name}</b> <span class="dim">{r.description}</span>
                  {r.offered_from ? (
                    <button data-unoffer={r.name} disabled={busy !== null} title={`Offered from ${r.offered_from}`} onClick={() => void call("unoffer", { agent: r.name }, `couldn't stop offering ${r.name}`)}>
                      Stop offering
                    </button>
                  ) : (
                    <button data-offer={r.name} disabled={busy !== null} title={`Your team's agents may find it, and (from M78) send it tasks you approve, working in ${s.dir}`} onClick={() => void call("offer", { agent: r.name, dir: s.dir }, `couldn't offer ${r.name}`)}>
                      Offer
                    </button>
                  )}
                </li>
              ))}
            </ul>
          </section>
        )}
      </div>
    </div>
  );
}

/** How to draw an agents block: the Labs entry hands it to the page. */
export const agentsBlock: BlockRenderer = (client, id): BlockView => {
  const host = document.createElement("div");
  host.className = "block block-fountain block-agents";
  let state: AgentsState | null = null;
  const draw = () => render(<AgentsBlock client={client} id={id} s={state} />, host);
  draw();
  const off = client.subscribe(draw);
  return {
    host,
    setVisible: (v) => (host.style.display = v ? "" : "none"),
    update: (s) => {
      state = s as AgentsState;
      draw();
    },
    title: () => "Team agents",
    text: () => plain(state),
    focus: () => host.querySelector<HTMLElement>("button")?.focus(),
    dispose: () => {
      off();
      render(null, host);
      host.remove();
    },
  };
};
