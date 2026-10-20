import { AgentMessage } from "../blocks/agent-message";
import { useEffect, useLayoutEffect, useRef, useState } from "preact/hooks";

import { cloudApi as api, cloudPanes, cloudTerminal as terminal, useCloudApi, useCloudPanes, type CloudAgent as Agent, type CloudRun as Run, type PastCloudTab } from "../cloud";
type Block = { kind: string; body?: unknown; summary?: string; name?: string; request_id?: string; options?: { optionId: string; name?: string }[]; schema?: { properties?: Record<string, { type?: string; title?: string; enum?: string[] }>; required?: string[] } };
export type Event = { id: number; kind?: string; stage?: string; state?: string; data?: string; blocks?: Block[] };

const parsed = (data: string | undefined): Record<string, unknown> => {
  try { const v = JSON.parse(data ?? "{}"); return v && typeof v === "object" ? v : {}; } catch { return {}; }
};

/** Nothing to show: an empty string, or an empty object or list (a tool
 * call's input before the agent has filled it in is `{}`). */
export function emptyBody(body: unknown): boolean {
  if (body == null) return true;
  if (typeof body === "string") { const t = body.trim(); return !t || t === "{}" || t === "[]"; }
  if (Array.isArray(body)) return body.length === 0;
  return typeof body === "object" && Object.keys(body).length === 0;
}

/** The transcript's blocks in order, with each run of text (or thinking)
 * chunks joined into one: Managoat sends a message as many chunks, one
 * block each, that join with nothing between them. Anything else (a tool
 * call, an approval, the next prompt) ends the run. `init` and `raw`
 * blocks are bookkeeping, not output. */
export function transcriptBlocks(events: Event[]): { key: string; block: Block }[] {
  const out: { key: string; block: Block }[] = [];
  for (const event of events) {
    (event.blocks ?? []).forEach((block, index) => {
      if (block.kind === "init" || block.kind === "raw") return;
      const last = out.at(-1);
      const joins = (k: string) => k === "text" || k === "thinking" || k === "thought";
      if (joins(block.kind) && typeof block.body === "string" && last && last.block.kind === block.kind && typeof last.block.body === "string") {
        last.block = { ...last.block, body: last.block.body + block.body };
        return;
      }
      out.push({ key: `${event.id}-${index}`, block });
    });
  }
  return out;
}

/** How each request the transcript shows was settled, from its `request`
 * `done` stage: whether it was answered and with which option. */
function settledRequests(events: Event[]): Map<string, { outcome?: string; option?: string }> {
  const settled = new Map<string, { outcome?: string; option?: string }>();
  for (const e of events) {
    if (e.stage !== "request" || e.state !== "done") continue;
    const d = parsed(e.data);
    const id = d.request_id ?? d.requestId;
    if (typeof id === "string") settled.set(id, { outcome: typeof d.outcome === "string" ? d.outcome : undefined, option: typeof d.option_id === "string" ? d.option_id : undefined });
  }
  return settled;
}

function eventFailure(event: Event): string {
  const data = parsed(event.data);
  if (typeof data.reason === "string") return data.reason;
  if (data && typeof data.error === "string") return data.error;
  return `Cloud execution failed during ${event.stage ?? "the run"}.`;
}

/** The account's cloud agents: the ones it may open in a Cloud workspace's
 * tabs, its secrets, and its past runs (runs are retired, S40 M119: they
 * open read only, with their downloads). */
