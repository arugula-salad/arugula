// `/p/:project/runs` and `/p/:project/runs/:run` (M215): a project's runs
// on its hosted workspaces (step 7), and one run.
//
// The list narrows by state, by what started it (its Trigger: by hand, a
// deploy, a schedule, an event) and by run spec, newest first (M216's
// reads). Owners and editors on the hosted list start a run (a spec, or a
// command) and keep the project's run specs, with their schedules and
// event rules (M214's routes): a list, add and remove.
//
// A run's view: who or what started it, the exact command its S37
// operation runs and the operation's hash, its pane while it runs (on
// control's page, where its machine is: the platform page has no machines,
// "aru-055: the platform doesn't read panes"), its durable log afterwards
// (M213's, a page at a time from its tail), its artifacts, its timeline
// (M210's events, each said by its machine or by Arugula), and its thread
// (M196's object room on the run's id). While it goes, the view waits on
// the platform for its next event (`since` and `wait`).

import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "preact/hooks";
import { chat } from "../../shared/chat-store";
import { RoomBody } from "../../shared/chat/room";
import type { ChatRoom } from "../../shared/proto";
import type { RunEventView } from "../../shared/proto.gen";
import { usePhone, useSubscribe } from "../../shared/ui/hooks";
import { ConfirmRemove } from "../../shared/ui/confirm";
import { openMenu, type MenuItem } from "../../shared/ui/menu";
import { Markdown } from "../../shared/ui/markdown";
import { PageHeader } from "../../shared/ui/page-header";
import { StateChip } from "../../shared/ui/state-chip";
import { Time } from "../../shared/ui/time";
import "../../shared/issues/issues.css";
import { names } from "../links";
import { follow, go, pathOf } from "../router";
import { controlOrigin, session, type PlatformProject } from "../session";
import { toast } from "../toast";
import { ListLoading, mayWrite, nameIn } from "./issues";
import { Missing } from "./project";
import { NewWorkspaceButton } from "./hosted-workspace";
import { runs, type HostedWorkspace, type RunArtifacts, type RunFilter, type RunLogPage, type RunSpec, type RunView } from "../runs";
import { runTone, toneClass, type Tone } from "../swarm";
import { STATES, TRIGGERS, argvOf, commandLine, decodeOp, deployTitle, httpsOnly, ended, eventDetail, keepLatest, paneLink, runTitle, shortId, startedBy, stripAnsi, tone, took, triggerLine, withoutExitNote, workspaceLabel } from "../runs-text";
import { ThreadUnread } from "../thread-unread";

const menuAt = (e: MouseEvent, items: MenuItem[]) => {
  const at = (e.currentTarget as HTMLElement).getBoundingClientRect();
  openMenu({ clientX: at.left, clientY: at.bottom + 4, preventDefault: () => e.preventDefault() }, items);
};

const short = (id: string) => id.replace(/^run_/, "").slice(0, 8);

/** Whether the reader starts runs and keeps specs here: an owner or an
 * editor, on the hosted list. */
const starter = (p: PlatformProject) => mayWrite(p) && session.hosted;

function useProject(id: string): PlatformProject | null {
  useSubscribe((fn) => session.subscribe(fn));
  return session.project(id) ?? null;
}

/** The project's run specs, and a way to read them again. */
function useSpecs(project: string): [RunSpec[] | null, () => void] {
  const [specs, setSpecs] = useState<RunSpec[] | null>(null);
  const [tick, setTick] = useState(0);
  useEffect(() => {
    let live = true;
    runs.specs(project).then(
      (s) => live && setSpecs(s),
      () => live && setSpecs([]),
    );
    return () => void (live = false);
  }, [project, tick]);
  return [specs, () => setTick((t) => t + 1)];
}

function useWorkspaces(project: string): HostedWorkspace[] | null {
  const [ws, setWs] = useState<HostedWorkspace[] | null>(null);
  useEffect(() => {
    let live = true;
    runs.workspaces(project).then((w) => live && setWs(w));
    return () => void (live = false);
  }, [project]);
  return ws;
}

/** What the list holds, as the swarm's bar counts its panes: the runs
 * (the newest 50 that match), and of them the waiting, the going, the
 * succeeded and the failed (UI audit #164: `29 runs · 25 succeeded`). */
function RunStats({ list }: { list: RunView[] }) {
  const n = (t: Tone) => list.filter((r) => runTone(r.state) === t).length;
  const stats: [number, string][] = [
    [n("warn"), "waiting"],
    [n("accent"), "running"],
    [list.filter((r) => r.state === "succeeded").length, "succeeded"],
    [n("danger"), "failed"],
  ];
  return (
    <div class="sw-stats" data-runs-stats>
      <span>
        <b>{list.length}</b> {list.length === 1 ? "run" : "runs"}
      </span>
      {stats.map(([k, what]) =>
        k ? (
          <span key={what} class={what === "failed" ? "bad" : undefined}>
            <b>{k}</b> {what}
          </span>
        ) : null,
      )}
    </div>
  );
}

export function RunsPage({ project, filter }: { project: string; filter: RunFilter }) {
  const p = useProject(project);
  if (!p) return <Missing what="That project isn't one of yours (or it's gone)." />;
  return <RunList p={p} filter={filter} />;
}

