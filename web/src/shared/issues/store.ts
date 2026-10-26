// A project's native issues (M201), as the platform's issue routes give
// them (M195) to the reader: a page of them, one with its timeline, a new
// one, a change, and the project's issue home. Shared since M203: an issue
// block on control's page reads through it too. The issues the page has
// read are kept by id, for their links' slugs and for an id-only
// address's project. An issue's comments are its thread (M196): chat's
// store has them.

import type {
  IssueEvent,
  IssueEvents,
  IssueHome,
  IssueList,
  IssuePatch,
  IssuePatched,
  NativeIssue,
  NewNativeIssue,
} from "../proto";
import { chat } from "../chat-store";
import { platformFetch, refusalOf } from "../maintenance";
import { queryOf, type IssueFilter } from "./text";

export type { IssueFilter };

/** A refusal from the issue routes: its status, and `reason` (`chat_off`
 * while chat's Labs flag is off) when it gave one. */
export class IssueError extends Error {
  constructor(
    readonly status: number,
    message: string,
    readonly reason?: string,
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
  if (!res.ok) throw new IssueError(res.status, refusalOf(res.status, j, `HTTP ${res.status}`), j.reason);
  return j;
}

const enc = encodeURIComponent;

const listeners = new Set<() => void>();
const known = new Map<string, NativeIssue>();

function keep(list: NativeIssue[]) {
  for (const i of list) known.set(i.id, i);
  listeners.forEach((fn) => fn());
}

export const issues = {
  subscribe(fn: () => void) {
    listeners.add(fn);
    return () => void listeners.delete(fn);
  },
  /** An issue the page has read. */
  known: (id: string): NativeIssue | undefined => known.get(id),
  /** Every issue the page has read. */
  all: (): NativeIssue[] => [...known.values()],

  async list(project: string, f: IssueFilter, before?: number, limit?: number): Promise<IssueList> {
    const r = await call<IssueList>("GET", `/api/projects/${enc(project)}/issues?${queryOf(f, before, limit)}`);
    keep(r.issues);
    return r;
  },
  async create(project: string, n: NewNativeIssue): Promise<NativeIssue> {
    const i = await call<NativeIssue>("POST", `/api/projects/${enc(project)}/issues`, n);
    keep([i]);
    return i;
  },
  async get(id: string): Promise<NativeIssue> {
    const i = await call<NativeIssue>("GET", `/api/issues/${enc(id)}`);
    keep([i]);
    return i;
  },
  async events(id: string): Promise<IssueEvent[]> {
    return (await call<IssueEvents>("GET", `/api/issues/${enc(id)}/events`)).events;
  },
  /** Change what's given; the rest stays. */
  async change(id: string, p: Partial<IssuePatch>): Promise<IssuePatched> {
    const full: IssuePatch = { title: null, body: null, state: null, reason: null, labels: null, assignees: null, ...p };
    const r = await call<IssuePatched>("PATCH", `/api/issues/${enc(id)}`, full);
    keep([r.issue]);
    return r;
  },
  home: (project: string) => call<IssueHome>("GET", `/api/projects/${enc(project)}/issue-home`),
  /** Every label on the project's issues, open or closed (UI audit #163):
   * the issue home says them. */
  labels: (project: string) => call<IssueHome>("GET", `/api/projects/${enc(project)}/issue-home`).then((h) => h.labels ?? []),
  /** `native`, `github`, or null for the default again. */
  setHome: (project: string, home: string | null) => call<IssueHome>("PUT", `/api/projects/${enc(project)}/issue-home`, { home }),
  /** The issue's thread (made the first time someone opens it). */
  thread: (id: string) => chat.openObject(id),
  /** Where the project's export downloads from. */
  exportUrl: (project: string, format: "json" | "md") => `/api/projects/${enc(project)}/issues/export?format=${format}`,
};
