// The Agents page (#403, Labs: `agents`): this machine's agent recipes
// (Claude Code subagent files), and the team's agents.
//
// Recipes: every recipe this machine has, in `~/.claude/agents` and in the
// `.claude/agents` of each project it knows (added here, offered from, or
// open in a pane). New and Edit are a form that writes the file as Claude
// Code reads it (keys it doesn't know are kept); Run here opens an agent
// block wearing it; Offer and Stop offering put it in the team's catalog;
// Delete removes the file. Team: the catalog, by machine and owner; tasks
// from other people waiting for you; and standing grants, with Revoke. All
// of it the shown machine's, and its owner's.

import { useEffect, useState } from "preact/hooks";
import type { Client } from "../client";
import { getFleet } from "./hosts";
import { directory } from "../hosts";
import { askText } from "../../shared/ui/menu";
import { ConfirmRemove } from "../../shared/ui/confirm";
import { agentsRoute, closeAgentsPage, openAgentsPage, type AgentsTab } from "./agents-route";
import { AppBar } from "./app-bar";
import { CopyText } from "../../shared/ui/copy";
import { Time } from "../../shared/ui/time";

interface Recipe {
  name: string;
  description: string;
  prompt: string;
  tools: string[];
  disallowed_tools: string[];
  model: string | null;
  permission_mode: string | null;
  skills: string[];
  mcp_servers: (string | { name: string; config: unknown })[];
  path: string;
  offered_from: string | null;
}
interface Recipes {
  user: { dir: string; recipes: Recipe[] };
  projects: { dir: string; added: boolean; recipes: Recipe[] }[];
}
interface Shelf {
  machine: string;
  name: string;
  owner: string;
  here: boolean;
  online: boolean;
  note: string | null;
  fetched_ms: number | null;
  agents: { name: string; description: string; skills?: { tags?: string[] }[] }[];
}
interface Waiting {
  id: string;
  history?: { parts: { text?: string }[] }[];
  metadata?: { arugula?: { agent?: string; caller?: string } };
}
interface Grant {
  account: string;
  name: string;
  agent: string;
  until_ms: number;
}

/** The page, while the route says so and the machine has the flag. */
export function AgentsPage({ client }: { client: Client }) {
  const [, setTick] = useState(0);
  useEffect(() => {
    const fn = () => setTick((t) => t + 1);
    window.addEventListener("hashchange", fn);
    return () => window.removeEventListener("hashchange", fn);
  }, []);
  const tab = agentsRoute();
  if (!tab || !client.flag("agents")) return null;
  return <Page client={client} tab={tab} />;
}

function Page({ client, tab }: { client: Client; tab: AgentsTab }) {
  return (
    <div class="chat agents-page" data-agents-page={tab}>
      <AppBar client={client} at="agents">
        <span class="agents-tabs" role="tablist" aria-label="Agents">
          <button role="tab" aria-selected={tab === "recipes"} class={tab === "recipes" ? "on" : ""} data-agents-tab="recipes" onClick={() => openAgentsPage("recipes")}>
            Recipes
          </button>
          <button role="tab" aria-selected={tab === "team"} class={tab === "team" ? "on" : ""} data-agents-tab="team" onClick={() => openAgentsPage("team")}>
            Team
          </button>
        </span>
      </AppBar>
      <div class="agents-body">{tab === "recipes" ? <RecipesTab client={client} /> : <TeamTab client={client} />}</div>
    </div>
  );
}

async function json<T>(client: Client, method: string, path: string, body?: unknown): Promise<T> {
  const r = await client.request(method, path, body);
  const v = await r.json<T & { error?: string }>().catch(() => ({}) as T & { error?: string });
  if (!r.ok) throw new Error(v.error ?? `HTTP ${r.status}`);
  return v;
}

const EMPTY: Form = { name: "", description: "", model: "", tools: "", disallowedTools: "", permissionMode: "", skills: "", mcpServers: "", prompt: "" };
interface Form {
  path?: string;
  dir?: string;
  name: string;
  description: string;
  model: string;
  tools: string;
  disallowedTools: string;
  permissionMode: string;
  skills: string;
  mcpServers: string;
  prompt: string;
}
// An offer changed since the Team tab last asked: its next look asks every
// machine again, so what you just offered is there.
let teamStale = false;