function RunList({ p, filter }: { p: PlatformProject; filter: RunFilter }) {
  const [list, setList] = useState<RunView[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  // The project home's Start a run comes here with `#new-run`.
  const [starting, setStarting] = useState(() => starter(p) && location.hash === "#new-run");
  const [addingSpec, setAddingSpec] = useState(false);
  const [specs, rereadSpecs] = useSpecs(p.id);
  const listed = useWorkspaces(p.id);
  const workspaces = listed ?? [];
  const nameOf = nameIn(p);
  const specName = (id: unknown) => (typeof id === "string" ? specs?.find((s) => s.id === id)?.name : undefined);
  const key = JSON.stringify([p.id, filter.state, filter.trigger, filter.spec]);
  const [tick, setTick] = useState(0);
  useEffect(() => {
    let live = true;
    setError(null);
    runs.list(p.id, filter).then(
      (r) => live && setList(r.runs),
      (e: Error) => live && setError(e.message),
    );
    return () => void (live = false);
  }, [key, tick]);
  // While any of them goes, read the list again now and then.
  const going = !!list?.some((r) => !ended(r.state));
  useEffect(() => {
    if (!going) return;
    const t = setInterval(() => document.visibilityState === "visible" && setTick((n) => n + 1), 10_000);
    return () => clearInterval(t);
  }, [going]);

  const to = (f: RunFilter) => go(pathOf({ page: "runs", project: p.id, ...f }, names));
  const stateMenu = (e: MouseEvent) =>
    menuAt(e, [
      { header: "State" },
      { label: "Any state", checked: !filter.state, run: () => to({ ...filter, state: undefined }) },
      ...Object.entries(STATES).map(([k, v]) => ({ label: v, checked: filter.state === k, run: () => to({ ...filter, state: k }) })),
    ]);
  const triggerMenu = (e: MouseEvent) =>
    menuAt(e, [
      { header: "Trigger" },
      { label: "Any trigger", checked: !filter.trigger, run: () => to({ ...filter, trigger: undefined }) },
      ...Object.entries(TRIGGERS).map(([k, v]) => ({ label: v, checked: filter.trigger === k, run: () => to({ ...filter, trigger: k }) })),
    ]);
  const specMenu = (e: MouseEvent) =>
    menuAt(e, [
      { header: "Run spec" },
      { label: "Any spec, or none", checked: !filter.spec, run: () => to({ ...filter, spec: undefined }) },
      ...(specs ?? []).map((s) => ({ label: s.name, checked: filter.spec === s.id, run: () => to({ ...filter, spec: s.id }) })),
    ]);
  const narrowed = !!(filter.state || filter.trigger || filter.spec);
  const settings = pathOf({ page: "project-settings", project: p.id }, names);

  return (
    <section class="platform-section sw-page runs-page" data-runs={p.id}>
      <PageHeader
        title="Runs"
        actions={
          // One way to add, as the issues' New issue (UI audit #164).
          starter(p) && (
            <>
              <button class="secondary" data-run-spec-new aria-expanded={addingSpec} onClick={() => setAddingSpec(!addingSpec)}>
                New run spec
              </button>
              <button class="primary" data-run-start-open aria-expanded={starting} onClick={() => setStarting(!starting)}>
                New run
              </button>
            </>
          )
        }
        sub={
          <>
            Runs go on this project's cloud workspaces: by hand, on a schedule, or on an event from a linked repo. Scheduled and event runs start
            under the project's standing grant, in its{" "}
            <a href={settings} onClick={(e) => follow(e, settings)}>
              settings
            </a>
            .
          </>
        }
        stats={!!list?.length && <RunStats list={list} />}
      />
      {starting && <StartRun p={p} specs={specs ?? []} workspaces={workspaces} done={() => setStarting(false)} />}
      <div class="issues-tools">
        <button class={filter.state ? "issues-filter on" : "issues-filter"} data-runs-state onClick={stateMenu}>
          {filter.state ? `State: ${STATES[filter.state] ?? filter.state}` : "State"} <span class="caret">▾</span>
        </button>
        <button class={filter.trigger ? "issues-filter on" : "issues-filter"} data-runs-trigger onClick={triggerMenu}>
          {filter.trigger ? `Trigger: ${TRIGGERS[filter.trigger] ?? filter.trigger}` : "Trigger"} <span class="caret">▾</span>
        </button>
        <button class={filter.spec ? "issues-filter on" : "issues-filter"} data-runs-spec onClick={specMenu}>
          {filter.spec ? `Spec: ${specName(filter.spec) ?? "one"}` : "Spec"} <span class="caret">▾</span>
        </button>
        {narrowed && (
          <button class="link" data-runs-clear onClick={() => to({})}>
            Clear
          </button>
        )}
      </div>
      {error && <p class="control-error" role="alert">{error}</p>}
      {list === null && !error && <ListLoading />}
      {list?.length === 0 &&
        (narrowed ? (
          <p class="dim issues-none" data-runs-none>
            No runs match.
          </p>
        ) : (
          <NoRuns p={p} workspaces={listed} starting={starting} start={() => setStarting(true)} />
        ))}
      <ul class="issues-list sw-cards" data-runs-list>
        {list?.map((r) => {
          const path = pathOf({ page: "run", project: p.id, run: r.id }, names);
          const end = r.ended_at ?? (ended(r.state) ? null : Date.now());
          // By name, not its `hw_…` id (UI audit #145).
          const ws = r.workspace ? (workspaces.find((w) => w.id === r.workspace)?.name ?? workspaceLabel(r)) : null;
          return (
            // A card in the swarm's language (../swarm.css): its bar is the
            // run's state, which the meta line says in words.
            <li class={`issue-row run-row sw-card ${toneClass(runTone(r.state))}`} key={r.id} data-run-row={r.id} data-run-state={r.state}>
              <div class="issue-row-main">
                <a href={path} class="issue-row-title run-title" onClick={(e) => follow(e, path)}>
                  {runTitle(r.op, specName(r.source.spec), r.deploy)}
                </a>
                <ThreadUnread target={r.id} />
                <span class="issue-row-meta dim">
                  <StateChip tone={runTone(r.state)}>{STATES[r.state] ?? r.state}</StateChip> · {triggerLine(r, nameOf)} · <Time at={r.created_at} long />
                  {r.started_at && end ? ` · ${took(r.started_at, end)}` : ""}
                  {r.exit_code !== null && r.exit_code !== undefined ? ` · exit ${r.exit_code}` : ""}
                </span>
              </div>
              <code class="run-id sw-id" title={r.id}>
                {short(r.id)}
                {ws ? ` · ${ws}` : ""}
              </code>
            </li>
          );
        })}
      </ul>
      <Specs p={p} specs={specs} workspaces={workspaces} reread={rereadSpecs} adding={addingSpec} setAdding={setAddingSpec} />
    </section>
  );
}

/** Why a reader can't start runs here, in one line; null when they can. */
export function whyNoStart(p: PlatformProject): string | null {
  if (!mayWrite(p)) return "A team's owners and editors start its runs; you can read them here.";
  if (!session.hosted) return "Runs go on cloud workspaces, which are in a limited beta your account isn't in yet.";
  return null;
}

/** No runs yet: the next step for someone who can start one (a cloud
 * workspace first, where there's none to run on), and why not, in one
 * line, for anyone else. */
function NoRuns({ p, workspaces, starting, start }: { p: PlatformProject; workspaces: HostedWorkspace[] | null; starting: boolean; start: () => void }) {
  const why = whyNoStart(p);
  if (why || workspaces === null)
    return (
      <p class="dim issues-none" data-runs-none data-runs-why={why ? "" : undefined}>
        No runs yet.{why ? ` ${why}` : ""}
      </p>
    );
  if (!workspaces.length)
    return (
      <div class="issues-none platform-empty" data-runs-none>
        <p>No runs yet. A run is a command on one of the project's cloud workspaces, and it has none yet: make one first.</p>
        <NewWorkspaceButton project={p.id} />
      </div>
    );
  return (
    <div class="issues-none platform-empty" data-runs-none>
      <p>No runs yet. Start one by hand (a command, or a run spec), or give a run spec a schedule.</p>
      {!starting && (
        <button class="primary" data-runs-none-start onClick={start}>
          Start a run
        </button>
      )}
    </div>
  );
}

/** Start a run by hand: a spec, or a command on a hosted workspace. */
function StartRun({ p, specs, workspaces, done }: { p: PlatformProject; specs: RunSpec[]; workspaces: HostedWorkspace[]; done: () => void }) {
  const [what, setWhat] = useState<"spec" | "command">(specs.length ? "spec" : "command");
  const [spec, setSpec] = useState(specs[0]?.id ?? "");
  const [command, setCommand] = useState("");
  const [workspace, setWorkspace] = useState(workspaces[0]?.id ?? "");
  const [busy, setBusy] = useState(false);
  const start = async (e: Event) => {
    e.preventDefault();
    const argv = argvOf(command.trim());
    if (what === "command" && (!argv || argv.length === 0)) return toast(argv ? "Type a command" : "A quote isn't closed");
    setBusy(true);
    try {
      const body = what === "spec" ? { spec } : { argv: argv!, ...(workspace ? { workspace } : {}) };
      const r = await runs.start(p.id, body);
      done();
      go(pathOf({ page: "run", project: p.id, run: r.id }, names));
    } catch (err) {
      toast(`It didn't start: ${(err as Error).message}`);
    } finally {
      setBusy(false);
    }
  };
  return (
    <form class="run-start" data-run-start onSubmit={(e) => void start(e)}>
      {specs.length > 0 && (
        <div class="issues-states" role="group" aria-label="What to run">
          <button type="button" aria-pressed={what === "spec"} data-run-start-spec-mode onClick={() => setWhat("spec")}>
            A run spec
          </button>
          <button type="button" aria-pressed={what === "command"} data-run-start-command-mode onClick={() => setWhat("command")}>
            A command
          </button>
        </div>
      )}
      {what === "spec" ? (
        <select aria-label="Run spec" value={spec} data-run-start-spec onChange={(e) => setSpec(e.currentTarget.value)}>
          {specs.map((s) => (
            <option key={s.id} value={s.id}>
              {s.name}
            </option>
          ))}
        </select>
      ) : (
        <>
          <input class="run-command" placeholder="make test" aria-label="Command" value={command} data-run-start-command onInput={(e) => setCommand(e.currentTarget.value)} />
          {workspaces.length > 1 && (
            <select aria-label="Cloud workspace" value={workspace} data-run-start-workspace onChange={(e) => setWorkspace(e.currentTarget.value)}>
              {workspaces.map((w) => (
                <option key={w.id} value={w.id}>
                  {w.name}
                </option>
              ))}
            </select>
          )}
        </>
      )}
      <button class="primary" type="submit" disabled={busy} data-run-start-go>
        Start
      </button>
    </form>
  );
}

/** The project's run specs, each with its schedules and rules: a list,
 * and (owners and editors) add and remove. */
function Specs({
  p,
  specs,
  workspaces,
  reread,
  adding,
  setAdding,
}: {
  p: PlatformProject;
  specs: RunSpec[] | null;
  workspaces: HostedWorkspace[];
  reread: () => void;
  /** New run spec's form is open (the header's button, UI audit #164). */
  adding: boolean;
  setAdding: (on: boolean) => void;
}) {
  const can = starter(p);
  // Delete and Remove ask first, in the page's own dialog (UI audit #158).
  const [removing, setRemoving] = useState<{ title: string; label: string; what: string; body: string; go: () => Promise<unknown> } | null>(null);
  const act = async (what: string, f: () => Promise<unknown>) => {
    try {
      await f();
      reread();
    } catch (e) {
      toast(`Couldn't ${what}: ${(e as Error).message}`);
    }
  };
  const wsName = (id: string) => workspaces.find((w) => w.id === id)?.name ?? id;
  if (specs === null) return null;
  if (specs.length === 0 && !can) return null;
  return (
    <div class="run-specs" data-run-specs>
      <h2>
        Run specs <span class="dim">{specs.length}</span>
      </h2>
      {adding && <NewSpec p={p} workspaces={workspaces} done={() => (setAdding(false), reread())} />}
      {specs.length === 0 && <p class="dim">None yet. A spec names a command or an instruction and the workspace it runs on; give it a schedule or an event rule.</p>}
      <ul class="platform-list run-spec-list">
        {specs.map((s) => {
          const filter = pathOf({ page: "runs", project: p.id, spec: s.id }, names);
          return (
            <li key={s.id} data-run-spec={s.name}>
              <div class="run-spec-head">
                <a href={filter} class="run-spec-name" onClick={(e) => follow(e, filter)}>
                  <b>{s.name}</b>
                </a>{" "}
                <span class="dim">on {wsName(s.workspace)}</span>
                {can && (
                  <span class="run-spec-actions">
                    <button class="secondary small" data-run-spec-run={s.name} onClick={() => void act("start it", async () => go(pathOf({ page: "run", project: p.id, run: (await runs.start(p.id, { spec: s.id })).id }, names)))}>
                      Run now
                    </button>
                    <button
                      class="secondary danger small"
                      data-run-spec-delete={s.name}
                      onClick={() =>
                        setRemoving({
                          title: `Delete the run spec ${s.name}?`,
                          label: "Delete",
                          what: "delete it",
                          body: "Its schedules and event rules go with it. Runs it started stay in the list.",
                          go: () => runs.deleteSpec(p.id, s.id),
                        })
                      }
                    >
                      Delete
                    </button>
                  </span>
                )}
              </div>
              <SpecWhat s={s} />
              <ul class="run-triggers">
                {s.schedules.map((x) => (
                  <li key={x.id} data-run-schedule={x.id} class={x.enabled ? "" : "dim"}>
                    ⏱ <code>{x.cron}</code> {x.timezone}
                    {x.enabled ? (
                      <>
                        {" "}
                        · next <Time at={x.next_at} long title="Next" />
                      </>
                    ) : (
                      " · off"
                    )}
                    {x.last_error ? <span class="control-error"> · {x.last_error}</span> : null}
                    {can && (
                      <>
                        {" "}
                        <button class="secondary small" onClick={() => void act("change it", () => runs.setSchedule(p.id, s.id, x.id, !x.enabled))}>
                          {x.enabled ? "Turn off" : "Turn on"}
                        </button>{" "}
                        <button
                          class="secondary danger small"
                          data-run-schedule-delete={x.id}
                          onClick={() =>
                            setRemoving({
                              title: `Remove the schedule ${x.cron}?`,
                              label: "Remove",
                              what: "remove it",
                              body: `${s.name} stops running on it. To run on it again, add it again.`,
                              go: () => runs.deleteSchedule(p.id, s.id, x.id),
                            })
                          }
                        >
                          Remove
                        </button>
                      </>
                    )}
                  </li>
                ))}
                {s.rules.map((x) => (
                  <li key={x.id} data-run-rule={x.id} class={x.enabled ? "" : "dim"}>
                    ⚡ {x.event.replace(/_/g, " ")} on {x.repo ?? "any linked repo"}
                    {x.branch ? ` (${x.branch})` : ""} · at most {x.max_per_hour} an hour{x.outsiders ? " · outsiders too" : ""}
                    {x.enabled ? "" : " · off"}
                    {can && (
                      <>
                        {" "}
                        <button class="secondary small" onClick={() => void act("change it", () => runs.setRule(p.id, s.id, x.id, !x.enabled))}>
                          {x.enabled ? "Turn off" : "Turn on"}
                        </button>{" "}
                        <button
                          class="secondary danger small"
                          data-run-rule-delete={x.id}
                          onClick={() =>
                            setRemoving({
                              title: "Remove this event rule?",
                              label: "Remove",
                              what: "remove it",
                              body: `${s.name} stops running on ${x.event.replace(/_/g, " ")}s. To run on them again, add it again.`,
                              go: () => runs.deleteRule(p.id, s.id, x.id),
                            })
                          }
                        >
                          Remove
                        </button>
                      </>
                    )}
                  </li>
                ))}
              </ul>
              {can && <AddTrigger p={p} spec={s} act={act} />}
            </li>
          );
        })}
      </ul>
      {removing && (
        <ConfirmRemove
          title={removing.title}
          label={removing.label}
          cancel={() => setRemoving(null)}
          go={() => {
            const r = removing;
            setRemoving(null);
            void act(r.what, r.go);
          }}
        >
          <p>{removing.body}</p>
        </ConfirmRemove>
      )}
    </div>
  );
}

/** What a spec runs, under its name (UI audit #164): a command on one
 * line, or its instruction's Markdown, a line of it until Show. */
function SpecWhat({ s }: { s: RunSpec }) {
  const [open, setOpen] = useState(false);
  if (s.instruction === undefined || s.instruction === null) return <code class="run-spec-op">{commandLine(s.argv ?? [])}</code>;
  const long = s.instruction.trim().includes("\n") || s.instruction.length > 120;
  return (
    <div class={`run-spec-instruction${open ? " open" : ""}`} data-run-spec-instruction>
      <div class="run-spec-md">
        <Markdown text={s.instruction} refs={null} />
      </div>
      {long && (
        <button class="link" aria-expanded={open} data-run-spec-show onClick={() => setOpen(!open)}>
          {open ? "Show less" : "Show"}
        </button>
      )}
    </div>
  );
}

function NewSpec({ p, workspaces, done }: { p: PlatformProject; workspaces: HostedWorkspace[]; done: () => void }) {
  const [name, setName] = useState("");
  const [command, setCommand] = useState("");
  const [workspace, setWorkspace] = useState(workspaces[0]?.id ?? "");
  const make = async (e: Event) => {
    e.preventDefault();
    const argv = argvOf(command.trim());
    if (!name.trim() || !argv?.length) return toast("A name and a command");
    if (!workspace) return toast("This project has no cloud workspace to run on");
    try {
      await runs.addSpec(p.id, { name: name.trim(), workspace, argv });
      done();
    } catch (err) {
      toast(`Couldn't make it: ${(err as Error).message}`);
    }
  };
  return (
    <form class="run-start" data-run-spec-form onSubmit={(e) => void make(e)}>
      <input placeholder="nightly" aria-label="Name" value={name} data-run-spec-name onInput={(e) => setName(e.currentTarget.value)} />
      <input class="run-command" placeholder="make test" aria-label="Command" value={command} data-run-spec-command onInput={(e) => setCommand(e.currentTarget.value)} />
      {workspaces.length > 1 && (
        <select aria-label="Cloud workspace" value={workspace} onChange={(e) => setWorkspace(e.currentTarget.value)}>
          {workspaces.map((w) => (
            <option key={w.id} value={w.id}>
              {w.name}
            </option>
          ))}
        </select>
      )}
      <button class="primary" type="submit">
        Add
      </button>
    </form>
  );
}

/** Add a schedule (cron, in a time zone) or an event rule to a spec: a
 * rule's repo, branch, cap an hour and (an owner's call) outsiders, as
 * `arugula runs rule add` takes them. */
function AddTrigger({ p, spec, act }: { p: PlatformProject; spec: RunSpec; act: (what: string, f: () => Promise<unknown>) => Promise<void> }) {
  const [open, setOpen] = useState<null | "schedule" | "rule">(null);
  const [cron, setCron] = useState("0 3 * * *");
  const [tz, setTz] = useState(() => Intl.DateTimeFormat().resolvedOptions().timeZone || "UTC");
  const [event, setEvent] = useState<"pr_opened" | "check_failed" | "push">("push");
  const [repo, setRepo] = useState("");
  const [branch, setBranch] = useState("");
  const [cap, setCap] = useState("");
  const [outsiders, setOutsiders] = useState(false);
  // Outsiders let anyone who can open a PR start it: the platform takes
  // that from a project's owners only.
  const owner = !p.team || p.team.role === "owner";
  if (!open) {
    return (
      <p class="run-add-trigger">
        <button class="secondary small" data-run-schedule-add={spec.name} onClick={() => setOpen("schedule")}>
          + Schedule
        </button>{" "}
        <button class="secondary small" data-run-rule-add={spec.name} onClick={() => setOpen("rule")}>
          + Event rule
        </button>
      </p>
    );
  }
  const add = (e: Event) => {
    e.preventDefault();
    const f =
      open === "schedule"
        ? () => runs.addSchedule(p.id, spec.id, { cron: cron.trim(), timezone: tz.trim() || undefined })
        : () =>
            runs.addRule(p.id, spec.id, {
              event,
              ...(repo.trim() ? { repo: repo.trim() } : {}),
              ...(branch.trim() ? { branch: branch.trim() } : {}),
              ...(cap.trim() ? { max_per_hour: Number(cap) } : {}),
              ...(outsiders ? { outsiders } : {}),
            });
    void act("add it", f).then(() => setOpen(null));
  };
  return (
    <form class="run-start" onSubmit={add}>
      {open === "schedule" ? (
        <>
          <input aria-label="Cron" value={cron} data-run-schedule-cron onInput={(e) => setCron(e.currentTarget.value)} />
          <input aria-label="Time zone" value={tz} onInput={(e) => setTz(e.currentTarget.value)} />
        </>
      ) : (
        <>
          <select aria-label="Event" value={event} data-run-rule-event onChange={(e) => setEvent(e.currentTarget.value as typeof event)}>
            <option value="push">A push</option>
            <option value="pr_opened">A PR opened</option>
            <option value="check_failed">A check failed</option>
          </select>
          <input placeholder="owner/repo (any)" aria-label="Repo" value={repo} onInput={(e) => setRepo(e.currentTarget.value)} />
          <input placeholder="branch (any)" aria-label="Branch" value={branch} onInput={(e) => setBranch(e.currentTarget.value)} />
          <input
            type="number"
            min={1}
            max={1000}
            placeholder="20 an hour"
            aria-label="At most, an hour"
            title="The most runs it starts in an hour"
            value={cap}
            data-run-rule-cap
            onInput={(e) => setCap(e.currentTarget.value)}
          />
          {owner && (
            <label class="control-check" title="Forks and people who aren't the repo's owners, members or collaborators">
              <input type="checkbox" checked={outsiders} data-run-rule-outsiders onChange={(e) => setOutsiders(e.currentTarget.checked)} /> Outsiders too
            </label>
          )}
        </>
      )}
      <button class="primary" type="submit">
        Add
      </button>
      <button type="button" class="secondary" onClick={() => setOpen(null)}>
        Cancel
      </button>
    </form>
  );
}

// ---- One run

export function RunPage({ project, run, msg }: { project: string; run: string; msg?: number }) {
  const p = useProject(project);
  if (!p) return <Missing what="That project isn't one of yours (or it's gone)." />;
  return <RunDetail key={run} p={p} id={run} msg={msg} />;
}

/** The run, kept current: read now, then (while it goes) again each time
 * the platform says it moved. */
function useRun(id: string): { run: RunView | null; missing: string | null } {
  const [run, setRun] = useState<RunView | null>(() => {
    const k = runs.known(id);
    return k?.events ? k : null;
  });
  const [missing, setMissing] = useState<string | null>(null);
  useEffect(() => {
    let live = true;
    (async () => {
      let v: RunView | null = null;
      while (live) {
        try {
          v = await runs.get(id, v && !ended(v.state) ? v.last_seq : undefined);
          if (!live) return;
          setRun(v);
          if (ended(v.state)) return;
        } catch (e) {
          if (!live) return;
          if (!v) return setMissing((e as Error).message);
          // Unreachable for now: look again shortly.
          await new Promise((r) => setTimeout(r, 5_000));
        }
      }
    })();
    return () => void (live = false);
  }, [id]);
  return { run, missing };
}

function RunDetail({ p, id, msg }: { p: PlatformProject; id: string; msg?: number }) {
  const { run, missing } = useRun(id);
  const [specs] = useSpecs(p.id);
  const workspaces = useWorkspaces(p.id) ?? [];
  const nameOf = nameIn(p);
  const back = pathOf({ page: "runs", project: p.id }, names);
  if (missing && !run) return <Missing what={`That run isn't one you can read (${missing}).`} />;
  if (!run) return <p class="platform-section dim">Loading…</p>;
  if (run.project !== p.id) return <Missing what="That run is in another project." />;
  const op = decodeOp(run.op);
  const spec = typeof run.source.spec === "string" ? specs?.find((s) => s.id === run.source.spec) : undefined;
  const live = paneLink(controlOrigin(), run);
  const going = !ended(run.state);
  const by = startedBy(run, nameOf, true);
  // Why it ended without succeeding, under its state (UI audit #164): its
  // last event's detail, which the timeline has too.
  const last = run.events?.at(-1);
  const why = ended(run.state) && run.state !== "succeeded" && last?.to === run.state ? eventDetail(last.detail ?? {}) : "";
  const wsName = run.workspace_name || workspaces.find((w) => w.id === run.workspace)?.name;
  return (
    <section class="platform-section sw-page run-page" data-run={run.id} data-run-state={run.state}>
      <PageHeader
        title={<span data-run-title>{runTitle(run.op, spec?.name, run.deploy)}</span>}
        named
        back={{ href: back, label: "Runs", onClick: (e) => follow(e, back), data: "run-back" }}
        state={
          <StateChip tone={runTone(run.state)} data-run-state-label>
            {STATES[run.state] ?? run.state}
            {run.exit_code !== null && run.exit_code !== undefined ? ` · exit ${run.exit_code}` : ""}
          </StateChip>
        }
        sub={
          why && (
            <span class="run-why" data-run-why>
              {why}
            </span>
          )
        }
      />
      <dl class="run-facts">
        <dt>Trigger</dt>
        <dd data-run-started-by>
          {triggerLine(run, nameOf)}
          {spec ? (
            <>
              {" "}
              · from{" "}
              <a href={pathOf({ page: "runs", project: p.id, spec: spec.id }, names)} onClick={(e) => follow(e, pathOf({ page: "runs", project: p.id, spec: spec.id }, names))}>
                {spec.name}
              </a>
            </>
          ) : null}
          {" "}· <Time at={run.created_at} long />
          {run.started_at ? ` · ran ${took(run.started_at, run.ended_at ?? Date.now())}` : ""}
        </dd>
        <dt>{op?.deploy ? "Deploy" : op?.instruction !== null && op?.instruction !== undefined ? "Instruction" : "Command"}</dt>
        <dd>
          {op?.deploy ? (
            // A deploy's apply (M219): its deploy's page says what and
            // where (UI audit #145).
            <>
              <a href={`/deploys/${op.deploy.deploy}`} onClick={(e) => follow(e, `/deploys/${op.deploy!.deploy}`)} data-run-deploy={op.deploy.deploy}>
                {run.deploy ? deployTitle(run.deploy) : `Deploy ${op.deploy.deploy}`}
              </a>
              <span class="dim"> · driver {op.deploy.driver}</span>
            </>
          ) : op ? (
            <code class="run-command-line" data-run-command>
              {op.instruction ?? commandLine(op.argv)}
            </code>
          ) : (
            <span class="dim">Not an operation this page reads: its bytes are below.</span>
          )}
          {op?.agent ? <span class="dim"> · agent {op.agent}</span> : null}
          {op?.directory ? <span class="dim"> · in {op.directory}</span> : null}
        </dd>
        <dt>Operation</dt>
        <dd>
          <code class="run-hash" title="SHA-256 of the operation's bytes: what its grant or approval names" data-run-op-hash>
            {run.op_hash}
          </code>
          <details class="run-op">
            <summary>Its exact bytes</summary>
            <pre data-run-op>{run.op}</pre>
          </details>
        </dd>
        {run.chant_ledger ? (
          <>
            <dt>chant's ledger</dt>
            <dd data-run-chant-ledger>
              {httpsOnly(run.chant_ledger) ? (
                <a href={run.chant_ledger} target="_blank" rel="noopener noreferrer">
                  The release's entry ↗
                </a>
              ) : (
                <code>{run.chant_ledger}</code>
              )}
            </dd>
          </>
        ) : null}
        <dt>Where</dt>
        <dd>
          {/* By name, its id short with the whole one in a title (UI audit #164). */}
          {run.workspace ? (
            <>
              {run.deploy ? "Its deploy's own box" : wsName ? <b data-run-workspace>{wsName}</b> : "A cloud workspace"}{" "}
              <code class="dim" title={run.workspace}>
                {shortId(run.workspace)}
              </code>
            </>
          ) : (
            "No cloud workspace"
          )}
          <span class="dim">
            {" "}
            · machine{" "}
            <code title={run.daemon}>{shortId(run.daemon)}</code>
          </span>
          {run.grant ? (
            <span class="dim"> · under the project's standing grant</span>
          ) : (
            <span class="dim"> · no standing grant: {going ? "it waits for an approval" : "it needed an approval"}</span>
          )}
        </dd>
      </dl>
      <h2>Live</h2>
      <p data-run-live>
        {going && live ? (
          <>
            It's running in a pane on its machine.{" "}
            <a href={live} target="_blank" rel="noopener" data-run-pane>
              Watch pane %{run.pane} on Control ↗
            </a>
            <span class="dim"> The platform page has no machines; Control shows the pane end to end.</span>
          </>
        ) : going ? (
          <span class="dim">Waiting for its cloud workspace to take it.</span>
        ) : live ? (
          <span class="dim">
            It ended. It ran in pane %{run.pane}, which may still be open{" "}
            <a href={live} target="_blank" rel="noopener" data-run-pane>
              on Control ↗
            </a>
            .
          </span>
        ) : (
          <span class="dim">It ended without running in a pane.</span>
        )}
      </p>
      <RunLog run={run} />
      <Artifacts p={p} run={run} />
      <h2>Timeline</h2>
      <ol class="run-timeline" data-run-timeline>
        {(run.events ?? []).map((e) => (
          <TimelineRow key={e.seq} e={e} by={by} />
        ))}
      </ol>
      <RunThread run={run} msg={msg} />
    </section>
  );
}

function TimelineRow({ e, by }: { e: RunEventView; by: string }) {
  // Each row a sentence, its subject capitalised (UI audit #164).
  const who = e.from === null || e.from === undefined ? by : e.daemon ? "Its machine (signed)" : "Arugula";
  const what = e.from === null || e.from === undefined ? "made it" : `moved it to ${STATES[e.to]?.toLowerCase() ?? e.to}`;
  const detail = eventDetail(e.detail ?? {});
  return (
    <li class={`run-event ${tone(e.to)}`} data-run-event={e.to}>
      <span class={`run-dot ${tone(e.to)}`} aria-hidden="true" />
      <span>
        <b>{who}</b> {what}
        {detail ? <span class="dim"> · {detail}</span> : null}{" "}
        <Time class="dim" at={e.at} long />
      </span>
    </li>
  );
}

/** How often a going run's kept log is read again for what came. */
const FOLLOW_MS = 5_000;

/** Its durable log (M213): its tail once it's read, earlier pages on ask,
 * and while it goes, what its machine sends after: about the last 4 MiB of
 * it kept here, the older pages for Earlier to read again. Shown as text,
 * its terminal escapes taken out. */
function RunLog({ run }: { run: RunView }) {
  const [pages, setPages] = useState<RunLogPage[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const state = run.state;
  const going = !ended(state);
  useEffect(() => {
    let live = true;
    runs.log(run.id, { tail: true }).then(
      (pg) => live && (setPages([pg]), setError(null)),
      (e: Error) => live && setError(e.message),
    );
    return () => void (live = false);
  }, [run.id, state]);
  // While it goes: what came since the last page, every few seconds.
  const last = pages?.at(-1);
  const [tick, setTick] = useState(0);
  useEffect(() => {
    if (!going || !last) return;
    let live = true;
    const t = setTimeout(() => {
      const again = () => live && setTick((n) => n + 1);
      if (document.visibilityState !== "visible") return again();
      runs.log(run.id, { offset: last.next_offset }).then((pg) => {
        if (live && pg.kept && pg.text) setPages((p) => keepLatest(p ?? [], pg));
        again();
      }, again);
    }, FOLLOW_MS);
    return () => {
      live = false;
      clearTimeout(t);
    };
  }, [run.id, going, last, tick]);
  const first = pages?.[0];
  const earlier = async () => {
    if (!first || first.offset === 0) return;
    const offset = Math.max(0, first.offset - 64 * 1024);
    try {
      const pg = await runs.log(run.id, { offset, max: first.offset - offset });
      setPages([pg, ...(pages ?? [])]);
    } catch (e) {
      toast((e as Error).message);
    }
  };
  // The pane's own "press Enter for a shell" note goes (UI audit #146).
  const text = useMemo(() => {
    const t = stripAnsi((pages ?? []).map((p) => p.text).join(""));
    return going ? t : withoutExitNote(t);
  }, [pages, going]);
  const kept = !!pages?.some((p) => p.kept);
  // A failed run's log opens at its end, where it says why (UI audit
  // #146); one that goes stays there while it's followed at its end.
  const box = useRef<HTMLPreElement>(null);
  const opened = useRef(false);
  useLayoutEffect(() => {
    const el = box.current;
    if (!el || !text) return;
    if (!opened.current) {
      opened.current = true;
      if (state === "failed") el.scrollTop = el.scrollHeight;
    }
  }, [text, state]);
  return (
    <div class="run-log" data-run-log={pages ? (kept ? "kept" : "none") : ""}>
      <h2>Log</h2>
      {error && <p class="control-error">{error}</p>}
      {!pages && !error && <p class="dim">Loading…</p>}
      {first && !kept && <p class="dim" data-run-log-note>{sentence(first.note ?? "the platform keeps no log of this run")}</p>}
      {kept && first && (
        <>
          {first.offset > 0 && (
            <button class="chat-older" data-run-log-earlier onClick={() => void earlier()}>
              Earlier
            </button>
          )}
          <pre class="run-log-text" data-run-log-text ref={box}>
            {text || (going ? "(nothing yet)" : "(empty)")}
          </pre>
          <p class="dim run-log-foot">{going ? "It follows the log while the run goes." : last?.end ? "The end of its log. Kept 30 days." : "Kept 30 days."}</p>
        </>
      )}
    </div>
  );
}

/** A note the platform wrote in lower case, as a sentence. */
const sentence = (s: string) => `${s.charAt(0).toUpperCase()}${s.slice(1)}${/[.!?]$/.test(s) ? "" : "."}`;

function size(n: number): string {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
  return `${(n / 1024 / 1024).toFixed(1)} MB`;
}

/** What it left behind (M213): as the run-log bucket holds them, each
 * downloading through M213's redirect to a signed `GET`. */
function Artifacts({ p, run }: { p: PlatformProject; run: RunView }) {
  const [got, setGot] = useState<RunArtifacts | null>(null);
  const [error, setError] = useState<string | null>(null);
  const state = run.state;
  useEffect(() => {
    let live = true;
    runs.artifacts(p.id, run.id).then(
      (v) => live && (setGot(v), setError(null)),
      (e: Error) => live && setError(e.message),
    );
    return () => void (live = false);
  }, [p.id, run.id, state]);
  const list = got?.artifacts ?? [];
  return (
    <div class="run-artifacts" data-run-artifacts={list.length}>
      <h2>
        Artifacts <span class="dim">{list.length}</span>
      </h2>
      {error && <p class="control-error">{error}</p>}
      {got && !got.on && <p class="dim">This platform keeps no runs' logs or artifacts.</p>}
      {got?.on && list.length === 0 && <p class="dim">{ended(run.state) ? "It left none." : "None yet."}</p>}
      {list.length > 0 && (
        <ul class="platform-list">
          {list.map((a) => (
            <li key={a.name} data-run-artifact={a.name}>
              <a href={runs.artifactUrl(p.id, run.id, a.name)} download={a.name} data-run-artifact-download>
                {a.name}
              </a>{" "}
              <span class="dim">
                {size(a.size)} · <code title={a.digest}>{a.digest.slice(0, 19)}</code>
              </span>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

/** Its thread (M196's object room on the run's id): its project's people
 * talk it over. Threads come with chat. */
function RunThread({ run, msg }: { run: RunView; msg?: number }) {
  useSubscribe((fn) => chat.subscribe(fn));
  const phone = usePhone();
  const [room, setRoom] = useState<ChatRoom | null>(null);
  const [error, setError] = useState<string | null>(null);
  const on = chat.on;
  useEffect(() => {
    if (!on) return;
    let live = true;
    chat.openObject(run.id).then(
      (r) => live && setRoom(r),
      (e: Error) => live && setError(e.message),
    );
    return () => void (live = false);
  }, [run.id, on]);
  return (
    <div class="run-thread">
      <h2>Thread</h2>
      {on === false ? (
        <p class="dim">Threads come with chat, which is off for you. A team owner turns it on for the team under Teams, on Control.</p>
      ) : (
        <div class="issue-thread chat-thread" data-run-thread={room?.id}>
          {room ? (
            <RoomBody room={room} phone={phone} focus={msg} embedded name={`run ${short(run.id)}`} empty="Nothing said about this run yet. Everyone in the project reads it; write @name to notify someone." />
          ) : (
            <p class="thread-note">{error ? `The thread didn't load: ${error}` : "Loading the thread…"}</p>
          )}
        </div>
      )}
    </div>
  );
}
