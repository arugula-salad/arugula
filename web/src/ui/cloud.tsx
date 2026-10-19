import { AgentMessage } from "../blocks/agent-message";
import { useEffect, useLayoutEffect, useRef, useState } from "preact/hooks";

import { cloudApi as api, cloudPanes, cloudTerminal as terminal, useCloudPanes, type CloudAgent as Agent, type CloudRun as Run } from "../cloud";
type Block = { kind: string; body?: unknown; summary?: string; request_id?: string; options?: { optionId: string; name?: string }[]; schema?: { properties?: Record<string, { type?: string; title?: string; enum?: string[] }>; required?: string[] } };
type Event = { id: number; stage?: string; state?: string; data?: string; blocks?: Block[] };

function eventFailure(event: Event): string {
  let data: { reason?: unknown; error?: unknown } = {};
  try { data = JSON.parse(event.data ?? "{}"); } catch { /* Fall back to the stage below. */ }
  if (data && typeof data.reason === "string") return data.reason;
  if (data && typeof data.error === "string") return data.error;
  return `Cloud execution failed during ${event.stage ?? "the run"}.`;
}

export function CloudPanel({ close, initialRun, notify, launchOnly = false, repeat }: { close: () => void; initialRun?: string; notify: () => Promise<string>; launchOnly?: boolean; repeat?: Run }) {
  const [agents, setAgents] = useState<Agent[]>([]);
  const [runs, setRuns] = useState<Run[]>([]);
  const [names, setNames] = useState<string[]>([]);
  const [agent, setAgent] = useState(repeat?.spec.agent_id ?? "curated_coding");
  const [repository, setRepository] = useState(repeat?.spec.repository ?? "");
  const [gitRef, setGitRef] = useState(repeat?.spec.git_ref ?? "");
  const [repositorySecret, setRepositorySecret] = useState("");
  const [prompt, setPrompt] = useState(repeat?.spec.prompt ?? "");
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const [selected, setSelected] = useState<string | null>(initialRun ?? null);
  const [editor, setEditor] = useState<Agent | "new" | null>(null);
  const [limits, setLimits] = useState("");
  const [unavailable, setUnavailable] = useState("");
  const requestKey = useRef(crypto.randomUUID());
  useEffect(() => { requestKey.current = crypto.randomUUID(); }, [agent, repository, gitRef, repositorySecret, prompt]);
  const refresh = async () => {
    const [catalog, history, secrets] = await Promise.allSettled([
      api<{ agents: Agent[]; limits: { daily_runs: number; concurrent_runs: number; duration_ms: number } }>("agents"),
      api<{ runs: Run[] }>("runs"), api<{ names: string[] }>("secrets"),
    ]);
    if (history.status === "fulfilled") setRuns(history.value.runs);
    if (secrets.status === "fulfilled") setNames(secrets.value.names);
    if (catalog.status === "fulfilled") {
      setAgents(catalog.value.agents); setUnavailable("");
      const limits = catalog.value.limits;
      setLimits(`${limits.daily_runs} runs per day · ${limits.concurrent_runs} at once · ${limits.duration_ms / 60000} minutes per run`);
    } else {
      setAgents([]); setLimits(""); setUnavailable(`New cloud runs are unavailable: ${catalog.reason.message}`);
    }
    if (history.status === "rejected") throw history.reason;
    if (secrets.status === "rejected") throw secrets.reason;
  };
  useEffect(() => {
    let active = true;
    const poll = async () => { try { if (active) await refresh(); } catch (e) { if (active) setError((e as Error).message); } };
    void poll(); const timer = setInterval(() => void poll(), 5000);
    return () => { active = false; clearInterval(timer); };
  }, []);
  return <div class={`cloud-panel${launchOnly ? " cloud-launch-form" : ""}`}>
    {!launchOnly && <div class="prompt-buttons"><h2>Cloud agents</h2><button onClick={close}>Close</button></div>}
    <p>Run an agent in a fresh checkout. Work continues when you close this window. Saved results remain after the checkout is removed.</p>
    {!launchOnly && <button onClick={() => void notify().then((state) => { if (state !== "on") setError(state === "denied" ? "Notifications are blocked in browser settings." : "Notifications are unavailable in this browser."); }, (e: Error) => setError(e.message))}>Notify me when an agent needs input</button>}
    <p class="dim">{unavailable || `Limited beta${limits ? ` · ${limits}` : ""}`}</p>
    <p class="dim">Cloud execution processes your prompts, repository, selected secrets, and results. Terminal connections remain end-to-end encrypted.</p>
    {error && <p role="alert" class="control-error">{error}</p>}
    {selected ? <RunDetails key={selected} id={selected} back={() => setSelected(null)} /> : <>
      <form class="cloud-form" onSubmit={async (e) => {
        e.preventDefault(); setBusy(true); setError("");
        try {
          const run = await api<Run>("runs", "POST", { request_key: requestKey.current, agent_id: agent, repository, git_ref: gitRef, repository_secret: repositorySecret || null, prompt });
          requestKey.current = crypto.randomUUID(); cloudPanes.open(run.id); close();
        } catch (e) { setError((e as Error).message); } finally { setBusy(false); }
      }}>
        <label>Agent<select value={agent} onChange={(e) => setAgent(e.currentTarget.value)}>{agents.map((a) => <option key={a.id} value={a.id}>{a.definition.name}{a.curated ? " · Arugula" : ""}</option>)}</select></label>
        <label>Repository URL<input type="url" required placeholder="https://github.com/owner/repository" value={repository} onInput={(e) => setRepository(e.currentTarget.value)} /></label>
        <label>Branch or tag (optional)<input value={gitRef} onInput={(e) => setGitRef(e.currentTarget.value)} /></label>
        <label>Repository credential<select value={repositorySecret} onChange={(e) => setRepositorySecret(e.currentTarget.value)}><option value="">Public repository</option>{names.map((n) => <option key={n}>{n}</option>)}</select></label>
        <label>Task<textarea required rows={4} value={prompt} onInput={(e) => setPrompt(e.currentTarget.value)} /></label>
        <button class="primary" disabled={busy || !agents.length}>{busy ? "Starting…" : "Run in cloud"}</button>
      </form>
      {!launchOnly && <><h3>Runs</h3>
      {!runs.length && <p class="dim">Your runs will appear here.</p>}
      <ul class="cloud-runs">{runs.map((r) => <li key={r.id}><button onClick={() => { cloudPanes.open(r.id); close(); }}>{r.spec.agent.name}: {r.spec.prompt.slice(0, 90)}</button><span>{r.state.replaceAll("_", " ")}</span></li>)}</ul>
      <details><summary>Manage agents</summary><button onClick={() => setEditor("new")}>Create agent</button>
        {agents.map((a) => <p key={a.id}>{a.definition.name} <button onClick={() => setEditor(a)}>{a.curated ? "Customize a copy" : "Edit"}</button></p>)}
        {editor && <AgentEditor key={typeof editor === "string" ? editor : editor.id} agent={editor === "new" ? undefined : editor} names={names} done={() => { setEditor(null); void refresh(); }} />}
      </details>
      <details><summary>Import an existing Managoat agent</summary><ImportAgent refresh={refresh} /></details>
      <details><summary>Secrets</summary><SecretEditor names={names} refresh={refresh} /></details></>}
    </>}
  </div>;
}

