// Workspace blocks (M34): a chant workspace, read by the daemon through
// chant's read contract. Gates waiting for a person come first (approved
// here by the owner and editors), then a card per member to open a shell,
// an agent, its changes or a nested workspace on, then the records.
// Everything drawn comes from the daemon's state, so a shared session's
// viewers see the same, without the buttons.

import { render } from "preact";
import { useState } from "preact/hooks";
import type { Client } from "../client";
import { EXPIRE_TITLE, gateKey, type Gate, type PaneId, type RunRequest, type WorkspaceRecord } from "../proto";
import { openWorkspace } from "./open-labs";
import type { BlockRenderer, BlockView } from "./view";
import { ago, decisionLine, GateWhy } from "../ui/gate-why";

interface Diagnostic { rule: string; severity: string; message: string; file: string | null; line: number | null }
interface Member {
  name: string; dir: string; path: string; kind: string; because: string | null; roles: string[]; nested: boolean;
  unreadable: string | null; errors: number; warnings: number; diagnostics: Diagnostic[]; releases: number; gates: number;
  agents: string[]; ops: string[];
}
interface Read { name: string; ms: number; code: number; ok: boolean; note: string | null; reason: string | null }
export interface WorkspaceState {
  root: string; name: string | null; chant: string | null; how: string | null; version: string | null; env: string;
  members: Member[]; records: WorkspaceRecord[]; records_note: string | null; gates: Gate[]; diagnostics: Diagnostic[];
  reads: Read[]; ms: number; error: string | null; error_code: string | null; headline: string | null; loading: boolean; updated_ms: number; watching?: boolean;
  /** The envs chant has releases for, with `local` and the one watched (#312). */
  envs?: string[];
  /** Who approvals here are recorded as (#302): the owner's chant
   * principal, and editors' by their Arugula name. */
  actor?: string | null;
  principals?: Record<string, string>;
}

/** Editors' principals as lines, `name=principal`, and back. */
function principalLines(p: Record<string, string> | undefined): string {
  return Object.entries(p ?? {})
    .map(([n, v]) => `${n}=${v}`)
    .join("\n");
}
function parsePrincipals(text: string): Record<string, string> | string {
  const out: Record<string, string> = {};
  for (const line of text.split("\n")) {
    if (!line.trim()) continue;
    const at = line.indexOf("=");
    if (at < 1) return `"${line.trim()}": a line is name=principal`;
    out[line.slice(0, at).trim()] = line.slice(at + 1).trim();
  }
  return out;
}

/** The owner's say over who chant records approvals as (#302): the
 * principal they approve as, and editors'. */
function Principals({ client, id, s, close }: { client: Client; id: PaneId; s: WorkspaceState; close: () => void }) {
  const [actor, setActor] = useState(s.actor ?? "");
  const [editors, setEditors] = useState(principalLines(s.principals));
  const save = async () => {
    const principals = parsePrincipals(editors);
    if (typeof principals === "string") return client.toast(principals);
    const ok = await client.api(`/api/blocks/${id}/call/principals`, { actor: actor.trim(), principals }, "couldn't set the principals");
    if (ok) close();
  };
  return (
    <div class="browser-card ws-principals" data-ws-principals>
      <label>
        You approve as
        <input placeholder="github:you (default: your Arugula name)" value={actor} onInput={(e) => setActor(e.currentTarget.value)} />
      </label>
      <label>
        Editors approve as, one a line
        <textarea rows={3} placeholder="sam=github:sam-h" value={editors} onInput={(e) => setEditors(e.currentTarget.value)} />
      </label>
      <p class="dim ws-note">The chant principal its ledger records for an approval. A name not listed is passed as is.</p>
      <div class="ws-actions">
        <button class="pri" data-ws-principals-save onClick={() => void save()}>
          Save
        </button>
        <button onClick={close}>Cancel</button>
      </div>
    </div>
  );
}

/** What to do about a failure, by chant's reason code (or the reader's). */
function hint(error: string, code: string | null, root: string): string | null {
  if (error.startsWith("no chant here")) return `Run npm install in ${root}, or set CHANT for the daemon.`;
  switch (code) {
    case "declaration-missing":
      return "This directory isn't in a chant workspace: chant workspace init proposes one.";
    case "declaration-ambiguous":
      return "Keep one of chant.workspace.json and chant.workspace.jsonc.";
    case "reader-too-old":
      return "The declaration's minReader is newer than this chant: install a newer @intentius/chant at the root.";
    case "root-chant-required":
      return "The declaration pins another chant: run npm install at the workspace root.";
    case "contract-unknown":
      return "This Arugula reads contract 1: update Arugula, or install a chant that writes contract 1.";
  }
  return error.startsWith("no chant.workspace.json") ? "This directory isn't in a chant workspace." : null;
}

