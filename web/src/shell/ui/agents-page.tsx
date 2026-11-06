// The Agents page (#403, Labs: `agent-delegation`): this machine's agent recipes
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
//
// Both tabs are list pages (Arugula #590): a recipe's or an agent's buttons are
// in the preview of the row that's selected, and the recipe form takes the
// preview's place. A task waiting for you is answered without selecting
// anything, so it's a section above the list; the grants, and the places with
// nothing to list, are sections below it.

import { useEffect, useMemo, useRef, useState } from "preact/hooks";
import type { Client } from "../client";
import { getFleet } from "./hosts";
import { directory } from "../hosts";
import { askText, openMenu } from "../../shared/ui/menu";
import { ConfirmRemove } from "../../shared/ui/confirm";
import { ListCounts, ListItem, ListPage, ListPreview, ListSection, type ListCount, type ListGroup } from "../../shared/ui/list-page";
import { PageHeader } from "../../shared/ui/page-header";
import { agentsRoute, closeAgentsPage, openAgentsPage, type AgentsTab } from "./agents-route";
import { AppBar } from "./app-bar";
import { CopyText } from "../../shared/ui/copy";
import { Time } from "../../shared/ui/time";
import { Chip } from "../../shared/ui/state-chip";

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
  if (!tab || !client.flag("agent-delegation")) return null;
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
      {tab === "recipes" ? <RecipesTab client={client} /> : <TeamTab client={client} />}
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

/** A recipe, and the place it's listed in. */
interface RecipeRow {
  r: Recipe;
  project: string | null;
  /** Its project was added by hand, so it can be taken off the page. */
  added: boolean;
}

const baseName = (dir: string) => dir.split("/").pop();
const machineName = () => directory.current || "this machine";

/** The ⋯ of a preview: what's destructive. */
function moreMenu(e: MouseEvent, items: Parameters<typeof openMenu>[1]) {
  const el = e.currentTarget as HTMLElement;
  const b = el.getBoundingClientRect();
  openMenu({ clientX: b.left, clientY: b.bottom + 4, preventDefault: () => e.preventDefault() }, items, el);
}