function AgentEditor({ agent, names, done }: { agent?: Agent; names: string[]; done: () => void }) {
  const [name, setName] = useState(agent?.definition.name ?? "");
  const [instructions, setInstructions] = useState(agent?.definition.instructions ?? "");
  const [tools, setTools] = useState(JSON.stringify(agent?.definition.mcp_servers ?? {}, null, 2));
  const [skills, setSkills] = useState(JSON.stringify(agent?.definition.skills ?? [], null, 2));
  const [secrets, setSecrets] = useState(agent?.definition.secret_names ?? []);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  return <form class="cloud-form" onSubmit={async (e) => {
    e.preventDefault(); setBusy(true); setError("");
    try {
      const definition = { name, instructions, mcp_servers: JSON.parse(tools), skills: JSON.parse(skills), secret_names: secrets };
      if (agent && !agent.curated) await api(`agents/${agent.id}`, "PUT", { version: agent.version, definition });
      else await api("agents", "POST", definition);
      done();
    } catch (e) { setError((e as Error).message); } finally { setBusy(false); }
  }}>
    <label>Name<input required value={name} onInput={(e) => setName(e.currentTarget.value)} /></label>
    <label>Instructions<textarea required rows={5} value={instructions} onInput={(e) => setInstructions(e.currentTarget.value)} /></label>
    <details><summary>Tools and skills</summary><label>MCP servers (JSON)<textarea rows={5} value={tools} onInput={(e) => setTools(e.currentTarget.value)} /></label><label>Skills (JSON array of name and content)<textarea rows={5} value={skills} onInput={(e) => setSkills(e.currentTarget.value)} /></label></details>
    <fieldset><legend>Secrets available to this agent</legend>{names.map((n) => <label key={n}><input type="checkbox" checked={secrets.includes(n)} onChange={(e) => setSecrets(e.currentTarget.checked ? [...secrets, n] : secrets.filter((v) => v !== n))} />{n}</label>)}</fieldset>
    {error && <p role="alert">{error}</p>}<button disabled={busy}>Save agent</button><button type="button" onClick={done}>Cancel</button>
    {agent && !agent.curated && <button type="button" disabled={busy} onClick={async () => { setBusy(true); try { await api(`agents/${agent.id}`, "DELETE"); done(); } catch (e) { setError((e as Error).message); setBusy(false); } }}>Delete agent</button>}
  </form>;
}