export function CloudPanel({ close, notify }: { close: () => void; notify: () => Promise<string> }) {
  const [agents, setAgents] = useState<Agent[]>([]);
  const [runs, setRuns] = useState<Run[]>([]);
  const [names, setNames] = useState<string[]>([]);
  const [error, setError] = useState("");
  const [editor, setEditor] = useState<Agent | "new" | null>(null);
  const [unavailable, setUnavailable] = useState("");
  const refresh = async () => {
    const [catalog, history, secrets] = await Promise.allSettled([api<{ agents: Agent[] }>("agents"), api<{ runs: Run[] }>("runs"), api<{ names: string[] }>("secrets")]);
    if (history.status === "fulfilled") setRuns(history.value.runs);
    if (secrets.status === "fulfilled") setNames(secrets.value.names);
    if (catalog.status === "fulfilled") { setAgents(catalog.value.agents); setUnavailable(""); }
    else { setAgents([]); setUnavailable(`Cloud agents are unavailable: ${catalog.reason.message}`); }
    if (history.status === "rejected") throw history.reason;
    if (secrets.status === "rejected") throw secrets.reason;
  };
  useEffect(() => {
    let active = true;
    const poll = async () => { try { if (active) await refresh(); } catch (e) { if (active) setError((e as Error).message); } };
    void poll(); const timer = setInterval(() => void poll(), 5000);
    return () => { active = false; clearInterval(timer); };
  }, []);
  return <div class="cloud-panel">
    <div class="prompt-buttons"><h2>Cloud agents</h2><button onClick={close}>Close</button></div>
    <p>Agents run in Cloud workspaces: a machine in the cloud with your repositories on it. <b>New agent</b> → <b>Cloud</b> opens one there, and so does <b>+ Agent</b> in a Cloud workspace.</p>
    {!unavailable && <p><button data-open-cloud-workspaces onClick={() => { close(); location.hash = "cloud-ws"; }}>Cloud workspaces</button></p>}
    <button onClick={() => void notify().then((state) => { if (state !== "on") setError(state === "denied" ? "Notifications are blocked in browser settings." : "Notifications are unavailable in this browser."); }, (e: Error) => setError(e.message))}>Notify me when an agent needs input</button>
    <p class="dim">{unavailable || "Limited beta"}</p>
    <p class="dim">Cloud execution processes your prompts, repository, selected secrets, and results. Terminal connections remain end-to-end encrypted.</p>
    {error && <p role="alert" class="control-error">{error}</p>}
    <details><summary>Manage agents</summary><button onClick={() => setEditor("new")}>Create agent</button>
      {agents.map((a) => <p key={a.id}>{a.definition.name} <button onClick={() => setEditor(a)}>{a.curated ? "Customize a copy" : "Edit"}</button></p>)}
        {editor && <AgentEditor key={typeof editor === "string" ? editor : editor.id} agent={editor === "new" ? undefined : editor} names={names} done={() => { setEditor(null); void refresh(); }} />}
    </details>
    <details><summary>Import an existing Managoat agent</summary><ImportAgent refresh={refresh} /></details>
    <details><summary>Secrets</summary><SecretEditor names={names} refresh={refresh} /></details>
    {runs.length > 0 && <><h3>Past runs</h3>
      <p class="dim">One-shot runs are retired. What they did stays here to read and download.</p>
      <ul class="cloud-runs" data-past-runs>{runs.map((r) => <li key={r.id}><button onClick={() => { cloudPanes.open(r.id); close(); }}>{r.spec.agent.name}: {r.spec.prompt.slice(0, 90)}</button><span>{r.state.replaceAll("_", " ")}</span></li>)}</ul></>}
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
    <p>Your old credential is used once to read the agent definition and is not saved. Cloud agents use Arugula's account. Reconnect tools and secrets after importing; old conversations stay on the old account.</p>
    <label>Old agent ID<input required value={id} onInput={(e) => setId(e.currentTarget.value)} /></label>
    <label>Old API credential<input required type="password" autoComplete="off" value={key} onInput={(e) => setKey(e.currentTarget.value)} /></label>
    <button disabled={busy}>Import definition</button>{message && <p role="status">{message}</p>}
  </form>;
}

function SecretEditor({ names, refresh }: { names: string[]; refresh: () => Promise<void> }) {
  const [name, setName] = useState(""); const [value, setValue] = useState(""); const [error, setError] = useState(""); const [busy, setBusy] = useState(false);
  return <><p>Values are write-only. A Cloud workspace takes the ones it names when it's made.</p><form class="cloud-form" onSubmit={async (e) => {
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
  const call = useCloudApi();
  const [item, setItem] = useState<T | null>(null); const [events, setEvents] = useState<Event[]>([]); const [error, setError] = useState(""); const [answered, setAnswered] = useState<Record<string, string>>({}); const [pending, setPending] = useState<string[]>([]);
  const cursor = useRef(0);
  // What the poll last failed with, cleared once it reads again (a
  // maintenance window over, M141); an answer's failure stays.
  const failed = useRef("");
  useEffect(() => {
    let active = true, timer: ReturnType<typeof setTimeout>;
    const poll = async () => {
      try {
        const [r, page, requests] = await Promise.all([call<T>(path), call<{ events: Event[] }>(`${path}/events?after=${cursor.current}`), call<{ requests: Block[] }>(`${path}/requests`)]);
        if (!active) return;
        if (failed.current) { const was = failed.current; failed.current = ""; setError((e) => (e === was ? "" : e)); }
        setItem(r); setPending(requests.requests.flatMap((r) => r.request_id ? [r.request_id] : []));
        if (page.events.length) { cursor.current = Math.max(...page.events.map((e) => e.id)); setEvents((old) => [...old, ...page.events]); }
        if (!ended(r) || page.events.length === 1000) timer = setTimeout(() => void poll(), page.events.length === 1000 ? 0 : 2000);
      } catch (e) { if (active) { failed.current = (e as Error).message; setError(failed.current); timer = setTimeout(() => void poll(), 5000); } }
    };
    void poll(); return () => { active = false; clearTimeout(timer); };
  }, [path]);
  // What each request was answered with here: the option's id, or the
  // form's action.
  const answer = async (request: string, body: { option_id?: string; action?: string }) => {
    await call(`${path}/requests/${encodeURIComponent(request)}`, "POST", body);
    setAnswered((old) => ({ ...old, [request]: body.option_id ?? body.action ?? "answered" }));
  };
  return { item, events, error, setError, pending, answered, answer };
}

