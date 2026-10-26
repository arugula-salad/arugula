// The agents block (M77): the team's agent catalog. The daemon asks every
// machine it reaches (the owner's, the team's, teammates' that offer agents)
// for its A2A cards; here they're grouped by machine, its owner named, an
// offline machine's shown as last seen. Below, this machine's recipes (Claude
// Code subagent files in a project): *Offer* and *Stop offering*, and *Run
// here* for one this machine offers, *Run there* for one another of the
// account's machines offers (it starts there, and the page goes to it).
// Those are the owner's. M79: tasks
// from other people wait here for the owner (the card on this block allows
// or denies one), and standing grants are listed with *Revoke*. Agents send
// tasks with MCP's `delegate`; people with `arugula agents send`.

import { render } from "preact";
import { useState } from "preact/hooks";
import type { Client } from "../client";
import type { PaneId } from "../../shared/proto";
import { getFleet } from "../ui/hosts";
import { askText } from "../../shared/ui/menu";
import { BlockEmpty, BlockError, BlockLoading } from "./block-states";
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
interface WaitingTask {
  id: string;
  agent: string;
  caller: string;
  device: string | null;
  text: string;
  dir: string;
  at_ms: number;
}
interface Grant {
  account: string;
  name: string;
  agent: string;
  until_ms: number;
}
export interface AgentsState {
  waiting?: WaitingTask[];
  grants?: Grant[];
  catalog: { machines: Shelf[]; refreshed_ms: number; note: string | null };
  loading: boolean;
  error: string | null;
  dir: string | null;
  recipes: Recipe[];
  recipes_error: string | null;
  said: string | null;
  updated_ms: number;
}

/** A grant's end: the time today, else the day and time (UI audit #178:
 * a time alone read as today's). */