function ImportAgent({ refresh }: { refresh: () => Promise<void> }) {
  const [id, setId] = useState(""); const [key, setKey] = useState("");
  const [busy, setBusy] = useState(false); const [message, setMessage] = useState("");
  return <form class="cloud-form" onSubmit={async (event) => {
    event.preventDefault(); setBusy(true); setMessage("");
    try {
      const result = await api<{ agent: Agent; warnings: string[] }>("import", "POST", { agent_id: id, api_key: key });
      setKey(""); setMessage(`Imported ${result.agent.definition.name}. ${result.warnings.join(" ")}`); await refresh();
    } catch (error) { setMessage((error as Error).message); } finally { setBusy(false); }
  }}>
    <p>Your old credential is used once to read the agent definition and is not saved. Cloud runs use Arugula's account. Reconnect tools and secrets after importing; old conversations stay on the old account.</p>
    <label>Old agent ID<input required value={id} onInput={(e) => setId(e.currentTarget.value)} /></label>
    <label>Old API credential<input required type="password" autoComplete="off" value={key} onInput={(e) => setKey(e.currentTarget.value)} /></label>
    <button disabled={busy}>Import definition</button>{message && <p role="status">{message}</p>}
  </form>;
}

function SecretEditor({ names, refresh }: { names: string[]; refresh: () => Promise<void> }) {
  const [name, setName] = useState(""); const [value, setValue] = useState(""); const [error, setError] = useState(""); const [busy, setBusy] = useState(false);
  return <><p>Values are write-only. Updates apply to new runs; existing runs keep their selected credentials.</p><form class="cloud-form" onSubmit={async (e) => {
    e.preventDefault(); setBusy(true); setError("");
    try { await api(`secrets/${encodeURIComponent(name)}`, "PUT", { value }); setValue(""); setName(""); await refresh(); } catch (e) { setError((e as Error).message); } finally { setBusy(false); }
  }}><label>Name<input required placeholder="GITHUB_TOKEN" value={name} onInput={(e) => setName(e.currentTarget.value)} /></label><label>Value<input type="password" autoComplete="new-password" required value={value} onInput={(e) => setValue(e.currentTarget.value)} /></label><button disabled={busy}>Save secret</button></form>
    {error && <p role="alert">{error}</p>}{names.map((n) => <p key={n}>{n} <button onClick={async () => { try { await api(`secrets/${encodeURIComponent(n)}`, "DELETE"); await refresh(); } catch (e) { setError((e as Error).message); } }}>Remove</button></p>)}</>;
}

/** A cloud conversation's record, transcript and pending requests, polled
 * from control: a run's (`runs/<id>`) or a Cloud workspace tab's
 * (`workspaces/<id>/tabs/<tab>`). Polling stops once `ended` says so and
 * the transcript is read to its end. */