/** The env menu's entry for one it doesn't list. */
const OTHER = "\u0000other";

function WorkspaceBlock({ client, id, s }: { client: Client; id: PaneId; s: WorkspaceState | null }) {
  const [busy, setBusy] = useState<string | null>(null);
  const [said, setSaid] = useState<string | null>(null);
  // #309: the member whose Run op is open, and the op name typed there.
  const [picking, setPicking] = useState<string | null>(null);
  const [typed, setTyped] = useState("");
  const [who, setWho] = useState(false);
  const session = client.sessionOfTab(client.tabOfPane(id)?.id ?? -1) ?? null;
  const role = client.role(session);
  // Approving is for the owner and editors (#75); opening panes and blocks
  // on the host is the owner's.
  const mayApprove = role !== "viewer";
  const mayOpen = role === "owner";
  const beside = { split: id, from_pane: id };
  const shell = (cwd: string) => void client.make("/api/run", { ...beside, cwd } satisfies RunRequest).then((e) => e && client.toast(e));
  // #304: as the member's agent session (CHANT_AGENT), writing a run record per turn.
  const agent = (m: Member) =>
    void client.openBlock(
      {
        type: "agent",
        config: { agent: "claude", cwd: m.path, chant: { root: s?.root, member: m.name, agent: m.agents[0], chant: s?.chant ?? undefined } },
        ...beside,
      },
      "couldn't start the agent",
    );
  const changes = (m: Member) => void client.openBlock({ type: "diff", config: { repo: m.path }, ...beside }, "couldn't show the changes");
  const nested = (m: Member) => openWorkspace(client, m.path, id, s?.env ?? "local");
  const run = (cwd: string, op: string) =>
    void client.make("/api/run", { ...beside, cwd, command: `${s?.chant ?? "chant"} run ${op}` } satisfies RunRequest).then((e) => e && client.toast(e));
  const runOp = (g: Gate) => g.source.kind === "chant" && run(g.source.dir, g.op);
  // A typed op name goes on a command line: a name, nothing a shell reads.
  const runTyped = (m: Member) => {
    const op = typed.trim();
    if (!/^[\w.:/-]+$/.test(op)) return client.toast("an op name: letters, digits, '.', '_', ':', '/' and '-'");
    setPicking(null);
    run(m.path, op);
  };
  const approve = async (g: Gate) => {
    setBusy(`approve:${gateKey(g)}`);
    setSaid(null);
    const ok = await client.api(`/api/blocks/${id}/call/approve`, { key: gateKey(g) }, "couldn't approve it");
    setBusy(null);
    if (ok) setSaid(`Approved ${g.gate}. Run ${g.op} again to walk through it.`);
  };
  // #310: turned down, not approved.
  const expire = async (g: Gate) => {
    setBusy(`expire:${gateKey(g)}`);
    setSaid(null);
    const ok = await client.api(`/api/blocks/${id}/call/expire`, { key: gateKey(g) }, "couldn't expire it");
    setBusy(null);
    if (ok) setSaid(`Expired ${g.gate}, not approved. The next run of ${g.op} stops there again.`);
  };
  const refresh = () => void client.api(`/api/blocks/${id}/call/refresh`, {}, "couldn't read the workspace");
  // The block watches one env's gates and releases (#312): switching reads
  // again, and the choice is kept in its config.
  const switchEnv = (name: string) => {
    if (name === OTHER) name = window.prompt("Which environment?", "")?.trim() ?? "";
    if (name && name !== s?.env) void client.api(`/api/blocks/${id}/call/env`, { name }, "couldn't switch the environment");
  };

  if (!s || (s.loading && !s.updated_ms)) {
    return (
      <div class="review ws">
        <div class="browser-card dim">Reading the workspace…</div>
      </div>
    );
  }
  const help = s.error ? hint(s.error, s.error_code, s.root) : null;
  return (
    <div class="review ws" data-workspace-block={id}>
      <div class="review-bar">
        <span class="review-path" title={s.root}>
          <b>{s.name ?? "workspace"}</b> {s.root}
        </span>
        {mayApprove ? (
          <select
            class="ws-env"
            data-ws-env
            title="The environment whose gates and releases this block watches"
            value={s.env}
            disabled={s.loading}
            onChange={(e) => {
              const name = e.currentTarget.value;
              // Shows the env watched until the block says it switched.
              e.currentTarget.value = s.env;
              switchEnv(name);
            }}
          >
            {(s.envs ?? [s.env]).map((e) => (
              <option key={e} value={e}>
                {e}
              </option>
            ))}
            <option value={OTHER}>other…</option>
          </select>
        ) : (
          <span class="dim ws-meta" title="The environment whose gates and releases this block watches">
            {s.env}
          </span>
        )}
        <span class="dim ws-meta" title={s.chant ?? ""}>
          chant {s.version ?? "?"}
        </span>
        {mayOpen && (
          <button class="ws-meta" data-ws-actor title="Who chant records approvals here as" onClick={() => setWho(!who)}>
            as {s.actor || "you"}
          </button>
        )}
        {mayOpen && (
          <button title="Read it again" disabled={s.loading} onClick={refresh}>
            {s.loading ? "…" : "↻"}
          </button>
        )}
        <span class={`review-live ${s.watching ? "on" : ""}`}>{s.watching ? "live" : "paused"}</span>
      </div>
      {mayOpen && who && <Principals client={client} id={id} s={s} close={() => setWho(false)} />}
      {s.error ? (
        <div class="browser-card" data-ws-error>
          <p>Can't show this workspace</p>
          <p class="dim">
            {s.error}
            {s.error_code && (
              <>
                {" "}
                <code data-ws-reason>{s.error_code}</code>
              </>
            )}
          </p>
          {help && <p class="dim">{help}</p>}
          {mayOpen && <button onClick={refresh}>Try again</button>}
        </div>
      ) : (
        <div class="review-body ws-body">
          {s.gates.length > 0 && (
            <section class="ws-gates" data-ws-gates>
              <h4>Waiting on you</h4>
              {s.gates.map((g) => (
                <div class="ws-gate" key={gateKey(g)} data-gate={gateKey(g)}>
                  <div class="ws-gate-what">
                    <b>{g.member}</b>: {g.op} waits at gate <b>{g.gate}</b>
                    <div class="dim ws-gate-when">
                      {g.approvals}/{g.needed} approvals
                      {g.since && ` · ${ago(g.since)}`}
                      {g.expires && ` · expires ${new Date(g.expires).toLocaleString()}`}
                    </div>
                    <GateWhy gate={g} />
                  </div>
                  <div class="ws-actions">
                    {mayApprove && (
                      <button class="pri" data-approve disabled={busy !== null} onClick={() => void approve(g)}>
                        {busy === `approve:${gateKey(g)}` ? "Approving…" : "Approve"}
                      </button>
                    )}
                    {mayApprove && g.source.kind === "chant" && (
                      <button data-expire disabled={busy !== null} title={EXPIRE_TITLE} onClick={() => void expire(g)}>
                        {busy === `expire:${gateKey(g)}` ? "Expiring…" : "Expire"}
                      </button>
                    )}
                    {mayOpen && <button onClick={() => runOp(g)}>Run {g.op}</button>}
                  </div>
                </div>
              ))}
              {!mayApprove && <p class="dim ws-note">You're watching this session: the owner or an editor approves.</p>}
            </section>
          )}
          {said && <p class="ws-said" data-ws-said>{said}</p>}
          {s.diagnostics.length > 0 && (
            <section>
              {s.diagnostics.map((d, i) => (
                <p key={i} class={`ws-diag ${d.severity}`}>
                  {d.rule}: {d.message}
                </p>
              ))}
            </section>
          )}
          <section>
            <h4>Members ({s.members.length})</h4>
            <div class="ws-members">
              {s.members.map((m) => {
                const problems = m.diagnostics.map((d) => `${d.rule}: ${d.message}`).join("\n");
                return (
                  <div class={`ws-card${m.errors || m.unreadable ? " bad" : ""}${m.gates ? " waits" : ""}`} key={m.name} data-member={m.name}>
                    <div class="ws-card-head">
                      <b title={m.name}>{m.name}</b>
                      <span class="ws-kind" title={m.because ?? ""}>
                        {m.kind}
                      </span>
                    </div>
                    <div class="dim ws-dir" title={m.path}>
                      {m.dir}
                    </div>
                    <div class="ws-tags" title={problems}>
                      {m.gates > 0 && <span class="ws-tag waits">{m.gates === 1 ? "gate waits" : `${m.gates} gates wait`}</span>}
                      {m.errors > 0 && <span class="ws-tag bad">{m.errors} errors</span>}
                      {m.warnings > 0 && <span class="ws-tag">{m.warnings} warnings</span>}
                      {m.releases > 0 && <span class="ws-tag">{m.releases} releases</span>}
                      {m.unreadable && <span class="ws-tag bad">{m.unreadable}</span>}
                      {m.roles.map((r) => (
                        <span key={r} class="ws-tag">
                          {r}
                        </span>
                      ))}
                    </div>
                    {mayOpen && (
                      <div class="ws-actions">
                        {m.nested ? (
                          <button data-open-nested onClick={() => nested(m)}>
                            Open
                          </button>
                        ) : (
                          <>
                            <button onClick={() => shell(m.path)}>Shell</button>
                            <button
                              title={m.agents[0] ? `As agent session ${m.agents[0]}` : "The declaration binds no agent session to this member"}
                              onClick={() => agent(m)}
                            >
                              Agent
                            </button>
                            <button onClick={() => changes(m)}>Changes</button>
                            <button data-run-op onClick={() => (setPicking(picking === m.name ? null : m.name), setTyped(""))}>
                              Run op
                            </button>
                          </>
                        )}
                      </div>
                    )}
                    {mayOpen && picking === m.name && (
                      <div class="ws-actions" data-run-ops={m.name}>
                        {m.ops.map((op) => (
                          <button key={op} data-op={op} onClick={() => (setPicking(null), run(m.path, op))}>
                            {op}
                          </button>
                        ))}
                        <input
                          placeholder={m.ops.length ? "another op" : "op name"}
                          value={typed}
                          onInput={(e) => setTyped(e.currentTarget.value)}
                          onKeyDown={(e) => e.key === "Enter" && runTyped(m)}
                        />
                        <button disabled={!typed.trim()} onClick={() => runTyped(m)}>
                          Run
                        </button>
                      </div>
                    )}
                  </div>
                );
              })}
            </div>
          </section>
          {s.records.length > 0 ? (
            <section class="ws-records">
              <h4>Records ({s.records.length})</h4>
              {s.records.map((r) => (
                <div class="ws-record" key={`${r.kind}/${r.id}`}>
                  <b>{r.id}</b> <span class="ws-kind">{r.state ?? "—"}</span> {r.title}
                  {r.blocked_by.length > 0 && <span class="dim"> · blocked by {r.blocked_by.join(", ")}</span>}
                  {r.warnings.map((w, i) => (
                    <div key={i} class="ws-diag warning">
                      {w}
                    </div>
                  ))}
                </div>
              ))}
            </section>
          ) : (
            s.records_note && <p class="dim ws-note">Records: {s.records_note}</p>
          )}
          <p class="dim ws-note">
            {s.reads.map((r) => `${r.name} ${r.ms} ms${r.ok ? "" : ` (failed${r.reason ? `: ${r.reason}` : ""})`}`).join(" · ")} · read {ago(new Date(s.updated_ms).toISOString())}
          </p>
        </div>
      )}
    </div>
  );
}

function plain(s: WorkspaceState | null): string {
  if (!s) return "";
  const lines = [`${s.name ?? "workspace"} ${s.root}`];
  if (s.error) lines.push(s.error);
  for (const g of s.gates) {
    lines.push(`waiting: ${g.member}: ${g.op} at gate ${g.gate}`);
    const d = decisionLine(g);
    if (d) lines.push(`  ${d}`);
  }
  for (const m of s.members) lines.push(`${m.name} ${m.kind} ${m.dir}`);
  for (const r of s.records) lines.push(`${r.id} ${r.state ?? ""} ${r.title ?? ""}`);
  return lines.join("\n");
}

/** How to draw a workspace block: the Labs entry hands it to the page. */
export const workspaceBlock: BlockRenderer = (client, id): BlockView => {
  const host = document.createElement("div");
  host.className = "block block-workspace";
  let state: WorkspaceState | null = null;
  const draw = () => render(<WorkspaceBlock client={client} id={id} s={state} />, host);
  draw();
  // Roles can change (a share made view-only) without the block changing.
  const off = client.subscribe(draw);
  return {
    host,
    setVisible: (v) => (host.style.display = v ? "" : "none"),
    update: (s) => {
      state = s as WorkspaceState;
      draw();
    },
    title: () => `${state?.name ?? "workspace"} (chant)`,
    text: () => plain(state),
    focus: () => host.querySelector<HTMLElement>("button")?.focus(),
    dispose: () => {
      off();
      render(null, host);
      host.remove();
    },
  };
};