function until(ms: number): string {
  const d = new Date(ms);
  const today = d.toDateString() === new Date().toDateString();
  return d.toLocaleString(undefined, { ...(today ? {} : { month: "short", day: "numeric" }), hour: "numeric", minute: "2-digit" });
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
      <div class="review ws agents" data-agents-block={id}>
        <BlockLoading what="the team's agents" />
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
  // Started on that machine, in its project: go there.
  const runThere = async (m: Shelf, name: string) => {
    const prompt = await askText(`Ask ${name} on ${m.name}`, "", "what to do (empty: just open it)");
    if (prompt === null) return;
    setBusy(`there:${m.machine}/${name}`);
    try {
      const res = await client.request("POST", `/api/blocks/${id}/call/run_there`, { machine: m.machine, agent: name, prompt: prompt.trim() });
      const body = await res.json<{ block?: number; error?: string }>().catch(() => null);
      if (!res.ok || typeof body?.block !== "number") {
        client.toast(body?.error ?? `couldn't run ${name} on ${m.name} (${res.status})`);
        return;
      }
      const fleet = getFleet();
      fleet?.open(fleet.list.find((h) => h.id === m.machine)?.name ?? m.name, body.block);
    } catch {
      client.toast(`couldn't run ${name} on ${m.name}`);
    } finally {
      setBusy(null);
    }
  };
  return (
    <div class="review ws agents" data-agents-block={id}>
      <div class="review-bar">
        <span class="review-path">
          <b>Team agents</b>
        </span>
        <span class="dim" data-agents-count>
          {total} on {s.catalog.machines.length} machine{s.catalog.machines.length === 1 ? "" : "s"}
        </span>
        <span class="dim">{s.loading ? "asking…" : ago(s.catalog.refreshed_ms)}</span>
        <button title="Ask every machine for its agents again" data-agents-refresh disabled={busy !== null || s.loading} onClick={() => void call("refresh", {}, "couldn't refresh the catalog")}>
          {busy === "refresh" ? "…" : "↻"}
        </button>
      </div>
      {s.error && (
        <BlockError
          inline
          data-agents-error
          title="Couldn't ask every machine for its agents"
          detail={s.error}
          retry={busy === null && !s.loading ? () => void call("refresh", {}, "couldn't refresh the catalog") : undefined}
        />
      )}
      {s.said && <p class="dim offer-said">{s.said}</p>}
      {(s.waiting?.length ?? 0) > 0 && (
        <section class="agents-recipes" data-agents-waiting>
          <h4 class="agents-shelf-head">Waiting for you</h4>
          <ul class="agents-recipe-list">
            {s.waiting!.map((t) => (
              <li key={t.id} data-waiting={t.id}>
                <b>{t.caller}</b>
                <span class="dim">
                  asks {t.agent}: {t.text}
                </span>
              </li>
            ))}
          </ul>
        </section>
      )}
      {s.catalog.note && <p class="dim offer-said">{s.catalog.note}</p>}
      <div class="review-body">
        {total === 0 && !s.error && <BlockEmpty data-agents-empty>No machine you reach offers an agent yet.</BlockEmpty>}
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
            {m.note && m.online && <p class="dim offer-said">{m.note}</p>}
            <ul class="offer-cards">
              {m.agents.map((c) => (
                <li key={`${m.machine}/${c.name}`} class={`offer-card ${m.online ? "" : "dim"}`} data-agent={c.name} data-on={m.name}>
                  <div class="offer-card-head">
                    <b class="offer-name">{c.name}</b>
                    {model(c) && <span class="ws-tag">{model(c)}</span>}
                  </div>
                  {c.description && <p class="offer-desc">{c.description}</p>}
                  <div class="ws-actions offer-actions">
                    {m.here && mayOwn && (
                      <button class="primary small" data-run-here={c.name} disabled={busy !== null} onClick={() => void runHere(c.name)}>
                        Run on {m.name}
                      </button>
                    )}
                    {!m.here && !m.owner && mayOwn && m.online && (
                      <button class="primary small" data-run-there={c.name} disabled={busy !== null} title={`Starts it on ${m.name}, in its project there, and goes to it`} onClick={() => void runThere(m, c.name)}>
                        Run on {m.name}
                      </button>
                    )}
                    {!m.here && (
                      <span class="dim" data-send-task={c.name} title={`Your agents send it tasks with Arugula's delegate tool; from a terminal: arugula agents send ${m.name} ${c.name}`}>
                        your agents can delegate to it
                      </span>
                    )}
                  </div>
                </li>
              ))}
            </ul>
          </section>
        ))}
        {mayOwn && (s.grants?.length ?? 0) > 0 && (
          <section class="agents-recipes" data-agents-grants>
            <h4 class="agents-shelf-head">Standing grants</h4>
            <ul class="agents-recipe-list">
              {s.grants!.map((g) => (
                <li key={`${g.account}/${g.agent}`} data-grant={`${g.name}/${g.agent}`}>
                  <b>{g.name}</b>
                  <span class="dim">
                    {g.agent}, until {until(g.until_ms)}
                  </span>
                  <button data-revoke={`${g.name}/${g.agent}`} disabled={busy !== null} onClick={() => void call("revoke", { account: g.account, agent: g.agent }, "couldn't take it back")}>
                    Revoke
                  </button>
                </li>
              ))}
            </ul>
          </section>
        )}
        {mayOwn && (
          <section class="agents-recipes" data-agents-recipes>
            <h4 class="agents-shelf-head">
              Offer from this machine
              <button class="offer-link" data-pick-dir onClick={() => void pickDir()}>
                {s.dir ? s.dir : "Pick a project…"}
              </button>
            </h4>
            {s.recipes_error && <p class="dim offer-said">{s.recipes_error}</p>}
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
                    <button data-offer={r.name} disabled={busy !== null} title={`Your team's agents can find it and send it tasks you approve; it works in ${s.dir}`} onClick={() => void call("offer", { agent: r.name, dir: s.dir }, `couldn't offer ${r.name}`)}>
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
  host.className = "block block-agents";
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