export function useCloudConversation<T>(path: string, ended: (item: T) => boolean) {
  const [item, setItem] = useState<T | null>(null); const [events, setEvents] = useState<Event[]>([]); const [error, setError] = useState(""); const [answered, setAnswered] = useState<string[]>([]); const [pending, setPending] = useState<string[]>([]);
  const cursor = useRef(0);
  useEffect(() => {
    let active = true, timer: ReturnType<typeof setTimeout>;
    const poll = async () => {
      try {
        const [r, page, requests] = await Promise.all([api<T>(path), api<{ events: Event[] }>(`${path}/events?after=${cursor.current}`), api<{ requests: Block[] }>(`${path}/requests`)]);
        if (!active) return;
        setItem(r); setPending(requests.requests.flatMap((r) => r.request_id ? [r.request_id] : []));
        if (page.events.length) { cursor.current = Math.max(...page.events.map((e) => e.id)); setEvents((old) => [...old, ...page.events]); }
        if (!ended(r) || page.events.length === 1000) timer = setTimeout(() => void poll(), page.events.length === 1000 ? 0 : 2000);
      } catch (e) { if (active) { setError((e as Error).message); timer = setTimeout(() => void poll(), 5000); } }
    };
    void poll(); return () => { active = false; clearTimeout(timer); };
  }, [path]);
  const answer = async (request: string, body: unknown) => { await api(`${path}/requests/${encodeURIComponent(request)}`, "POST", body); setAnswered((old) => [...old, request]); };
  return { item, events, error, setError, pending, answered, answer };
}

/** The transcript and its approvals, shared by a run and a Cloud workspace
 * tab: the last failure, then each block, with permission and question
 * requests answerable while they're pending and `ended` is false. */
export function CloudTranscript({ lead, events, pending, answered, ended, answer, setError }: {
  lead?: string; events: Event[]; pending: string[]; answered: string[]; ended: boolean;
  answer: (request: string, body: unknown) => Promise<void>; setError: (error: string) => void;
}) {
  const scroller = useRef<HTMLDivElement>(null);
  const stick = useRef(true);
  useLayoutEffect(() => { if (scroller.current && stick.current) scroller.current.scrollTop = scroller.current.scrollHeight; });
  const open = (request: string) => !ended && pending.includes(request) && !answered.includes(request);
  return <>
    {events.filter((event) => event.state === "failed").slice(-1).map((event) => <p key={event.id} role="alert" class="control-error">{eventFailure(event)}</p>)}
    <div class="agent-log cloud-transcript" ref={scroller} onScroll={(e) => { const el = e.currentTarget; stick.current = el.scrollHeight - el.scrollTop - el.clientHeight < 40; }}>{lead !== undefined && <div class="agent-user">{lead}</div>}{events.flatMap((event) => (event.blocks ?? []).map((block, index) => <div key={`${event.id}-${index}`}>
      {block.kind === "permission_request" && block.request_id ? <div class="agent-perm" role="alertdialog" aria-label={block.summary ?? "Agent approval"}><pre class="agent-perm-cmd">{block.summary}</pre>{block.options?.map((option) => <button key={option.optionId} disabled={!open(block.request_id!)} onClick={() => void answer(block.request_id!, { option_id: option.optionId }).catch((e: Error) => setError(e.message))}>{option.name ?? option.optionId}</button>)}</div>
        : block.kind === "elicitation_request" && block.request_id ? <Question block={block} disabled={!open(block.request_id)} answer={(body) => answer(block.request_id!, body)} />
        : block.kind === "prompt" && typeof block.body === "string" ? <div class="agent-user">{block.body}</div>
        : <AgentMessage text={typeof block.body === "string" ? block.body : block.body ? JSON.stringify(block.body, null, 2) : block.summary ?? ""} thought={block.kind === "thinking" || block.kind === "thought"} />}
    </div>))}</div>
  </>;
}