/** The transcript and its approvals, shared by a run, a Cloud workspace
 * tab and a past tab: the last failure, then each block, with permission
 * and question requests answerable while they're pending and `ended` is
 * false, and said how they were settled once they aren't. `lead` is the
 * prompt to show first when the transcript doesn't carry it. */
export function CloudTranscript({ lead, events, pending, answered, ended, answer, setError }: {
  lead?: string; events: Event[]; pending: string[]; answered: Record<string, string>; ended: boolean;
  answer: (request: string, body: { option_id?: string; action?: string }) => Promise<void>; setError: (error: string) => void;
}) {
  const scroller = useRef<HTMLDivElement>(null);
  const stick = useRef(true);
  useLayoutEffect(() => { if (scroller.current && stick.current) scroller.current.scrollTop = scroller.current.scrollHeight; });
  const open = (request: string) => !ended && pending.includes(request) && !(request in answered);
  const settled = settledRequests(events);
  const blocks = transcriptBlocks(events);
  // Managoat's transcript opens each turn with its prompt: a lead the
  // transcript already has isn't shown twice.
  const showLead = lead !== undefined && !blocks.some(({ block }) => block.kind === "prompt");
  /** How a request that's no longer open was settled. */
  const outcome = (request: string, options?: Block["options"]): string => {
    const named = (id?: string) => (id && options?.find((o) => o.optionId === id)?.name) || id;
    if (request in answered) return answered[request] === "decline" ? "Declined" : answered[request] === "accept" ? "Answered" : `Answered: ${named(answered[request])}`;
    const s = settled.get(request);
    if (s?.option) return `Answered: ${named(s.option)}`;
    if (s?.outcome && s.outcome !== "answered") return `Not answered (${s.outcome.replaceAll("_", " ")})`;
    if (s) return "Answered";
    return pending.includes(request) ? "Waiting" : "No longer waiting";
  };
  return <>
    {events.filter((event) => event.state === "failed").slice(-1).map((event) => <p key={event.id} role="alert" class="control-error">{eventFailure(event)}</p>)}
    <div class="agent-log cloud-transcript" ref={scroller} onScroll={(e) => { const el = e.currentTarget; stick.current = el.scrollHeight - el.scrollTop - el.clientHeight < 40; }}>{showLead && <div class="agent-user">{lead}</div>}{blocks.map(({ key, block }) => {
      const body = <CloudBlock block={block} open={open} outcome={outcome} answer={answer} setError={setError} />;
      return body && <div key={key}>{body}</div>;
    })}</div>
  </>;
}

function CloudBlock({ block, open, outcome, answer, setError }: {
  block: Block; open: (request: string) => boolean; outcome: (request: string, options?: Block["options"]) => string;
  answer: (request: string, body: { option_id?: string; action?: string }) => Promise<void>; setError: (error: string) => void;
}) {
  const request = block.request_id;
  if (block.kind === "permission_request" && request) {
    return <div class="agent-perm" role={open(request) ? "alertdialog" : undefined} aria-label={block.summary ?? "Agent approval"} data-request={request} data-settled={open(request) ? undefined : ""}>
      <pre class="agent-perm-cmd">{block.summary || block.name}</pre>
      {open(request)
        ? block.options?.map((option) => <button key={option.optionId} onClick={() => void answer(request, { option_id: option.optionId }).catch((e: Error) => setError(e.message))}>{option.name ?? option.optionId}</button>)
        : <p class="agent-perm-done">{outcome(request, block.options)}</p>}
    </div>;
  }
  if (block.kind === "elicitation_request" && request) {
    return open(request) ? <Question block={block} answer={(body) => answer(request, body)} />
      : <div class="agent-perm" data-request={request} data-settled=""><p>{String(block.body ?? "The agent asked a question")}</p><p class="agent-perm-done">{outcome(request)}</p></div>;
  }
  if (block.kind === "prompt" && typeof block.body === "string") return <div class="agent-user">{block.body}</div>;
  // A tool's output is text it printed, not markdown.
  if (block.kind === "tool_result") {
    if (emptyBody(block.body)) return null;
    const failed = (block as { error?: boolean }).error === true;
    return <details class={`agent-tool cloud-tool${failed ? " failed" : ""}`}><summary class="agent-tool-head"><span class="agent-tool-title">{failed ? "Tool failed" : "Tool output"}</span></summary><pre class="agent-tool-text">{typeof block.body === "string" ? block.body : JSON.stringify(block.body, null, 2)}</pre></details>;
  }
  if (block.kind === "tool_use") {
    const title = [block.name, block.summary].filter(Boolean).join(" · ") || "tool";
    return emptyBody(block.body)
      ? <div class="agent-tool cloud-tool"><div class="agent-tool-head"><span class="agent-tool-title">{title}</span></div></div>
      : <details class="agent-tool cloud-tool"><summary class="agent-tool-head"><span class="agent-tool-title">{title}</span></summary><pre class="agent-tool-text">{typeof block.body === "string" ? block.body : JSON.stringify(block.body, null, 2)}</pre></details>;
  }
  const text = !emptyBody(block.body) ? (typeof block.body === "string" ? block.body : JSON.stringify(block.body, null, 2)) : block.summary ?? "";
  if (!text.trim()) return null;
  return <AgentMessage text={text} thought={block.kind === "thinking" || block.kind === "thought"} />;
}

