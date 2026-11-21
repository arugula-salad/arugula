// What a machine's agents may do, in Settings › Machines' Agents (Arugula
// #1107): a row per project, and your DMs, with eight columns. Threads &
// issues (M197) lets the machine's agents read and post in the project's
// threads and write its issues; Runs (M216) lets them read its runs and
// start its run specs; Commands (M216, apart) start commands of their own
// there; Deploys (M218) plan its deploys and ask for approval: never
// approve; Start sessions and Answer sessions (aru-121) start agent
// sessions as you, and answer or prompt them; Plane writes (#359); and
// Boards (#293, a select of three levels, not a switch) lets them read its
// boards, or read them and write their rows. Your DMs get one, Threads
// (M197). Off until you turn one on, and only over what you read; what
// they write is labelled as your agent's, and every room they reach says
// so. The same grants as `arugula grants add` (`/api/grants`, the
// platform's). What each switch reads of the grants is ./grants's, tested.

import { useEffect, useState } from "preact/hooks";
import { platformFetch } from "../../shared/maintenance";
import { useSubscribe } from "../../shared/ui/hooks";
import { session } from "../session";
import {
  boardsLimit,
  boardsLevel,
  boardsMove,
  boardsRevoked,
  cellKey,
  COLUMNS,
  columnRefused,
  COMMAND_POWERS,
  forRole,
  liveOver,
  machineGrantsSummary,
  matrixOf,
  powersFor,
  RUN_POWERS,
  SINGLE,
  singleGrant,
  type AgentScope,
  type BoardsLevel,
  type Cell,
  type Column,
  type Grant,
  type Role,
} from "./grants";
import { Time } from "../../shared/ui/time";
import { roleIn } from "./environments";
import { Section } from "../../shared/ui/settings-section";

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

// Each column's head, the word in a switch's name, and what it gives: the
// head's tooltip.
type Head = { head: string; short?: string; name: string; gives: string };
const HEADS: Record<Column, Head> = {
  threads: { head: "Threads & issues", short: "Threads", name: "threads & issues", gives: "read and post in its threads (a team's channels), write its issues, and write its Docs" },
  runs: { head: "Runs", name: "runs", gives: "read its runs, and start its run specs on its cloud workspaces" },
  commands: { head: "Commands", name: "commands", gives: "start commands of their own on its cloud workspaces" },
  deploys: { head: "Deploys", name: "deploys", gives: "plan its deploys and ask its owners to approve them" },
  start: {
    head: "Start sessions",
    short: "Start",
    name: "start sessions",
    gives: "start agent sessions on its cloud workspaces as you: you're the asker, and you pay",
  },
  reply: {
    head: "Answer sessions",
    short: "Answer",
    name: "answer sessions",
    gives: "answer its agent sessions' questions and prompt them, as you (never approve a permission)",
  },
  plane: {
    head: "Plane writes",
    short: "Plane",
    name: "Plane writes",
    gives: "create, comment on, close, reopen and edit the work items of its connected Plane project, as you; whether each waits for your approval is set per Plane connection",
  },
  boards: {
    head: "Boards",
    name: "boards",
    gives: "read its boards (read), or read them and add, attach and ask on them (read and write)",
  },
};

const LEVEL_WORDS: Record<BoardsLevel, string> = { off: "Off", read: "Read", write: "Read and write" };

/** The boards column's control: its level, and a choice of any of the
 * three, which moves straight there. A viewer's machine reads at most
 * (Arugula #683): write isn't offered, and one from before shows as it is. */
function BoardsLevelSelect({ level, label, k, busy, role, turn }: { level: BoardsLevel; label: string; k: string; busy: boolean; role?: Role; turn: (to: BoardsLevel) => void }) {
  const levels = (Object.keys(LEVEL_WORDS) as BoardsLevel[]).filter((l) => l !== "write" || role !== "viewer" || level === "write");
  return (
    <select
      class={`agent-level${level === "off" ? "" : " on"}`}
      data-agent-cell={k}
      data-level={level}
      aria-label={label}
      aria-busy={busy || undefined}
      value={level}
      disabled={busy}
      onChange={(e) => turn((e.target as HTMLSelectElement).value as BoardsLevel)}
    >
      {levels.map((l) => (
        <option key={l} value={l}>
          {LEVEL_WORDS[l]}
        </option>
      ))}
    </select>
  );
}

/** What a reader of this role is told under a switch (Arugula #683):
 * what a live grant does on a viewer's ("idle", "reads only", with why
 * as its tooltip), and who revoked one that wasn't the giver, and when. */
function Says({ cell, k }: { cell: Pick<Cell, "limited" | "revoked">; k: string }) {
  return (
    <>
      {cell.limited && (
        <span class="agent-idle" title={cell.limited.why} data-agent-limited={k}>
          {cell.limited.label}
        </span>
      )}
      {cell.revoked && (
        <span class="agent-idle" data-agent-revoked={k}>
          revoked by <bdi>{cell.revoked.name}</bdi> <Time at={cell.revoked.at} />
        </span>
      )}
    </>
  );
}