export function RunDetails({ id, back }: { id: string; back?: () => void }) {
  const { item: run, events, error, setError, pending, answered, answer } = useCloudConversation<Run>(`runs/${id}`, terminal);
  const [repeat, setRepeat] = useState(false);
  return <div class="agent cloud-agent" data-cloud-pane={id}>{back && <button onClick={back}>Back to runs</button>}{error && <p role="alert">{error}</p>}{run && <>
    <div class="agent-bar"><span class="agent-status">{run.state === "needs_input" ? "Needs you" : run.state.replaceAll("_", " ")}</span><span class="agent-name">{run.spec.agent.name}</span><span class="host-tag">Cloud</span><span class="agent-spacer" />{terminal(run) && <button onClick={() => setRepeat(true)}>Run again</button>}</div><div class="agent-note">{run.spec.repository} · {run.spec.git_ref || "default branch"} · Fresh checkout</div>
    {!terminal(run) && <button onClick={() => void api(`runs/${id}/cancel`, "POST", {}).catch((e: Error) => setError(e.message))}>Cancel run</button>}
    <CloudTranscript lead={run.spec.prompt} events={events} pending={pending} answered={answered} ended={terminal(run)} answer={answer} setError={setError} />
    {run.result && <><h3>Saved results</h3><button onClick={() => { const url = URL.createObjectURL(new Blob([JSON.stringify({ run, events }, null, 2)], { type: "application/json" })); const a = document.createElement("a"); a.href = url; a.download = `${id}.json`; a.click(); setTimeout(() => URL.revokeObjectURL(url), 1000); }}>Download results and transcript</button><SavedResults result={run.result} /></>}
  </>}{repeat && run && <div class="prompt-backdrop" role="dialog" aria-label="Run again"><div class="prompt cloud-launch"><h2>Run again</h2><p>This starts a fresh checkout. Saved changes from this run are not applied automatically.</p><CloudPanel launchOnly repeat={run} close={() => setRepeat(false)} notify={async () => "off"} /><button onClick={() => setRepeat(false)}>Cancel</button></div></div>}</div>;
}

function SavedResults({ result }: { result: Record<string, unknown> }) {
  const diff = result.diff as { diff?: string } | undefined;
  const files = result.files as Record<string, { content: string; encoding: string }> | undefined;
  return <>
    {typeof result.message === "string" && <p>{result.message}</p>}
    {diff && <details open><summary>Changes</summary><pre>{diff.diff || "No tracked-file changes."}</pre></details>}
    {Object.entries(files ?? {}).map(([name, file]) => <details key={name}><summary>{name}</summary>{file.encoding === "utf-8" ? <pre>{file.content}</pre> : <p>Binary file included in the download.</p>}</details>)}
  </>;
}

function Question({ block, disabled, answer }: { block: Block; disabled: boolean; answer: (body: unknown) => Promise<void> }) {
  const [values, setValues] = useState<Record<string, unknown>>({}); const [error, setError] = useState(""); const [busy, setBusy] = useState(false);
  return <form class="cloud-form" onSubmit={async (e) => { e.preventDefault(); setBusy(true); try { await answer({ action: "accept", content: values }); } catch (e) { setError((e as Error).message); } finally { setBusy(false); } }}>
    <p>{String(block.body ?? "The agent needs your input")}</p>
    {Object.entries(block.schema?.properties ?? {}).map(([name, field]) => {
      const required = block.schema?.required?.includes(name);
      const set = (value: unknown) => setValues({ ...values, [name]: value });
      return <label key={name}>{field.title ?? name}{field.enum
        ? <select required={required} value={String(values[name] ?? "")} onChange={(e) => set(e.currentTarget.value)}><option value="">Choose…</option>{field.enum.map((v) => <option key={v}>{v}</option>)}</select>
        : field.type === "boolean"
          ? <input type="checkbox" checked={values[name] === true} onChange={(e) => set(e.currentTarget.checked)} />
          : field.type === "number" || field.type === "integer"
            ? <input type="number" required={required} step={field.type === "integer" ? "1" : "any"} onInput={(e) => set(e.currentTarget.valueAsNumber)} />
            : <input type="text" required={required} onInput={(e) => set(e.currentTarget.value)} />}</label>;
    })}
    {error && <p role="alert">{error}</p>}<button disabled={disabled || busy}>Answer</button><button type="button" disabled={disabled || busy} onClick={() => void answer({ action: "decline" }).catch((e: Error) => setError(e.message))}>Decline</button>
  </form>;
}

export function CloudTabs() {
  const selected = useCloudPanes();
  return <>{cloudPanes.opened.map((id) => { const run = cloudPanes.runs.find((r) => r.id === id); return <div class={`tab${selected === id ? " selected" : ""}`} role="tab" aria-selected={selected === id} key={id} onClick={() => cloudPanes.open(id)}><span class="host-tag">Cloud</span><span class="tab-label">{run?.spec.prompt.slice(0, 45) || "Cloud agent"}</span>{run?.state === "needs_input" && <span class="attention" title="Needs you">●</span>}<button class="tab-close" title="Close cloud pane" aria-label="Close cloud pane" onClick={(e) => { e.stopPropagation(); cloudPanes.close(id); }}>×</button></div>; })}</>;
}