/** A past run, read only: what it was asked, its transcript, and its saved
 * results to download. Runs are retired (S40, M119). */
export function RunDetails({ id, back }: { id: string; back?: () => void }) {
  const { item: run, events, error, setError, pending, answered, answer } = useCloudConversation<Run>(`runs/${id}`, terminal);
  return <div class="agent cloud-agent" data-cloud-pane={id}>{back && <button onClick={back}>Back to runs</button>}{error && <p role="alert">{error}</p>}{run && <>
    <div class="agent-bar"><span class="agent-status">{run.state.replaceAll("_", " ")}</span><span class="agent-name">{run.spec.agent.name}</span><span class="host-tag">Past run</span></div><div class="agent-note">{run.spec.repository} · {run.spec.git_ref || "default branch"} · Read only: runs are retired, and agents now run in Cloud workspaces.</div>
    <CloudTranscript lead={run.spec.prompt} events={events} pending={pending} answered={answered} ended answer={answer} setError={setError} />
    {run.result && <><h3>Saved results</h3><button onClick={() => { const url = URL.createObjectURL(new Blob([JSON.stringify({ run, events }, null, 2)], { type: "application/json" })); const a = document.createElement("a"); a.href = url; a.download = `${id}.json`; a.click(); setTimeout(() => URL.revokeObjectURL(url), 1000); }}>Download results and transcript</button><SavedResults result={run.result} /></>}
  </>}</div>;
}

/** A closed tab, read only: its transcript, kept after its conversation
 * ended and after its Cloud workspace was deleted. */
export function PastTabDetails({ id }: { id: string }) {
  const { item: tab, events, error, setError, pending, answered, answer } = useCloudConversation<PastCloudTab>(`past-tabs/${id}`, () => true);
  return <div class="agent cloud-agent" data-cloud-pane={id} data-past-tab-view={id}>{error && <p role="alert">{error}</p>}{tab && <>
    <div class="agent-bar"><span class="agent-status">Closed</span><span class="agent-name">{tab.agent_name}</span><span class="host-tag" title={tab.model}>{tab.runtime}</span><span class="host-tag">Past tab</span></div>
    <div class="agent-note">{tab.cloud_workspace_name}{tab.cloud_workspace_deleted ? " (deleted)" : ""} · <span class="mono">{tab.cwd}</span> · Read only: its conversation has ended.</div>
    <CloudTranscript events={events} pending={pending} answered={answered} ended answer={answer} setError={setError} />
  </>}</div>;
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

function Question({ block, answer }: { block: Block; answer: (body: { action: string; content?: unknown }) => Promise<void> }) {
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
    {error && <p role="alert">{error}</p>}<button disabled={busy}>Answer</button><button type="button" disabled={busy} onClick={() => void answer({ action: "decline" }).catch((e: Error) => setError(e.message))}>Decline</button>
  </form>;
}

/** The shown past run or past tab, as a tab: closing it goes back to the
 * machine. */
export function CloudTabs() {
  const selected = useCloudPanes();
  if (!selected) return null;
  const what = selected.startsWith("cwc_") ? "Past tab" : "Past run";
  return <div class="tab selected" role="tab" aria-selected key={selected}><span class="host-tag">Cloud</span><span class="tab-label">{what}</span><button class="tab-close" title={`Close ${what.toLowerCase()}`} aria-label={`Close ${what.toLowerCase()}`} onClick={(e) => { e.stopPropagation(); cloudPanes.local(); }}>×</button></div>;
}
