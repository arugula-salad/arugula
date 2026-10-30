// Agents on your machines, in the account's settings: per project, a
// matrix of the machines and seven columns each. Threads & issues (M197)
// lets a machine's agents read and post in the project's threads and
// write its issues; Runs (M216) lets them read its runs and start its run
// specs; Commands (M216, apart) start commands of their own there; Deploys
// (M218) plan its deploys and ask for approval: never approve; Start
// sessions and Answer sessions (aru-121) start agent sessions as you, and
// answer or prompt them; and Boards (#293, a select of three levels, not a
// switch) lets them read its boards, or read them and write their rows.
// Your DMs get one more, one switch per machine (M197). Off until you turn
// one on, and only over what you read; what they write is labelled as your
// agent's, and every room they reach says so. The same grants as `arugula
// grants add` (`/api/grants`, the platform's). What each switch reads of
// the grants is ./grants's, tested.

import { useEffect, useState } from "preact/hooks";
import { platformFetch } from "../../shared/maintenance";
import { useSubscribe } from "../../shared/ui/hooks";
import { session } from "../session";
import { toneClass } from "../swarm";
import {
  boardsLevel,
  boardsMove,
  cellKey,
  COLUMNS,
  COMMAND_POWERS,
  liveOver,
  machinesOf,
  matrixOf,
  powersFor,
  reaching,
  RUN_POWERS,
  SINGLE,
  singleGrant,
  type AgentScope,
  type BoardsLevel,
  type Cell,
  type Column,
  type Grant,
} from "./grants";
import { Section } from "./section";

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
// head's tooltip and the key under the section's line.
type Head = { head: string; short?: string; name: string; gives: string };
const HEADS: Record<Column, Head> = {
  threads: { head: "Threads & issues", short: "Threads", name: "threads & issues", gives: "read and post in its threads (a team's channels), and write its issues" },
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
    gives: "create, comment on, close, reopen and edit the work items of its connected Plane project, as you; whether each waits for your approval is set per Plane connection. Plane is in Labs: this does nothing until boards are on for your account",
  },
  boards: {
    head: "Boards",
    name: "boards",
    gives: "read its boards (read), or read them and set their notes and statuses and move rows (read and write). Boards are in Labs: this does nothing until boards are on for your account",
  },
};

const LEVEL_WORDS: Record<BoardsLevel, string> = { off: "Off", read: "Read", write: "Read and write" };

/** The boards column's control: its level, and a choice of any of the
 * three, which moves straight there. */
function BoardsLevelSelect({ level, label, k, busy, turn }: { level: BoardsLevel; label: string; k: string; busy: boolean; turn: (to: BoardsLevel) => void }) {
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
      {(Object.keys(LEVEL_WORDS) as BoardsLevel[]).map((l) => (
        <option key={l} value={l}>
          {LEVEL_WORDS[l]}
        </option>
      ))}
    </select>
  );
}

/** A switch in a matrix: on, off, or partly on (some of what it gives,
 * said in its title and under it), and busy while it's being turned. */
function Switch({ cell, label, k, busy, turn }: { cell: Cell; label: string; k: string; busy: boolean; turn: (on: boolean) => void }) {
  const only = cell.only ? `only ${cell.only.join(", ")}` : "";
  return (
    <>
      <input
        type="checkbox"
        role="switch"
        class={`sw-switch${cell.only ? " partial" : ""}`}
        data-agent-cell={k}
        data-partial={cell.only ? "" : undefined}
        aria-label={label}
        aria-busy={busy || undefined}
        title={only || undefined}
        checked={cell.on}
        disabled={busy}
        onChange={(e) => turn((e.target as HTMLInputElement).checked)}
      />
      {only && <span class="agent-only">{only}</span>}
    </>
  );
}