/** A switch in a matrix: on, off, or partly on (some of what it gives,
 * said in its title and under it), and busy while it's being turned.
 * `refused`: nothing it gives is given to a viewer's machine (Arugula
 * #683), so it can't be turned on, and says why. */
function Switch({ cell, label, k, busy, refused, turn }: { cell: Cell; label: string; k: string; busy: boolean; refused?: string; turn: (on: boolean) => void }) {
  const only = cell.only ? `only ${cell.only.join(", ")}` : "";
  const off = !!refused && !cell.on;
  return (
    <>
      <input
        type="checkbox"
        role="switch"
        class={`sw-switch${cell.only ? " partial" : ""}`}
        data-agent-cell={k}
        data-partial={cell.only ? "" : undefined}
        data-refused={off ? "" : undefined}
        aria-label={label}
        aria-busy={busy || undefined}
        title={off ? refused : only || undefined}
        checked={cell.on}
        disabled={busy || off}
        onChange={(e) => turn((e.target as HTMLInputElement).checked)}
      />
      {only && <span class="agent-only">{only}</span>}
      <Says cell={cell} k={k} />
    </>
  );
}

/** Why a column's switch is off for good on a viewer's project. */
const REFUSED_WHY = "You only view this project, so this would do nothing for its agents: every write is refused. Ask an owner for the editor role.";

/** The account's grants (`/api/grants`) and the switches' turns: each
 * gives or revokes what its column gives, then reads them again. */
function useGrantSwitches() {
  const [grants, setGrants] = useState<Grant[] | null>(null);
  const [err, setErr] = useState("");
  // What a viewer's switch left out (Arugula #683): a note, not an error.
  const [note, setNote] = useState("");
  const [busy, setBusy] = useState("");
  const load = () =>
    call<{ grants: Grant[] }>("/api/grants").then(
      (v) => setGrants(v.grants),
      (e: Error) => setErr(e.message),
    );
  useEffect(() => void load(), []);
  // A column that's one grant: deploys, start and reply.
  const turnSingle = async (all: Grant[], machine: string, s: AgentScope, column: Column, power: string, on: boolean) => {
    const project = s.id!;
    setErr("");
    setBusy(cellKey(machine, s, column));
    try {
      const g = singleGrant(all, machine, project, power);
      if (on && !g) await call("/api/grants", { daemon: machine, power, scope: { kind: "project", id: project } });
      if (!on && g) await call(`/api/grants/${encodeURIComponent(g.id)}/revoke`, {});
    } catch (e) {
      setErr((e as Error).message);
    }
    await load();
    setBusy("");
  };
  // The boards column: to a level, giving and revoking what that takes.
  const turnBoards = async (machine: string, s: AgentScope, to: BoardsLevel) => {
    const project = s.id!;
    setErr("");
    setBusy(cellKey(machine, s, "boards"));
    try {
      const { give, revoke } = boardsMove(grants ?? [], machine, project, to);
      for (const power of give) await call("/api/grants", { daemon: machine, power, scope: { kind: "project", id: project } });
      for (const g of revoke) await call(`/api/grants/${encodeURIComponent(g.id)}/revoke`, {});
    } catch (e) {
      setErr((e as Error).message);
    }
    await load();
    setBusy("");
  };
  const turn = async (all: Grant[], machine: string, s: AgentScope, on: boolean, which?: "runs" | "commands") => {
    setErr("");
    setBusy(cellKey(machine, s, which ?? "threads"));
    const gives = which === "runs" ? RUN_POWERS : which === "commands" ? COMMAND_POWERS : powersFor(s.kind);
    // A viewer's machine is given what still works for it (Arugula #683),
    // and told what wasn't asked for.
    const powers = forRole(gives, s.role);
    const left = gives.filter((p) => !powers.includes(p));
    setNote("");
    try {
      if (on) {
        const scope = s.kind === "project" ? { kind: "project", id: s.id } : { kind: "account" };
        for (const power of powers) await call("/api/grants", { daemon: machine, power, scope });
        if (left.length) setNote(`Given without ${left.join(" and ")}: you only view ${s.name}, so those would do nothing for its agents.`);
      } else {
        for (const g of liveOver(all, machine, s, which ? gives : undefined)) {
          await call(`/api/grants/${encodeURIComponent(g.id)}/revoke`, {});
        }
      }
    } catch (e) {
      setErr((e as Error).message);
    }
    await load();
    setBusy("");
  };
  const turnCell = (machine: string, s: AgentScope, column: Column, on: boolean) => {
    const all = grants ?? [];
    const single = SINGLE[column];
    if (single) return void turnSingle(all, machine, s, column, single, on);
    void turn(all, machine, s, on, column === "threads" ? undefined : column === "runs" ? "runs" : "commands");
  };
  return { grants, err, note, busy, turnCell, turnBoards };
}