const list = (s: string) => s.split(",").map((x) => x.trim()).filter(Boolean);
const named = (r: Recipe) => r.mcp_servers.filter((m): m is string => typeof m === "string");
const inline = (r: Recipe) => r.mcp_servers.filter((m) => typeof m !== "string").map((m) => (m as { name: string }).name);

function RecipesTab({ client }: { client: Client }) {
  const [data, setData] = useState<Recipes | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [form, setForm] = useState<{ f: Form; inline: string[] } | null>(null);
  const [busy, setBusy] = useState(false);
  const [deleting, setDeleting] = useState<Recipe | null>(null);
  const [said, setSaid] = useState<string | null>(null);
  const load = async () => {
    try {
      setData(await json<Recipes>(client, "GET", "/api/a2a/recipes/all"));
      setError(null);
    } catch (e) {
      setError((e as Error).message);
    }
  };
  useEffect(() => void load(), []);
  const act = async (what: () => Promise<unknown>, done?: string) => {
    setBusy(true);
    try {
      await what();
      if (done) setSaid(done);
      await load();
    } catch (e) {
      setSaid((e as Error).message);
    }
    setBusy(false);
  };
  const edit = (r: Recipe) =>
    setForm({
      f: {
        path: r.path,
        name: r.name,
        description: r.description,
        model: r.model ?? "",
        tools: r.tools.join(", "),
        disallowedTools: r.disallowed_tools.join(", "),
        permissionMode: r.permission_mode ?? "",
        skills: r.skills.join(", "),
        mcpServers: named(r).join(", "),
        prompt: r.prompt,
      },
      inline: inline(r),
    });
  const save = (f: Form) =>
    act(async () => {
      await json(client, "POST", "/api/a2a/recipes/save", {
        path: f.path,
        dir: f.dir || undefined,
        name: f.name,
        description: f.description,
        model: f.model || undefined,
        tools: list(f.tools),
        disallowedTools: list(f.disallowedTools),
        permissionMode: f.permissionMode || undefined,
        skills: list(f.skills),
        mcpServers: list(f.mcpServers),
        prompt: f.prompt,
      });
      setForm(null);
    }, `${f.name} saved`);
  const offer = async (r: Recipe, project: string | null) => {
    const dir = project ?? (await askText(`Offer ${r.name}, working in`, "", "a project folder (a git checkout)"));
    if (!dir?.trim()) return;
    teamStale = true;
    await act(() => json(client, "POST", "/api/a2a/offers", { agent: r.name, dir: dir.trim() }), `${r.name} is offered to your team`);
  };
  const runHere = async (r: Recipe, project: string | null) => {
    const cwd = project ?? (await askText(`Run ${r.name} in`, "~", "a folder on this machine"));
    if (!cwd?.trim()) return;
    const prompt = await askText(`Ask ${r.name}`, "", "what to do (empty: just open it)");
    if (prompt === null) return;
    await client.openBlock(
      { type: "agent", config: { agent: "claude", recipe: r.name, cwd: cwd.trim(), ...(prompt.trim() ? { prompt: prompt.trim() } : {}) }, local: true },
      `couldn't run ${r.name}`,
    );
    closeAgentsPage();
  };
  const addProject = async () => {
    const dir = await askText("A project with .claude/agents", "~/", "a folder on this machine");
    if (dir?.trim()) await act(() => json(client, "POST", "/api/a2a/recipes/projects", { dir: dir.trim() }));
  };
  if (error) return <p class="control-error agents-error" data-agents-error>{error}</p>;
  if (!data) return <p class="dim">Reading this machine's recipes…</p>;
  const where = [{ label: "~/.claude/agents (yours, every project)", dir: "" }, ...data.projects.map((p) => ({ label: p.dir, dir: p.dir }))];
  // Paths under the home directory read from ~ (the full ones are long).
  const home = data.user.dir.replace(/\/\.claude\/agents\/?$/, "");
  const tilde = (d: string) => (home && (d === home || d.startsWith(`${home}/`)) ? `~${d.slice(home.length)}` : d);
  const withRecipes = data.projects.filter((p) => p.recipes.length);
  const empty = data.projects.filter((p) => !p.recipes.length);
  const removeProject = (dir: string) => (
    <button class="agents-remove" title="Stop listing this project here (its files stay)" onClick={() => void act(() => json(client, "POST", "/api/a2a/recipes/projects", { dir, keep: false }))}>
      Remove
    </button>
  );
  const rows = (rs: Recipe[], project: string | null) => (
    <ul class="agents-cards">
      {rs.map((r) => (
        <li key={r.path} class="agents-card" data-recipe={r.name} data-offered={String(!!r.offered_from)}>
          <div class="agents-card-text">
            <div class="agents-card-head">
              <b>{r.name}</b>
              {r.model && <span class="ws-tag">{r.model}</span>}
              {r.offered_from && (
                <span class="ws-tag waits" title={`Your team's agents can send it tasks; it works in ${r.offered_from}`}>
                  offered to your team
                </span>
              )}
            </div>
            <p class="dim">{r.description}</p>
          </div>
          <div class="agents-buttons">
            <button class="primary small" data-run-here={r.name} disabled={busy} title="Open an agent block wearing this recipe, in its project" onClick={() => void runHere(r, project)}>
              Run on {directory.current || "this machine"}…
            </button>
            <button class="secondary small" data-edit={r.name} disabled={busy} title="Change its prompt, tools and model" onClick={() => edit(r)}>
              Edit
            </button>
            {r.offered_from ? (
              <button class="secondary small"
                data-unoffer={r.name}
                disabled={busy}
                title="Take it out of your team's catalog"
                onClick={() => {
                  teamStale = true;
                  void act(() => json(client, "POST", "/api/a2a/offers", { agent: r.name }), `${r.name} is no longer offered`);
                }}
              >
                Stop offering
              </button>
            ) : (
              <button class="secondary small" data-offer={r.name} disabled={busy} title="Put it in your team's catalog, so their agents can send it tasks" onClick={() => void offer(r, project)}>
                Offer to team
              </button>
            )}
            <button class="secondary danger small" data-delete={r.name} disabled={busy} title="Delete its file" onClick={() => setDeleting(r)}>
              Delete
            </button>
          </div>
        </li>
      ))}
    </ul>
  );
  const none = !data.user.recipes.length && data.projects.every((p) => !p.recipes.length);
  return (
    <div class="agents-recipes-tab">
      <p class="agents-lead">
        A recipe is a Claude Code subagent: a name, a prompt, and the tools and model it uses. <b>Run on {directory.current || "this machine"}…</b> asks what to do, then opens an agent wearing it on this machine.{" "}
        <b>Offer to team</b> lets your team's agents send it tasks; you approve each one.
      </p>
      <div class="agents-actions">
        <button class="primary small" data-new-recipe disabled={busy} onClick={() => setForm({ f: { ...EMPTY }, inline: [] })}>
          New recipe
        </button>
        <button class="secondary small" data-add-project disabled={busy} title="List the recipes in a project's .claude/agents" onClick={() => void addProject()}>
          Add a project…
        </button>
        {said && (
          <span class="agents-said" data-agents-said>
            {said}
          </span>
        )}
      </div>
      {none && !form && (
        <div class="agents-empty" data-agents-empty>
          <b>No recipes on this machine yet.</b>
          <p>
            Make one with <b>New recipe</b>, or <b>Add a project…</b> that already has recipes in its <code>.claude/agents</code> folder.
          </p>
        </div>
      )}
      {form && <RecipeForm form={form.f} inline={form.inline} where={where} busy={busy} save={(f) => void save(f)} cancel={() => setForm(null)} />}
      {/* The machine these are on: the Recipes tab is one machine's. */}
      <h3 class="agents-machine" data-agents-machine>
        Recipes on {directory.current || "this machine"}
      </h3>
      {/* Where there are recipes first; the places with none, after, on
          one line each. */}
      {data.user.recipes.length ? (
        <section class="agents-recipes" data-agents-scope="user">
          <h4 class="agents-section-head">
            Yours, in every project <span class="dim">{tilde(data.user.dir)}</span>
          </h4>
          {rows(data.user.recipes, null)}
        </section>
      ) : null}
      {withRecipes.map((p) => (
        <section key={p.dir} class="agents-recipes" data-agents-scope={p.dir}>
          <h4 class="agents-section-head">
            {p.dir.split("/").pop()} <span class="dim">{tilde(p.dir)}</span>
            {p.added && removeProject(p.dir)}
          </h4>
          {rows(p.recipes, p.dir)}
        </section>
      ))}
      {(empty.length > 0 || !data.user.recipes.length) && (
        <section class="agents-recipes agents-empties">
          <h4 class="agents-section-head">No recipes yet</h4>
          <ul class="agents-empty-list">
            {!data.user.recipes.length && (
              <li data-agents-scope="user">
                Yours, in every project <span class="dim">{tilde(data.user.dir)}</span>
                <button class="secondary small agents-inline-new" disabled={busy} onClick={() => setForm({ f: { ...EMPTY, dir: "" }, inline: [] })}>
                  New recipe here
                </button>
              </li>
            )}
            {empty.map((p) => (
              <li key={p.dir} data-agents-scope={p.dir}>
                {p.dir.split("/").pop()} <span class="dim">{tilde(p.dir)} · No recipes in .claude/agents</span>
                <button class="secondary small agents-inline-new" disabled={busy} onClick={() => setForm({ f: { ...EMPTY, dir: p.dir }, inline: [] })}>
                  New recipe here
                </button>
                {p.added && removeProject(p.dir)}
              </li>
            ))}
          </ul>

        </section>
      )}
      {deleting && (
        <ConfirmRemove
          title={`Delete ${deleting.name}?`}
          label="Delete"
          cancel={() => setDeleting(null)}
          go={() => {
            const r = deleting;
            setDeleting(null);
            void act(() => json(client, "POST", "/api/a2a/recipes/delete", { path: r.path }), `${r.name} deleted`);
          }}
        >
          <p>
            Removes <code>{deleting.path}</code>
            {deleting.offered_from ? ", and stops offering it" : ""}. Claude Code won't have it either.
          </p>
        </ConfirmRemove>
      )}
    </div>
  );
}

