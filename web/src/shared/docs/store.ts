// A project's Docs (aru-118), as the platform's Doc routes give them to the
// reader (crates/platform/src/project_docs.rs): the list, one Doc with its
// sections, and the writes. A Doc is the project's, in the machine Doc's
// shape (a title, then sections of Markdown, pending until written), so the
// page draws it with the Doc block's renderer.

import type { NewProjectDoc, ProjectDocList, ProjectDocPublic, ProjectDocTrash, ProjectDocView, ProjectDocWrite } from "../proto";
import { chat } from "../chat-store";
import { platformFetch, refusalOf } from "../maintenance";
import { titleRefusal } from "./doc-title.ts";

/** A refusal from the Doc routes, with its status and what else it said
 * (a 409 names the Doc that is there). */
export class DocError extends Error {
  constructor(
    readonly status: number,
    message: string,
    readonly body: { id?: string } = {},
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
  const j = (await res.json().catch(() => ({}))) as T & { error?: string; id?: string };
  if (!res.ok) throw new DocError(res.status, refusalOf(res.status, j, `HTTP ${res.status}`), { id: j.id });
  return j;
}

const enc = encodeURIComponent;
const docPath = (project: string, id: string) => `/api/projects/${enc(project)}/docs/${enc(id)}`;

/** The Docs' titles the page has read, for their links' slugs and tabs. */
const titles = new Map<string, string>();
const heard = new Set<() => void>();

/** Keep a Doc's title; the tab's title and the address follow a change (#979). */
function keep(id: string, title: string) {
  if (titles.get(id) === title) return;
  titles.set(id, title);
  for (const fn of [...heard]) fn();
}


export const projectDocs = {
  title: (id: string): string | undefined => titles.get(id),
  /** A Doc's title changed (read, renamed here or listed). Returns the way to stop. */
  subscribe(fn: () => void): () => void {
    heard.add(fn);
    return () => void heard.delete(fn);
  },
  async list(project: string): Promise<ProjectDocList> {
    const l = await call<ProjectDocList>("GET", `/api/projects/${enc(project)}/docs`);
    for (const d of l.docs) keep(d.id, d.title);
    return l;
  },
  async create(project: string, make: NewProjectDoc): Promise<ProjectDocView> {
    const v = await call<ProjectDocView>("POST", `/api/projects/${enc(project)}/docs`, make);
    keep(v.doc.id, v.doc.title);
    return v;
  },
  async get(project: string, id: string): Promise<ProjectDocView> {
    const v = await call<ProjectDocView>("GET", docPath(project, id));
    keep(id, v.doc.title);
    return v;
  },
  /** Rename it, write a section, or add one (what isn't given stays). */
  async write(project: string, id: string, w: Partial<ProjectDocWrite>): Promise<ProjectDocView> {
    const full: ProjectDocWrite = { title: null, section: null, heading: null, intent: null, markdown: null, ...w };
    const v = await call<ProjectDocView>("PUT", docPath(project, id), full);
    keep(id, v.doc.title);
    return v;
  },
  /** Rename it (#979): the title alone, checked as the platform does. */
  rename(project: string, id: string, title: string): Promise<ProjectDocView> {
    const why = titleRefusal(title);
    if (why) return Promise.reject(new DocError(400, why));
    return projectDocs.write(project, id, { title: title.trim() });
  },
  /** Arugula #871: make the Doc's public link (or read the one it has): the
   * platform's id and key, at version 0 until a client has put a version. */
  makePublic: (project: string, id: string) => call<ProjectDocPublic>("PUT", `${docPath(project, id)}/public`, {}),
  /** Say a version was committed at control, sealed from the Doc at `rev`. */
  servedPublic: (project: string, id: string, ver: number, rev: number) =>
    call<ProjectDocPublic>("POST", `${docPath(project, id)}/public`, { ver, rev }),
  /** Forget the link, once control has taken it down. */
  forgetPublic: (project: string, id: string) => call<Record<string, never>>("DELETE", `${docPath(project, id)}/public`),
  /** One of the Doc's pictures, the bytes. */
  async assetBytes(project: string, id: string, name: string): Promise<Uint8Array> {
    const res = await platformFetch(`${docPath(project, id)}/assets/${enc(name)}`, { credentials: "same-origin" });
    if (!res.ok) throw new DocError(res.status, `can't read the picture ${name}`);
    return new Uint8Array(await res.arrayBuffer());
  },
  remove: (project: string, id: string) => call<{ deleted: string }>("DELETE", docPath(project, id)),
  /** The project's trashed Docs (Arugula #866), the latest deleted first. */
  trash: (project: string) => call<ProjectDocTrash>("GET", `/api/projects/${enc(project)}/docs/trash`),
  /** Bring a trashed Doc back, whole. */
  restore: (project: string, id: string) => call<ProjectDocView>("POST", `${docPath(project, id)}/restore`),
  /** The Doc's thread (made the first time someone opens it): the room the
   * platform pushes a change on, so a page hears of it live. */
  thread: (id: string) => chat.openObject(id),
};
