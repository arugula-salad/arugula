// A project's Docs (aru-118), as the platform's Doc routes give them to the
// reader (crates/platform/src/project_docs.rs): the list, one Doc with its
// sections, and the writes. A Doc is the project's, in the machine Doc's
// shape (a title, then sections of Markdown, pending until written), so the
// page draws it with the Doc block's renderer.

import type { NewProjectDoc, ProjectDocList, ProjectDocView, ProjectDocWrite } from "../proto";
import { chat } from "../chat-store";
import { platformFetch, refusalOf } from "../maintenance";

/** A refusal from the Doc routes, with its status. */
export class DocError extends Error {
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
  const j = (await res.json().catch(() => ({}))) as T & { error?: string };
  if (!res.ok) throw new DocError(res.status, refusalOf(res.status, j, `HTTP ${res.status}`));
  return j;
}

const enc = encodeURIComponent;
const docPath = (project: string, id: string) => `/api/projects/${enc(project)}/docs/${enc(id)}`;

/** The Docs' titles the page has read, for their links' slugs and tabs. */
const titles = new Map<string, string>();

export const projectDocs = {
  title: (id: string): string | undefined => titles.get(id),
  async list(project: string): Promise<ProjectDocList> {
    const l = await call<ProjectDocList>("GET", `/api/projects/${enc(project)}/docs`);
    for (const d of l.docs) titles.set(d.id, d.title);
    return l;
  },
  async create(project: string, doc: NewProjectDoc): Promise<ProjectDocView> {
    const v = await call<ProjectDocView>("POST", `/api/projects/${enc(project)}/docs`, doc);
    titles.set(v.doc.id, v.doc.title);
    return v;
  },
  async get(project: string, id: string): Promise<ProjectDocView> {
    const v = await call<ProjectDocView>("GET", docPath(project, id));
    titles.set(id, v.doc.title);
    return v;
  },
  /** Rename it, write a section, or add one (what isn't given stays). */
  async write(project: string, id: string, w: Partial<ProjectDocWrite>): Promise<ProjectDocView> {
    const full: ProjectDocWrite = { title: null, section: null, heading: null, intent: null, markdown: null, ...w };
    const v = await call<ProjectDocView>("PUT", docPath(project, id), full);
    titles.set(id, v.doc.title);
    return v;
  },
  remove: (project: string, id: string) => call<{ deleted: string }>("DELETE", docPath(project, id)),
  /** The Doc's thread (made the first time someone opens it): the room the
   * platform pushes a change on, so a page hears of it live. */
  thread: (id: string) => chat.openObject(id),
};
