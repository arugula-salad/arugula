// A project's runs as the platform page reads and starts them (M215):
// M216's reads (a list, one run with its timeline, a page of its durable
// log), M214's starts and its run specs with their schedules and rules,
// and M213's artifacts. The runs the page has read are kept by id, for an
// id-only address's project.

import type { RunList, RunLogPage, RunView } from "../shared/proto.gen";
import { platformFetch, refusalOf } from "../shared/maintenance";
import { runsQuery, type RunFilter } from "./runs-text";

export type { RunFilter, RunLogPage, RunView };

/** A run spec (M214), with its schedules and rules. */
export interface RunSpec {
  id: string;
  project: string;
  name: string;
  workspace: string;
  kind: string;
  argv?: string[];
  instruction?: string;
  agent?: string;
  directory?: string;
  schedules: Schedule[];
  rules: Rule[];
}

export interface Schedule {
  id: string;
  cron: string;
  timezone: string;
  enabled: boolean;
  next_at: number;
  last_fired_at: number | null;
  last_run: string | null;
  last_error: string | null;
  failed: boolean;
}

export interface Rule {
  id: string;
  event: "pr_opened" | "check_failed" | "push";
  repo: string | null;
  branch: string | null;
  enabled: boolean;
  outsiders: boolean;
  max_per_hour: number;
}

/** A run's artifacts as the bucket holds them (M213's
 * `GET /api/projects/{project}/runs/{run}/artifacts`): `on` is false on a
 * platform that keeps no run logs. */
export interface RunArtifacts {
  run: string;
  on: boolean;
  artifacts: { name: string; size: number; digest: string; url: string }[];
  expires_at: number;
}

/** A project's hosted workspace (M179), as far as runs need it. */
export interface HostedWorkspace {
  id: string;
  name: string;
  state: string;
}

/** What a start takes: a spec, or a command or an instruction on a
 * workspace (the project's only one when left out). */
export interface StartRun {
  spec?: string;
  workspace?: string;
  argv?: string[];
  instruction?: string;
}

export class RunError extends Error {
  constructor(
    readonly status: number,
    message: string,
  ) {
    super(message);
  }
}

async function call<T>(method: string, path: string, body?: unknown): Promise<T> {
  const res = await platformFetch(path, {
    method,
    headers: body === undefined ? {} : { "Content-Type": "application/json" },
    body: body === undefined ? undefined : JSON.stringify(body),
    credentials: "same-origin",
  });
  const j = (await res.json().catch(() => ({}))) as T & { error?: string; reason?: string };
  if (!res.ok) throw new RunError(res.status, refusalOf(res.status, j, `HTTP ${res.status}`));
  return j;
}

const enc = encodeURIComponent;
const known = new Map<string, RunView>();
const listeners = new Set<() => void>();
const keep = (r: RunView) => {
  known.set(r.id, r);
  listeners.forEach((fn) => fn());
  return r;
};

export const runs = {
  subscribe(fn: () => void) {
    listeners.add(fn);
    return () => void listeners.delete(fn);
  },
  /** A run already read. */
  known: (id: string) => known.get(id),
  list: (project: string, f: RunFilter) => call<RunList>("GET", `/api/projects/${enc(project)}/runs?${runsQuery(f)}`),
  /** One run with its timeline: at once, or (`since`) once it has an
   * event after that one or has ended, waiting up to 15 s. */
  get: (id: string, since?: number) => call<RunView>("GET", `/api/runs/${enc(id)}${since === undefined ? "" : `?since=${since}&wait=15`}`).then(keep),
  /** A page of its durable log: its tail, or from `offset` (at most `max` bytes). */
  log: (id: string, at: { tail: true } | { offset: number; max?: number }) => {
    const q = "tail" in at ? "tail=true" : `offset=${at.offset}${at.max ? `&max=${at.max}` : ""}`;
    return call<RunLogPage>("GET", `/api/runs/${enc(id)}/log?${q}`);
  },
  /** Its artifacts in the run-log bucket (M213). */
  artifacts: (project: string, id: string) => call<RunArtifacts>("GET", `/api/projects/${enc(project)}/runs/${enc(id)}/artifacts`),
  /** Where one of its artifacts downloads from: M213's redirect to a
   * signed `GET` that saves it as a file. */
  artifactUrl: (project: string, id: string, name: string) => `/api/projects/${enc(project)}/runs/${enc(id)}/artifacts/${enc(name)}`,
  start: (project: string, b: StartRun) => call<RunView>("POST", `/api/projects/${enc(project)}/runs`, b),
  specs: (project: string) => call<{ project: string; specs: RunSpec[] }>("GET", `/api/projects/${enc(project)}/run-specs`).then((v) => v.specs),
  addSpec: (project: string, b: { name: string; workspace: string; argv?: string[]; instruction?: string }) =>
    call<RunSpec>("POST", `/api/projects/${enc(project)}/run-specs`, b),
  deleteSpec: (project: string, spec: string) => call<unknown>("DELETE", `/api/projects/${enc(project)}/run-specs/${enc(spec)}`),
  addSchedule: (project: string, spec: string, b: { cron: string; timezone?: string }) =>
    call<Schedule>("POST", `/api/projects/${enc(project)}/run-specs/${enc(spec)}/schedules`, b),
  setSchedule: (project: string, spec: string, id: string, enabled: boolean) =>
    call<Schedule>("PATCH", `/api/projects/${enc(project)}/run-specs/${enc(spec)}/schedules/${enc(id)}`, { enabled }),
  deleteSchedule: (project: string, spec: string, id: string) => call<unknown>("DELETE", `/api/projects/${enc(project)}/run-specs/${enc(spec)}/schedules/${enc(id)}`),
  addRule: (project: string, spec: string, b: { event: Rule["event"]; repo?: string; branch?: string }) =>
    call<Rule>("POST", `/api/projects/${enc(project)}/run-specs/${enc(spec)}/rules`, b),
  setRule: (project: string, spec: string, id: string, enabled: boolean) =>
    call<Rule>("PATCH", `/api/projects/${enc(project)}/run-specs/${enc(spec)}/rules/${enc(id)}`, { enabled }),
  deleteRule: (project: string, spec: string, id: string) => call<unknown>("DELETE", `/api/projects/${enc(project)}/run-specs/${enc(spec)}/rules/${enc(id)}`),
  /** The project's hosted workspaces; none when the platform has none on. */
  workspaces: (project: string) =>
    call<{ workspaces: HostedWorkspace[] }>("GET", `/api/projects/${enc(project)}/workspaces`).then(
      (v) => v.workspaces,
      () => [] as HostedWorkspace[],
    ),
};
