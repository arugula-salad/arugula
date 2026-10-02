// S21 (throwaway, branch only): a chant workspace as a block. The daemon
// reads it through chant's read contract; this draws what it read: gates
// waiting on you first, then a card per member (open a shell, an agent or
// its changes there), then the records.

import { render } from "preact";
import { useState } from "preact/hooks";
import type { Client } from "../client";
import type { PaneId } from "../proto";
import { registerBlock, type BlockView } from "./view";

interface Diagnostic { rule: string; severity: string; message: string; file: string | null; line: number | null }
interface Member {
  name: string; dir: string; path: string; kind: string; because: string | null; roles: string[]; nested: boolean;
  unreadable: string | null; errors: number; warnings: number; diagnostics: Diagnostic[]; releases: number; gates: number;
}
interface Gate { member: string; path: string; op: string; gate: string; since: string | null; expires: string | null; needed: number; approvals: number; approve: string | null }
interface Rec { kind: string; id: string; title: string | null; state: string | null; ready: boolean | null; blocked_by: string[]; warnings: string[]; valid: boolean }
interface Read { name: string; ms: number; code: number; ok: boolean; note: string | null }
export interface WorkspaceState {
  root: string; name: string | null; chant: string | null; how: string | null; version: string | null; env: string;
  members: Member[]; records: Rec[]; records_note: string | null; gates: Gate[]; diagnostics: Diagnostic[];
  reads: Read[]; ms: number; error: string | null; loading: boolean; updated_ms: number; watching?: boolean;
}