function RecipeForm({
  form,
  inline,
  where,
  busy,
  save,
  cancel,
}: {
  form: Form;
  inline: string[];
  where: { label: string; dir: string }[];
  busy: boolean;
  save: (f: Form) => void;
  cancel: () => void;
}) {
  const [f, setF] = useState<Form>(form);
  const set = (k: keyof Form) => (e: Event) => setF({ ...f, [k]: (e.currentTarget as HTMLInputElement).value });
  const field = (k: keyof Form, label: string, hint = "") => (
    <label class="agents-field">
      <span>{label}</span>
      <input name={k} value={f[k] as string} placeholder={hint} onInput={set(k)} autocomplete="off" spellcheck={false} />
    </label>
  );
  return (
    <form
      class="agents-form"
      data-recipe-form={f.path ? "edit" : "new"}
      onSubmit={(e) => {
        e.preventDefault();
        save(f);
      }}
    >
      <h4 class="agents-form-title">{f.path ? `Edit ${form.name}` : "New recipe"}</h4>
      <p class="dim agents-form-help">Saved as a Markdown file Claude Code reads. Name and description are required; leave the rest empty for Claude Code's defaults.</p>
      {!f.path && (
        <label class="agents-field">
          <span>Where</span>
          <select name="dir" value={f.dir ?? ""} onChange={set("dir")}>
            {where.map((w) => (
              <option key={w.dir} value={w.dir}>
                {w.label}
              </option>
            ))}
          </select>
        </label>
      )}
      {f.path && <p class="dim">{f.path}</p>}
      {field("name", "Name", "fixer")}
      {field("description", "Description", "what it's for: the catalog and other agents read this")}
      {field("model", "Model", "sonnet, opus, haiku, or empty for the default")}
      {field("tools", "Tools", "Read, Edit, Bash (empty: all)")}
      {field("disallowedTools", "Never", "tools it may not use")}
      <label class="agents-field">
        <span>Permissions</span>
        <select name="permissionMode" value={f.permissionMode} onChange={set("permissionMode")}>
          <option value="">ask (default)</option>
          <option value="acceptEdits">accept edits</option>
          <option value="plan">plan</option>
          <option value="bypassPermissions">skip every check (never for others' tasks)</option>
        </select>
      </label>
      {field("skills", "Skills", "by name, from .claude/skills")}
      {field("mcpServers", "MCP servers", "by name, from .mcp.json or ~/.claude.json")}
      {inline.length > 0 && <p class="dim">Also written in the file (kept): {inline.join(", ")}</p>}
      <label class="agents-field agents-prompt">
        <span>Prompt</span>
        <textarea name="prompt" value={f.prompt} rows={10} onInput={set("prompt")} placeholder="You are … When asked to …" />
      </label>
      <div class="ws-actions">
        <button class="primary small" type="submit" data-save-recipe disabled={busy || !f.name.trim() || !f.description.trim()}>
          Save
        </button>
        <button class="secondary small" type="button" onClick={cancel}>
          Cancel
        </button>
      </div>
    </form>
  );
}

function TeamTab({ client }: { client: Client }) {
  const [shelves, setShelves] = useState<Shelf[] | null>(null);
  const [waiting, setWaiting] = useState<Waiting[]>([]);
  const [grants, setGrants] = useState<Grant[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const load = async (fresh = false) => {
    setBusy(true);
    try {
      const c = await json<{ machines: Shelf[] }>(client, "GET", `/api/a2a/catalog${fresh ? "?fresh=1" : ""}`);
      setShelves(c.machines);
      setWaiting((await json<{ waiting: Waiting[] }>(client, "GET", "/api/a2a/waiting").catch(() => ({ waiting: [] }))).waiting);
      setGrants(await json<Grant[]>(client, "GET", "/api/a2a/grants").catch(() => []));
      setError(null);
    } catch (e) {
      setError((e as Error).message);
    }
    setBusy(false);
  };
  useEffect(() => {
    const fresh = teamStale;
    teamStale = false;
    void load(fresh);
  }, []);
  // One of the account's own agents, run in its project on its machine
  // (this one or another), and gone to.
  const run = async (s: Shelf, agent: string) => {
    const prompt = await askText(`Ask ${agent}${s.here ? "" : ` on ${s.name}`}`, "", "what to do (empty: just open it)");
    if (prompt === null) return;
    setBusy(true);
    try {
      const r = await json<{ block: number }>(client, "POST", "/api/a2a/run", { machine: s.here ? null : s.machine, agent, prompt: prompt.trim() || null });
      closeAgentsPage();
      if (s.here) client.setActive(r.block);
      else {
        const fleet = getFleet();
        fleet?.open(fleet.list.find((h) => h.id === s.machine)?.name ?? s.name, r.block);
      }
    } catch (e) {
      client.toast(`couldn't run ${agent}: ${(e as Error).message}`);
    }
    setBusy(false);
  };
  const answer = async (id: string, a: "once" | "hour" | "deny") => {
    setBusy(true);
    await client.request("POST", `/api/a2a/tasks/${encodeURIComponent(id)}/consent`, { answer: a }).catch(() => null);
    await load();
  };
  if (error) return <p class="control-error agents-error" data-agents-error>{error}</p>;
  if (!shelves) return <p class="dim">Asking every machine for its agents…</p>;
  // This machine, then your own, then your teammates', each by name
  // whatever its case (UI audit #173: WIN-… sorted before arugula-…).
  const rank = (s: Shelf) => (s.here ? 0 : s.owner ? 2 : 1);
  const sorted = [...shelves].sort((a, b) => rank(a) - rank(b) || a.name.localeCompare(b.name, undefined, { sensitivity: "base" }));
  const offered = sorted.filter((s) => s.agents.length);
  const quiet = sorted.filter((s) => !s.agents.length);
  return (
    <div class="agents-team-tab">
      <p class="agents-lead">
        The agents your team offers, on every machine, not just {directory.current || "this one"}. Your agents send them tasks with the <code>delegate</code> tool. When someone else's agent asks one of
        yours, the task waits here until you allow it.
      </p>
      <div class="agents-actions">
        <button class="secondary small" data-team-refresh disabled={busy} onClick={() => void load(true)}>
          {busy ? "Refreshing…" : "Refresh"}
        </button>
      </div>
      {waiting.length > 0 && (
        <section class="agents-recipes" data-agents-waiting>
          <h4 class="agents-section-head">Waiting for you</h4>
          <ul class="agents-cards">
            {waiting.map((t) => (
              <li key={t.id} class="agents-card waiting" data-waiting={t.id}>
                <div class="agents-card-text">
                  <div class="agents-card-head">
                    <b>{t.metadata?.arugula?.caller ?? "someone"}</b>
                    <span class="dim">asks {t.metadata?.arugula?.agent}</span>
                  </div>
                  <p>{t.history?.[0]?.parts?.[0]?.text}</p>
                </div>
                <div class="agents-buttons">
                  <button class="primary small" disabled={busy} onClick={() => void answer(t.id, "once")}>
                    Allow once
                  </button>
                  <button class="secondary small" disabled={busy} title="Allow this person's tasks for this agent for an hour" onClick={() => void answer(t.id, "hour")}>
                    Allow for an hour
                  </button>
                  <button class="secondary danger small" disabled={busy} onClick={() => void answer(t.id, "deny")}>
                    Deny
                  </button>
                </div>
              </li>
            ))}
          </ul>
        </section>
      )}
      {!offered.length && (
        <div class="agents-empty" data-agents-empty>
          <b>Nobody on your team offers an agent yet.</b>
          <p>
            Offer one of yours: open{" "}
            <button class="agents-inline" onClick={() => openAgentsPage("recipes")}>
              Recipes
            </button>{" "}
            and choose <b>Offer to team</b> on a recipe.
          </p>
        </div>
      )}
      {offered.map((s) => (
        <section key={s.machine} class="agents-recipes agents-shelf" data-shelf={s.name}>
          <h4 class="agents-section-head">
            {s.name}
            <span class="dim">
              {s.here ? " · this machine" : ""}
              {s.owner ? ` · ${s.owner}'s` : ""}
              {!s.online ? (
                <>
                  {" "}
                  · offline{s.fetched_ms ? <>, as seen <Time at={s.fetched_ms} /></> : ""}
                </>
              ) : (
                ""
              )}
            </span>
          </h4>
          {s.note && s.online && <p class="dim offer-said">{s.note}</p>}
          <ul class="agents-cards">
            {s.agents.map((a) => (
              <li key={a.name} class="agents-card" data-agent={a.name}>
                <div class="agents-card-text">
                  <div class="agents-card-head">
                    <b>{a.name}</b>
                  </div>
                  <p class="dim">{a.description}</p>
                  <span class="agents-how" title="Ask one of your agents to send it a task with this (pr: true to have it open a pull request in its own project)">
                    <CopyText inline text={`delegate {agent: ${a.name}${s.here ? "" : `, machine: ${s.name}`}}`} data-delegate={a.name} />
                  </span>
                </div>
                {!s.owner && s.online && (
                  <div class="agents-buttons">
                    <button class="primary small" data-run={a.name} disabled={busy} title={`Run it in its project on ${s.name}, and go to it`} onClick={() => void run(s, a.name)}>
                      Run on {s.name}…
                    </button>
                  </div>
                )}
              </li>
            ))}
          </ul>
        </section>
      ))}
      {/* Every other machine of the team's, and why it lists none: so a
          missing machine isn't a mystery. */}
      {quiet.length > 0 && (
        <section class="agents-recipes agents-empties" data-agents-quiet>
          <h4 class="agents-section-head">No agents listed</h4>
          <ul class="agents-empty-list">
            {quiet.map((q) => (
              <li key={q.machine} data-quiet={q.name} title={q.note ?? undefined}>
                {q.name}
                {/* The row's gap is its space: no second one before the "·". */}
                <span class="dim">
                  {[q.here ? "this machine" : "", q.owner ? `${q.owner}'s` : "", q.online ? "offers no agents" : q.note?.startsWith("offline") ? "offline" : "didn't answer"]
                    .filter(Boolean)
                    .map((x) => `· ${x}`)
                    .join(" ")}
                </span>
              </li>
            ))}
          </ul>
        </section>
      )}
      {grants.length > 0 && (
        <section class="agents-recipes" data-agents-grants>
          <h4 class="agents-section-head">Standing grants</h4>
          <ul class="agents-cards">
            {grants.map((g) => (
              <li key={`${g.account}/${g.agent}`} class="agents-card" data-grant={`${g.name}/${g.agent}`}>
                <div class="agents-card-text">
                  <div class="agents-card-head">
                    <b>{g.name}</b>
                    <span class="dim">
                      may use {g.agent} · ends <Time at={g.until_ms} title="Until" />
                    </span>
                  </div>
                </div>
                <div class="agents-buttons">
                  <button class="secondary danger small" disabled={busy} onClick={() => void client.request("POST", "/api/a2a/grants", { account: g.account, agent: g.agent }).then(() => load())}>
                    Revoke
                  </button>
                </div>
              </li>
            ))}
          </ul>
        </section>
      )}
    </div>
  );
}