export function AgentGrants() {
  useSubscribe((fn) => session.subscribe(fn));
  const [grants, setGrants] = useState<Grant[] | null>(null);
  const [err, setErr] = useState("");
  const [busy, setBusy] = useState("");
  const load = () =>
    call<{ grants: Grant[] }>("/api/grants").then(
      (v) => setGrants(v.grants),
      (e: Error) => setErr(e.message),
    );
  useEffect(() => void load(), []);
  if (grants === null) return err ? <p class="control-error">{err}</p> : null;
  const machines = machinesOf(grants);
  // A column that's one grant: deploys, start and reply.
  const turnSingle = async (machine: string, s: AgentScope, column: Column, power: string, on: boolean) => {
    const project = s.id!;
    setErr("");
    setBusy(cellKey(machine, s, column));
    try {
      const g = singleGrant(grants, machine, project, power);
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
      const { give, revoke } = boardsMove(grants, machine, project, to);
      for (const power of give) await call("/api/grants", { daemon: machine, power, scope: { kind: "project", id: project } });
      for (const g of revoke) await call(`/api/grants/${encodeURIComponent(g.id)}/revoke`, {});
    } catch (e) {
      setErr((e as Error).message);
    }
    await load();
    setBusy("");
  };
  const turn = async (machine: string, s: AgentScope, on: boolean, which?: "runs" | "commands") => {
    setErr("");
    setBusy(cellKey(machine, s, which ?? "threads"));
    const powers = which === "runs" ? RUN_POWERS : which === "commands" ? COMMAND_POWERS : powersFor(s.kind);
    try {
      if (on) {
        const scope = s.kind === "project" ? { kind: "project", id: s.id } : { kind: "account" };
        for (const power of powers) await call("/api/grants", { daemon: machine, power, scope });
      } else {
        for (const g of liveOver(grants, machine, s, which ? powers : undefined)) {
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
    const single = SINGLE[column];
    if (single) return void turnSingle(machine, s, column, single, on);
    void turn(machine, s, on, column === "threads" ? undefined : column === "runs" ? "runs" : "commands");
  };

  // A scope's card: its name, how many machines reach it (0/5 too, UI
  // audit #168), and the matrix. The bar says whether any machine's agents
  // reach it at all.
  const card = (s: AgentScope, title: string, sub: string, columns: Column[], heads: Head[], data: object) => {
    const rows = matrixOf(grants, machines, s, columns);
    const n = reaching(rows);
    return (
      <div key={s.id ?? "dms"} class={`sw-card agent-card ${toneClass(n ? "accent" : "muted")}`} {...data}>
        <div class="sw-head">
          <b class="sw-name">{title}</b>
          <span class="sw-n" title={`Agents on ${n} of ${machines.length} ${machines.length === 1 ? "machine" : "machines"} reach it`}>
            {n}/{machines.length}
          </span>
        </div>
        <span class="dim agent-card-sub">{sub}</span>
        <table class={`agent-matrix${columns.length > 1 ? " agent-matrix-wide" : ""}`}>
          <thead>
            <tr>
              <th scope="col">Machine</th>
              {heads.map((h) => (
                <th scope="col" key={h.head} title={`Its agents ${h.gives}`}>
                  {h.head}
                </th>
              ))}
            </tr>
          </thead>
          <tbody>
            {rows.map(({ machine: m, cells }) => (
              <tr key={m.id}>
                <th scope="row">{m.name}</th>
                {cells.map((cell, i) => {
                  const k = cellKey(m.id, s, columns[i]);
                  const name = `${m.name}: ${s.kind === "account" ? "your DMs" : `${s.name}: ${heads[i].name}`}`;
                  if (columns[i] === "boards") {
                    return (
                      <td key={k} data-col={heads[i].head}>
                        <BoardsLevelSelect
                          level={boardsLevel(grants, m.id, s.id ?? "")}
                          label={name}
                          k={k}
                          busy={busy === k}
                          turn={(to) => void turnBoards(m.id, s, to)}
                        />
                      </td>
                    );
                  }
                  return (
                    <td key={k} data-col={heads[i].short ?? heads[i].head}>
                      <Switch
                        cell={cell}
                        k={k}
                        label={name}
                        busy={busy === k}
                        turn={(on) => turnCell(m.id, s, columns[i], on)}
                      />
                    </td>
                  );
                })}
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    );
  };

  return (
    <Section
      id="agents"
      title="Agents on your machines"
      lede="What the agents on each of your machines may do in each project. Every switch is off until you turn it on."
      more={
        <>
          <p>
            Never more than you can read or do. What they write is labelled as your agent's on that machine, and every room they reach
            says so to everyone in it. A run they start is yours, labelled as started by your agent on that machine.
          </p>
          <p>
            An agent session one of them starts is yours: you're its asker and you pay for it, and its card says your agent on that
            machine started it.
          </p>
          <p>
            They never approve anything: an owner approves a deploy, on their phone or in a browser, with a passkey, and a session's
            permission waits on a person. A switch marked
            "only …" holds part of what it gives (from <code>arugula grants add</code>, say): turning it off takes that away, and on
            again gives all of it.
          </p>
        </>
      }
    >
      <dl class="agent-key">
        {COLUMNS.map((c) => (
          <div key={c}>
            <dt>{HEADS[c].head}</dt>
            <dd>{HEADS[c].gives}</dd>
          </div>
        ))}
      </dl>
      {machines.length === 0 ? (
        <p class="dim">No machines on your account yet.</p>
      ) : (
        <div class="sw-cards agent-cards" data-agent-grants>
          {session.projects.map((p) => {
            const s: AgentScope = { kind: "project", id: p.id, name: p.team ? p.name : "your personal project" };
            return card(
              s,
              p.name,
              p.team ? `${p.team.name}'s channels and issues` : "Your personal project",
              COLUMNS,
              COLUMNS.map((c) => HEADS[c]),
              { "data-agent-project": p.id },
            );
          })}
          {card(
            { kind: "account", name: "Your DMs" },
            "Your DMs",
            "Read and post in your direct messages",
            ["threads"],
            [{ head: "Read & post", name: "your DMs", gives: "read and post in your DMs" }],
            { "data-agent-dms": "" },
          )}
        </div>
      )}
      {err ? <p class="control-error">{err}</p> : null}
    </Section>
  );
}