function WorkspaceBlock({ client, id, s }: { client: Client; id: PaneId; s: WorkspaceState | null }) {
  const [busy, setBusy] = useState<string | null>(null);
  const [said, setSaid] = useState<string | null>(null);
  const shell = (m: Member) => client.make("/api/run", { split: id, cwd: m.path, from_pane: id });
  const agent = (m: Member) => client.openBlock({ type: "agent", config: { agent: "claude", cwd: m.path }, split: id, from_pane: id }, "couldn't start the agent");
  const changes = (m: Member) => client.openBlock({ type: "diff", config: { repo: m.path }, split: id, from_pane: id }, "couldn't show the changes");
  const nested = (m: Member) => client.openBlock({ type: "workspace", config: { root: m.path, env: s?.env ?? "local" }, split: id, from_pane: id }, "couldn't open it");
  const runOp = (g: Gate) => client.make("/api/run", { split: id, cwd: g.path, from_pane: id, command: `${s?.chant ?? "chant"} run ${g.op}` });
  const approve = async (g: Gate) => {
    setBusy(`${g.member}/${g.op}/${g.gate}`);
    const r = await client.api(`/api/blocks/${id}/call/approve`, { member: g.member, op: g.op, gate: g.gate }, "couldn't approve");
    setBusy(null);
    if (r) setSaid(`Approved ${g.gate}. Run ${g.op} again to walk through it.`);
  };
  const refresh = () => client.api(`/api/blocks/${id}/call/refresh`, {}, "couldn't read it");

  if (!s) return <div class="review"><div class="browser-card dim">Reading the workspace…</div></div>;
  return (
    <div class="review ws" data-workspace-block={id}>
      <div class="review-bar">
        <span class="review-path" title={s.root}>
          <b>{s.name ?? "workspace"}</b> <span class="dim">{s.root}</span>
        </span>
        <span class="dim" title={s.chant ?? ""}>chant {s.version ?? "?"} · {s.how ?? "—"} · {s.ms} ms</span>
        <button onClick={refresh}>Refresh</button>
        <span class={`review-live ${s.watching ? "on" : ""}`}>{s.watching ? "live" : "paused"}</span>
      </div>
      {s.error && (
        <div class="browser-card error">
          <p>Can't show this workspace</p>
          <p class="dim">{s.error}</p>
        </div>
      )}
      {s.loading && !s.error && <div class="browser-card dim">Reading the workspace…</div>}
      <div class="ws-body">
        {s.gates.length > 0 && (
          <section class="ws-gates">
            <h4>Waiting on you</h4>
            {s.gates.map((g) => (
              <div class="ws-gate" key={`${g.member}/${g.op}/${g.gate}`}>
                <span>
                  <b>{g.member}</b> / {g.op} — gate <b>{g.gate}</b>
                  <span class="dim"> ({g.approvals}/{g.needed}){g.expires ? ` · expires ${new Date(g.expires).toLocaleString()}` : ""}</span>
                </span>
                <button disabled={busy !== null} onClick={() => approve(g)}>Approve</button>
                <button onClick={() => runOp(g)}>Run {g.op}</button>
              </div>
            ))}
            {said && <p class="dim">{said}</p>}
          </section>
        )}
        {s.diagnostics.length > 0 && (
          <section>
            {s.diagnostics.map((d, i) => <p key={i} class={`ws-diag ${d.severity}`}>{d.rule}: {d.message}</p>)}
          </section>
        )}
        <section class="ws-members">
          {s.members.map((m) => (
            <div class={`ws-card ${m.errors || m.unreadable ? "bad" : ""} ${m.gates ? "waits" : ""}`} key={m.name} title={m.because ?? ""}>
              <div class="ws-card-head">
                <b>{m.name}</b>
                <span class="ws-kind">{m.kind}</span>
              </div>
              <div class="dim ws-dir">{m.dir}</div>
              <div class="ws-tags">
                {m.gates > 0 && <span class="ws-tag waits">{m.gates} gate{m.gates > 1 ? "s" : ""}</span>}
                {m.errors > 0 && <span class="ws-tag bad">{m.errors} errors</span>}
                {m.warnings > 0 && <span class="ws-tag">{m.warnings} warnings</span>}
                {m.releases > 0 && <span class="ws-tag">{m.releases} releases</span>}
                {m.unreadable && <span class="ws-tag bad">{m.unreadable}</span>}
              </div>
              <div class="ws-actions">
                {m.nested ? (
                  <button onClick={() => nested(m)}>Open</button>
                ) : (
                  <>
                    <button onClick={() => shell(m)}>Shell</button>
                    <button onClick={() => agent(m)}>Agent</button>
                    <button onClick={() => changes(m)}>Changes</button>
                  </>
                )}
              </div>
            </div>
          ))}
        </section>
        {s.records.length > 0 ? (
          <section class="ws-records">
            <h4>Records</h4>
            {s.records.map((r) => (
              <div class="ws-record" key={`${r.kind}/${r.id}`}>
                <b>{r.id}</b> <span class="ws-kind">{r.state ?? "—"}</span> {r.title}
                {r.blocked_by.length > 0 && <span class="dim"> · blocked by {r.blocked_by.join(", ")}</span>}
                {r.warnings.map((w, i) => <div key={i} class="ws-diag warning">{w}</div>)}
              </div>
            ))}
          </section>
        ) : (
          s.records_note && <p class="dim ws-note">Records: {s.records_note}</p>
        )}
        <p class="dim ws-note">{s.reads.map((r) => `${r.name} ${r.ms}ms${r.ok ? "" : " ✗"}`).join(" · ")}</p>
      </div>
    </div>
  );
}

function plain(s: WorkspaceState | null): string {
  if (!s) return "";
  const lines = [`${s.name ?? "workspace"} ${s.root}`];
  if (s.error) lines.push(s.error);
  for (const g of s.gates) lines.push(`waiting: ${g.member}/${g.op} gate ${g.gate}`);
  for (const m of s.members) lines.push(`${m.name} ${m.kind} ${m.dir}`);
  for (const r of s.records) lines.push(`${r.id} ${r.state ?? ""} ${r.title ?? ""}`);
  return lines.join("\n");
}

registerBlock("workspace", (client, id): BlockView => {
  const host = document.createElement("div");
  host.className = "block block-workspace";
  let state: WorkspaceState | null = null;
  const draw = () => render(<WorkspaceBlock client={client} id={id} s={state} />, host);
  draw();
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
      render(null, host);
      host.remove();
    },
  };
});