function RecipesTab({ client }: { client: Client }) {
  const [data, setData] = useState<Recipes | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [form, setForm] = useState<{ f: Form; inline: string[] } | null>(null);
  const [busy, setBusy] = useState(false);
  const [deleting, setDeleting] = useState<Recipe | null>(null);
  const [said, setSaid] = useState<string | null>(null);
  const [filter, setFilter] = useState("all");
  const [query, setQuery] = useState("");
  const [select, setSelect] = useState<string | null>(null);
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
      setSaid(done ?? null);
      await load();
    } catch (e) {
      setSaid(null);
      client.toast((e as Error).message);
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
      const saved = await json<{ path: string }>(client, "POST", "/api/a2a/recipes/save", {
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
      // Show what was just made, whatever the list was narrowed to.
      setFilter("all");
      setQuery("");
      setSelect(saved.path);
    }, `${f.name} saved`);
  const offer = async (r: Recipe, project: string | null) => {
    const dir = project ?? (await askText(`Offer ${r.name}, working in`, "", "a project folder (a git checkout)"));
    if (!dir?.trim()) return;
    teamStale = true;
    await act(() => json(client, "POST", "/api/a2a/offers", { agent: r.name, dir: dir.trim() }), `${r.name} is offered to your team`);
  };
  const unoffer = (r: Recipe) => {
    teamStale = true;
    void act(() => json(client, "POST", "/api/a2a/offers", { agent: r.name }), `${r.name} is no longer offered`);
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
  const removeProject = (dir: string) => void act(() => json(client, "POST", "/api/a2a/recipes/projects", { dir, keep: false }));

  // Paths under the home directory read from ~ (the full ones are long).
  const home = (data?.user.dir ?? "").replace(/\/\.claude\/agents\/?$/, "");
  const tilde = (d: string) => (home && (d === home || d.startsWith(`${home}/`)) ? `~${d.slice(home.length)}` : d);
  const q = query.trim().toLowerCase();
  const fits = (r: Recipe) =>
    (filter === "all" || !!r.offered_from) &&
    (!q || r.name.toLowerCase().includes(q) || r.description.toLowerCase().includes(q) || (r.model ?? "").toLowerCase().includes(q) || r.prompt.toLowerCase().includes(q));
  const groups = useMemo(() => {
    if (!data) return [];
    const yours: ListGroup<RecipeRow> = {
      title: "Yours",
      detail: `${tilde(data.user.dir)} · every project`,
      items: data.user.recipes.filter(fits).map((r) => ({ r, project: null, added: false })),
      attrs: { "data-agents-scope": "user" },
    };
    const projects = data.projects.map(
      (p): ListGroup<RecipeRow> => ({
        title: baseName(p.dir),
        named: true,
        key: p.dir,
        detail: tilde(p.dir),
        items: p.recipes.filter(fits).map((r) => ({ r, project: p.dir, added: p.added })),
        attrs: { "data-agents-scope": p.dir },
      }),
    );
    return [yours, ...projects];
  }, [data, filter, q]);

  const all = data ? [...data.user.recipes, ...data.projects.flatMap((p) => p.recipes)] : [];
  const counts: ListCount[] = [
    { key: "all", n: all.length, label: all.length === 1 ? "recipe" : "recipes" },
    { key: "offered", n: all.filter((r) => r.offered_from).length, label: "offered" },
  ];
  const where = data ? [{ label: "~/.claude/agents (yours, every project)", dir: "" }, ...data.projects.map((p) => ({ label: tilde(p.dir), dir: p.dir }))] : [];
  const newHere = (dir?: string) => setForm({ f: { ...EMPTY, dir }, inline: [] });
  const empties = data?.projects.filter((p) => !p.recipes.length) ?? [];

  const status =
    data === null && !error ? (
      <p class="dim list-status">Reading this machine's recipes…</p>
    ) : data && all.length === 0 ? (
      <div class="agents-empty list-status" data-agents-empty>
        <b>No recipes on this machine yet.</b>
        <p>A recipe is a Claude Code subagent: a name, a prompt, and the tools and model it uses.</p>
        <p>
          Make one with <b>New recipe</b>, or <b>Add a project…</b> that already has recipes in its <code>.claude/agents</code> folder.
        </p>
      </div>
    ) : data && groups.every((g) => !g.items.length) ? (
      <p class="dim list-status" data-agents-none>
        {q ? "No recipe's name, description or prompt matches." : "No recipe is offered to your team."}
      </p>
    ) : undefined;

  return (
    <>
      <ListPage<RecipeRow>
        label="Recipes"
        splitKey="agents-recipes"
        header={
          <PageHeader
            title="Recipes"
            state={
              <span class="list-where" data-agents-machine>
                on {machineName()}
              </span>
            }
            actions={
              <>
                <button class="secondary small" data-new-recipe disabled={busy} onClick={() => newHere()}>
                  New recipe
                </button>
                <button class="secondary small" data-add-project disabled={busy} title="List the recipes in a project's .claude/agents" onClick={() => void addProject()}>
                  Add a project…
                </button>
              </>
            }
            stats={all.length > 0 && <ListCounts counts={counts} on={filter} pick={(k) => setFilter(k === filter ? "all" : k)} />}
          />
        }
        searchLabel="Search names, descriptions and prompts"
        query={query}
        setQuery={setQuery}
        groups={groups}
        keyOf={(i) => i.r.path}
        onOpen={(i) => !form && void runHere(i.r, i.project)}
        select={select}
        notice={
          (error || said) &&
          (error ? (
            <p class="control-error agents-error" role="alert" data-agents-error>
              {error}
            </p>
          ) : (
            <p class="agents-said" role="status" data-agents-said>
              {said}
            </p>
          ))
        }
        status={status}
        row={({ r }) => ({
          title: r.name,
          tags: r.offered_from && (
            <Chip kind="tag" tone="info" data-recipe-offered title={`Your team's agents can send it tasks; it works in ${r.offered_from}`}>
              offered
            </Chip>
          ),
          age: r.model,
          detail: r.description,
          attrs: { "data-recipe": r.name, "data-offered": String(!!r.offered_from) },
        })}
        preview={(i) => (
          <RecipePreview
            key={i.r.path}
            item={i}
            busy={busy}
            tilde={tilde}
            run={() => void runHere(i.r, i.project)}
            edit={() => edit(i.r)}
            offer={() => void offer(i.r, i.project)}
            unoffer={() => unoffer(i.r)}
            ask={() => setDeleting(i.r)}
            unlist={() => removeProject(i.project!)}
          />
        )}
        below={
          // The places with nothing to list, whatever the search is: they aren't what it finds.
          data && !q && filter === "all" && (empties.length > 0 || !data.user.recipes.length) ? (
            <ListSection title="No recipes yet" data-agents-empties="">
              {!data.user.recipes.length && (
                <ListItem data-agents-scope="user" title="Yours" detail={`${tilde(data.user.dir)} · every project`} actions={<NewHere busy={busy} go={() => newHere("")} />} />
              )}
              {empties.map((p) => (
                <ListItem
                  key={p.dir}
                  data-agents-scope={p.dir}
                  title={baseName(p.dir)}
                  detail={`${tilde(p.dir)} · No recipes in .claude/agents`}
                  actions={
                    <>
                      <NewHere busy={busy} go={() => newHere(p.dir)} />
                      {p.added && (
                        <button class="secondary small" disabled={busy} title="Stop listing this project here (its files stay)" onClick={() => removeProject(p.dir)}>
                          Remove
                        </button>
                      )}
                    </>
                  }
                />
              ))}
            </ListSection>
          ) : undefined
        }
        pane={form && <RecipeForm key={form.f.path ?? `new:${form.f.dir ?? ""}`} form={form.f} inline={form.inline} where={where} busy={busy} tilde={tilde} save={(f) => void save(f)} cancel={() => setForm(null)} />}
      />
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
    </>
  );
}

function NewHere({ busy, go }: { busy: boolean; go: () => void }) {
  return (
    <button class="secondary small" disabled={busy} onClick={go}>
      New recipe here
    </button>
  );
}

/** A selected recipe: what it is, and the page's one primary button. */
function RecipePreview({ item, busy, tilde, run, edit, offer, unoffer, ask, unlist }: { item: RecipeRow; busy: boolean; tilde: (d: string) => string; run: () => void; edit: () => void; offer: () => void; unoffer: () => void; ask: () => void; unlist: () => void }) {
  const { r, project, added } = item;
  const servers = [...named(r), ...inline(r)];
  const fact = (term: string, value: string) => (
    <>
      <dt>{term}</dt>
      <dd>{value}</dd>
    </>
  );
  return (
    <ListPreview
      data-recipe-preview={r.name}
      title={r.name}
      facts={
        <>
          <span>{r.model ?? "the default model"}</span>
          <span>{project ? tilde(project) : "yours, in every project"}</span>
          {r.offered_from && <span>{r.offered_from === project ? "offered to your team" : `offered, working in ${tilde(r.offered_from)}`}</span>}
        </>
      }
      actions={
        <>
          <button class="primary small" data-run-here={r.name} disabled={busy} title="Open an agent block wearing this recipe, in its project" onClick={run}>
            Run on {machineName()}…
          </button>
          <button class="secondary small" data-edit={r.name} disabled={busy} title="Change its prompt, tools and model" onClick={edit}>
            Edit
          </button>
          {r.offered_from ? (
            <button class="secondary small" data-unoffer={r.name} disabled={busy} title="Take it out of your team's catalog" onClick={unoffer}>
              Stop offering
            </button>
          ) : (
            <button class="secondary small" data-offer={r.name} disabled={busy} title="Put it in your team's catalog, so their agents can send it tasks" onClick={offer}>
              Offer to team
            </button>
          )}
          <button
            class="secondary small"
            aria-label={`More for ${r.name}`}
            title="More"
            data-recipe-more
            disabled={busy}
            onClick={(e) =>
              moreMenu(e, [
                ...(added ? [{ label: `Stop listing ${baseName(project!)}`, hint: "Take this project off the page (its files stay)", run: unlist }] : []),
                { label: "Delete…", danger: true, hint: "Delete its file", run: ask },
              ])
            }
          >
            ⋯
          </button>
          <span class="list-preview-hint" aria-hidden="true">
            <kbd>↵</kbd> run <kbd>↑</kbd>
            <kbd>↓</kbd> move
          </span>
        </>
      }
      aside={
        <dl class="agents-facts" data-recipe-facts>
          {fact("Tools", r.tools.join(", ") || "all")}
          {r.disallowed_tools.length > 0 && fact("Never", r.disallowed_tools.join(", "))}
          {r.permission_mode && fact("Permissions", r.permission_mode)}
          {r.skills.length > 0 && fact("Skills", r.skills.join(", "))}
          {servers.length > 0 && fact("MCP servers", servers.join(", "))}
          {fact("File", tilde(r.path))}
        </dl>
      }
    >
      <p>{r.description}</p>
      {r.prompt && (
        <>
          <h3 class="agents-words-head">Prompt</h3>
          <pre class="agents-recipe-prompt" data-recipe-prompt>
            {r.prompt}
          </pre>
        </>
      )}
    </ListPreview>
  );
}

function RecipeForm({
  form,
  inline,
  where,
  busy,
  tilde,
  save,
  cancel,
}: {
  form: Form;
  inline: string[];
  where: { label: string; dir: string }[];
  busy: boolean;
  tilde: (d: string) => string;
  save: (f: Form) => void;
  cancel: () => void;
}) {
  const [f, setF] = useState<Form>(form);
  const el = useRef<HTMLFormElement>(null);
  // A new one is typed straight away; a phone's keyboard waits for a tap.
  useEffect(() => {
    if (!form.path && matchMedia("(pointer: fine)").matches) el.current?.querySelector<HTMLInputElement>("input[name=name]")?.focus();
  }, []);
  const set = (k: keyof Form) => (e: Event) => setF({ ...f, [k]: (e.currentTarget as HTMLInputElement).value });
  const field = (k: keyof Form, label: string, hint = "") => (
    <label class="agents-field">
      <span>{label}</span>
      <input name={k} value={f[k] as string} placeholder={hint} onInput={set(k)} autocomplete="off" spellcheck={false} />
    </label>
  );
  return (
    <form
      ref={el}
      class="agents-form"
      data-recipe-form={f.path ? "edit" : "new"}
      onSubmit={(e) => {
        e.preventDefault();
        save(f);
      }}
    >
      <ListPreview
        title={f.path ? `Edit ${form.name}` : "New recipe"}
        facts={f.path && <span>{tilde(f.path)}</span>}
        actions={
          <>
            <button class="primary small" type="submit" data-save-recipe disabled={busy || !f.name.trim() || !f.description.trim()}>
              Save
            </button>
            <button class="secondary small" type="button" data-cancel-recipe onClick={cancel}>
              Cancel
            </button>
          </>
        }
      >
        <p>Saved as a Markdown file Claude Code reads. Name and description are required; leave the rest empty for Claude Code's defaults.</p>
        <div class="agents-fields">
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
        </div>
      </ListPreview>
    </form>
  );
}

/** An agent on the team's shelves, and the machine it's on. */
interface AgentRow {
  s: Shelf;
  a: Shelf["agents"][number];
}

/** Whether Run goes: one of the account's own, on a machine that's up. */
const runnable = (s: Shelf) => !s.owner && s.online;

function TeamTab({ client }: { client: Client }) {
  const [shelves, setShelves] = useState<Shelf[] | null>(null);
  const [waiting, setWaiting] = useState<Waiting[]>([]);
  const [grants, setGrants] = useState<Grant[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [filter, setFilter] = useState("all");
  const [query, setQuery] = useState("");
  const load = async (fresh = false) => {
    setBusy(true);
    try {
      const c = await json<{ machines: Shelf[] }>(client, "GET", `/api/a2a/catalog${fresh ? "?fresh=1" : ""}`);
      // An agent here is the card its machine answered with, as it sent it.
      const named = (a: Shelf["agents"][number]) => typeof a.name === "string";
      const said = (a: Shelf["agents"][number]) => ({ ...a, description: typeof a.description === "string" ? a.description : "" });
      setShelves(c.machines.map((m) => ({ ...m, agents: m.agents.filter(named).map(said) })));
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

  // This machine, then your own, then your teammates', each by name
  // whatever its case (UI audit #173: WIN-… sorted before arugula-…).
  const rank = (s: Shelf) => (s.here ? 0 : s.owner ? 2 : 1);
  const sorted = [...(shelves ?? [])].sort((a, b) => rank(a) - rank(b) || a.name.localeCompare(b.name, undefined, { sensitivity: "base" }));
  const offered = sorted.filter((s) => s.agents.length);
  const quiet = sorted.filter((s) => !s.agents.length);
  const q = query.trim().toLowerCase();
  const fits = ({ s, a }: AgentRow) =>
    (filter === "all" || (filter === "online" && s.online) || (filter === "yours" && !s.owner)) &&
    (!q || a.name.toLowerCase().includes(q) || a.description.toLowerCase().includes(q) || s.name.toLowerCase().includes(q) || s.owner.toLowerCase().includes(q));
  const groups: ListGroup<AgentRow>[] = offered.map((s) => ({
    title: s.name,
    named: true,
    key: s.machine,
    detail: <ShelfState s={s} />,
    items: s.agents.map((a) => ({ s, a })).filter(fits),
    attrs: { "data-shelf": s.name, "data-online": String(s.online) },
  }));
  const rows = offered.flatMap((s) => s.agents.map((a) => ({ s, a })));
  const someTeammate = rows.some((i) => i.s.owner);
  const counts: ListCount[] = [
    { key: "all", n: rows.length, label: rows.length === 1 ? "agent" : "agents" },
    { key: "online", n: rows.filter((i) => i.s.online).length, label: "online", tone: "live" },
    ...(someTeammate ? [{ key: "yours", n: rows.filter((i) => !i.s.owner).length, label: "yours" }] : []),
    { n: sorted.length, label: sorted.length === 1 ? "machine" : "machines" },
  ];

  const status =
    shelves === null && !error ? (
      <p class="dim list-status">Asking every machine for its agents…</p>
    ) : shelves && !offered.length ? (
      <div class="agents-empty list-status" data-agents-empty>
        <b>Nobody on your team offers an agent yet.</b>
        <p>
          Offer one of yours: open{" "}
          <button class="agents-inline" onClick={() => openAgentsPage("recipes")}>
            Recipes
          </button>{" "}
          and choose <b>Offer to team</b> on a recipe.
        </p>
      </div>
    ) : shelves && groups.every((g) => !g.items.length) ? (
      <p class="dim list-status" data-agents-none>
        {q ? "No agent, machine or owner matches." : filter === "online" ? "No machine with an agent is online." : "None of the agents are yours."}
      </p>
    ) : undefined;

  return (
    <ListPage<AgentRow>
      label="Team agents"
      splitKey="agents-team"
      header={
        <PageHeader
          title="Team"
          actions={
            <button class="secondary small" data-team-refresh disabled={busy} title="Ask every machine again" onClick={() => void load(true)}>
              {busy ? "Refreshing…" : "Refresh"}
            </button>
          }
          stats={rows.length > 0 && <ListCounts counts={counts} on={filter} pick={(k) => setFilter(k === filter ? "all" : k)} />}
        />
      }
      searchLabel="Search agents, machines and owners"
      query={query}
      setQuery={setQuery}
      groups={groups}
      keyOf={({ s, a }) => `${s.machine}/${a.name}`}
      onOpen={({ s, a }) => runnable(s) && void run(s, a.name)}
      notice={
        error && (
          <p class="control-error agents-error" role="alert" data-agents-error>
            {error}
          </p>
        )
      }
      status={status}
      row={({ a }) => ({ title: a.name, detail: a.description, attrs: { "data-agent": a.name } })}
      preview={(i) => <AgentPreview key={`${i.s.machine}/${i.a.name}`} item={i} busy={busy} run={() => void run(i.s, i.a.name)} />}
      above={
        waiting.length > 0 && (
          <ListSection title="Waiting for you" n={waiting.length} data-agents-waiting="">
            {waiting.map((t) => (
              <ListItem
                key={t.id}
                tone="warn"
                data-waiting={t.id}
                title={
                  <>
                    {t.metadata?.arugula?.caller ?? "someone"} <span class="dim">asks {t.metadata?.arugula?.agent}</span>
                  </>
                }
                detail={t.history?.[0]?.parts?.[0]?.text}
                actions={
                  <>
                    <button class="secondary small" data-allow-once disabled={busy} onClick={() => void answer(t.id, "once")}>
                      Allow once
                    </button>
                    <button class="secondary small" data-allow-hour disabled={busy} title="Allow this person's tasks for this agent for an hour" onClick={() => void answer(t.id, "hour")}>
                      Allow for an hour
                    </button>
                    <button class="secondary danger small" data-deny disabled={busy} onClick={() => void answer(t.id, "deny")}>
                      Deny
                    </button>
                  </>
                }
              />
            ))}
          </ListSection>
        )
      }
      below={
        !q &&
        filter === "all" && (
          <>
            {grants.length > 0 && (
              <ListSection title="Standing grants" n={grants.length} data-agents-grants="">
                {grants.map((g) => (
                  <ListItem
                    key={`${g.account}/${g.agent}`}
                    data-grant={`${g.name}/${g.agent}`}
                    title={g.name}
                    detail={
                      <>
                        may use {g.agent} · ends <Time at={g.until_ms} title="Until" />
                      </>
                    }
                    actions={
                      <button class="secondary small" data-revoke disabled={busy} onClick={() => void client.request("POST", "/api/a2a/grants", { account: g.account, agent: g.agent }).then(() => load())}>
                        Revoke
                      </button>
                    }
                  />
                ))}
              </ListSection>
            )}
            {/* Every other machine of the team's, and why it lists none: so a
                missing machine isn't a mystery. */}
            {quiet.length > 0 && (
              <ListSection title="No agents listed" n={quiet.length} data-agents-quiet="">
                {quiet.map((m) => (
                  <ListItem
                    key={m.machine}
                    data-quiet={m.name}
                    title={m.name}
                    detail={<span title={m.note ?? undefined}>{[m.here ? "this machine" : "", m.owner ? `${m.owner}'s` : "", m.online ? "offers no agents" : m.note?.startsWith("offline") ? "offline" : "didn't answer"].filter(Boolean).join(" · ")}</span>}
                  />
                ))}
              </ListSection>
            )}
          </>
        )
      }
    />
  );
}

/** Beside a machine's name: whose it is, and whether it's up. */
function ShelfState({ s }: { s: Shelf }) {
  const parts = [s.here ? "this machine" : "", s.owner ? `${s.owner}'s` : ""].filter(Boolean).join(" · ");
  return (
    <>
      {parts}
      {!s.online && (
        <>
          {parts ? " · " : ""}offline{s.fetched_ms ? <>, as seen <Time at={s.fetched_ms} /></> : ""}
        </>
      )}
    </>
  );
}

/** A selected agent: where it runs, and how an agent of yours reaches it. */
function AgentPreview({ item, busy, run }: { item: AgentRow; busy: boolean; run: () => void }) {
  const { s, a } = item;
  return (
    <ListPreview
      data-agent-preview={a.name}
      title={a.name}
      facts={
        <>
          <span>on {s.name}</span>
          {s.owner && <span>{s.owner}'s</span>}
          {!s.online && (
            <span>
              offline{s.fetched_ms ? <>, as seen <Time at={s.fetched_ms} /></> : ""}
            </span>
          )}
        </>
      }
      actions={
        runnable(s) && (
          <>
            <button class="primary small" data-run={a.name} disabled={busy} title={`Run it in its project on ${s.name}, and go to it`} onClick={run}>
              Run on {s.name}…
            </button>
            <span class="list-preview-hint" aria-hidden="true">
              <kbd>↵</kbd> run <kbd>↑</kbd>
              <kbd>↓</kbd> move
            </span>
          </>
        )
      }
    >
      <p>{a.description}</p>
      {s.note && s.online && <p class="dim">{s.note}</p>}
      <h3 class="agents-words-head">Send it a task</h3>
      <p>
        Ask one of your agents to send it a task with this{s.owner ? `; ${s.owner} approves each one` : ""}. With <code>pr: true</code>, it opens a pull request in its own project.
      </p>
      <CopyText text={`delegate {agent: ${a.name}${s.here ? "" : `, machine: ${s.name}`}}`} data-delegate={a.name} />
    </ListPreview>
  );
}