/** What a machine's agents may do (Arugula #1107), a part of Settings ›
 * Machines' Agents: a row per project and one for your DMs, a switch per column.
 * "Agents on your machines" (You) and the machine's read-only *Agents'
 * grants…* were two views of these; this is the one. */
export function MachineAgentGrants({ machine }: { machine: { id: string; name: string } }) {
  useSubscribe((fn) => session.subscribe(fn));
  const { grants, err, note, busy, turnCell, turnBoards } = useGrantSwitches();
  const title = "Agents' grants";
  const lede = `What the agents on ${machine.name} may do on Arugula cloud, as you. Off until you turn one on, and never more than you can read or do.`;
  if (grants === null)
    return (
      <Section id="grants" title={title} summary={err ? "Couldn't load" : undefined} lede={lede}>
        {err ? <p class="control-error">{err}</p> : <p class="dim">Loading…</p>}
      </Section>
    );
  const dms: AgentScope = { kind: "account", name: "Your DMs" };
  const scopes = session.projects.map(
    (p): AgentScope => ({ kind: "project", id: p.id, name: p.team ? p.name : "your personal project", role: roleIn(p) as Role }),
  );
  // One machine's row of each scope's matrix: its cells.
  const cellsOf = (s: AgentScope, columns: Column[]) => matrixOf(grants, [machine], s, columns, s.role)[0].cells;
  const cell = (s: AgentScope, column: Column, c: Cell, label: string) => {
    const k = cellKey(machine.id, s, column);
    const head = HEADS[column];
    const name = `${machine.name}: ${s.kind === "account" ? "your DMs" : `${label}: ${head.name}`}`;
    if (column === "boards")
      return (
        <td key={k} data-col={head.head}>
          <BoardsLevelSelect level={boardsLevel(grants, machine.id, s.id ?? "")} label={name} k={k} busy={busy === k} role={s.role} turn={(to) => void turnBoards(machine.id, s, to)} />
          <Says cell={{ limited: boardsLimit(grants, machine.id, s.id ?? ""), revoked: boardsRevoked(grants, machine.id, s) }} k={k} />
        </td>
      );
    return (
      <td key={k} data-col={head.short ?? head.head}>
        <Switch cell={c} k={k} label={name} busy={busy === k} refused={columnRefused(column, s.role) ? REFUSED_WHY : undefined} turn={(on) => turnCell(machine.id, s, column, on)} />
      </td>
    );
  };
  return (
    <Section
      id="grants"
      title={title}
      summary={machineGrantsSummary(grants, machine.id)}
      lede={lede}
      hooks={{ "data-agent-grants": machine.id }}
      more={
        <>
          <p>
            What they write is labelled as your agent's on {machine.name}, and every room they reach says so to everyone in it. A run or an
            agent session one of them starts is yours: you're its asker and you pay for it.
          </p>
          <p>
            They never approve anything: an owner approves a deploy with a passkey, and a session's permission waits on a person. A switch
            marked "only …" holds part of what it gives (from <code>arugula grants add</code>, say): turning it off takes that away, and on
            again gives all of it. In a project you only view, a switch gives what still works for a viewer's agents and says what it left
            out. A project's owners can revoke any grant over it: it shows here as revoked, by them and when, and you can give it again.
          </p>
        </>
      }
    >
      <table class="agent-matrix agent-matrix-wide machine-grants">
        <thead>
          <tr>
            <th scope="col">Project</th>
            {COLUMNS.map((c) => (
              <th scope="col" key={c} title={`Its agents ${HEADS[c].gives}`}>
                {HEADS[c].head}
              </th>
            ))}
          </tr>
        </thead>
        <tbody>
          {scopes.map((s, i) => {
            const p = session.projects[i];
            return (
              <tr key={s.id} data-agent-project={s.id}>
                <th scope="row">
                  {p.name}
                  {p.team?.role === "viewer" && <span class="agent-idle">you only view it</span>}
                </th>
                {cellsOf(s, COLUMNS).map((c, j) => cell(s, COLUMNS[j], c, p.name))}
              </tr>
            );
          })}
          <tr data-agent-dms="">
            <th scope="row" title="Read and post in your direct messages">
              Your DMs
            </th>
            {cell(dms, "threads", cellsOf(dms, ["threads"])[0], "Your DMs")}
            {COLUMNS.slice(1).map((c) => (
              <td key={c} data-col={HEADS[c].short ?? HEADS[c].head} aria-hidden="true" />
            ))}
          </tr>
        </tbody>
      </table>
      {note ? (
        <p class="agent-grants-note" data-agent-grants-note>
          {note}
        </p>
      ) : null}
      {err ? <p class="control-error">{err}</p> : null}
    </Section>
  );
}
