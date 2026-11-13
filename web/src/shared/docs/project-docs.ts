// A project's Docs as the one Docs list reads them (Arugula #864): the list,
// each Doc read whole a few at a time for its headings and shape, and the
// one in the preview kept live through its thread. The platform's page and
// a team machine's page both list a project's Docs this way.

import { useEffect, useRef, useState } from "preact/hooks";
import type { ProjectDoc, ProjectDocSection, ProjectDocView } from "../proto";
import { chat } from "../chat-store";
import type { DocState } from "./doc-render";
import type { DocShapeSection, DocsListItem } from "./docs-list";
import { madeBy } from "./docs-mode";
import { projectDocs } from "./store";

const wordsIn = (s: ProjectDocSection) => (s.body?.match(/\S+/g) ?? []).length;
const figuresIn = (s: ProjectDocSection) => (s.body?.match(/^(```(chart|svg|prototype)|!\[)/gm) ?? []).length;

/** A project Doc's written sections, as the shape and the outline draw them. */
export const shapeOf = (sections: ProjectDocSection[]): DocShapeSection[] =>
  sections.filter((s) => s.body !== null).map((s) => ({ heading: s.heading, words: wordsIn(s), figures: figuresIn(s) }));

/** A project Doc's times, as the Docs' sort and line say them. */
export const timesOf = (d: ProjectDoc) => ({ made: d.created_at, updated: d.updated_at });

/** The Docs read whole, by Doc and its rev: their sections. */
const read = new Map<string, ProjectDocSection[]>();
const readKey = (d: ProjectDoc) => `${d.id}:${d.rev}`;

/** A project Doc as the list draws it; `me` is the reader's account. */
export function projectItem(d: ProjectDoc, me: string | null | undefined): DocsListItem {
  const s = read.get(readKey(d));
  return { id: d.id, title: d.title, layout: d.layout, ...timesOf(d), sections: s && shapeOf(s), mine: madeBy(d, me) };
}

/** The project's Docs, `null` while they load; none when it can't be read.
 * Read again when the tab comes back, or on `reload`. */
export function useProjectDocs(project: string | null): { docs: ProjectDoc[] | null; reload: () => void } {
  const [docs, setDocs] = useState<ProjectDoc[] | null>(null);
  const [, bump] = useState(0);
  const [again, setAgain] = useState(0);
  const had = useRef<string | null>(null);
  useEffect(() => {
    if (!project) return;
    let live = true;
    // Read again shows what's there while it does; another project, the loading.
    if (had.current !== project) setDocs(null);
    had.current = project;
    const fetched = () =>
      projectDocs.list(project).then(
        (l) => live && setDocs(l.docs),
        () => live && setDocs((was) => was ?? []),
      );
    void fetched();
    const back = () => document.visibilityState === "visible" && void fetched();
    document.addEventListener("visibilitychange", back);
    return () => {
      live = false;
      document.removeEventListener("visibilitychange", back);
    };
  }, [project, again]);
  // Each Doc is read whole, a few at a time, for its headings and shape.
  useEffect(() => {
    if (!docs || !project) return;
    let live = true;
    const todo = docs.filter((d) => !read.has(readKey(d)));
    const worker = async () => {
      for (let d = todo.shift(); d && live; d = todo.shift()) {
        try {
          read.set(readKey(d), (await projectDocs.get(project, d.id)).sections);
          if (live) bump((n) => n + 1);
        } catch {
          read.set(readKey(d), []);
        }
      }
    };
    void Promise.all([worker(), worker(), worker()]);
    return () => void (live = false);
  }, [docs]);
  return { docs, reload: () => setAgain((n) => n + 1) };
}

/** The Doc's view as the Doc block's renderer takes it. */
export function stateOf(v: ProjectDocView): DocState {
  return {
    title: v.doc.title,
    rev: v.doc.rev,
    layout: v.doc.layout,
    ...timesOf(v.doc),
    entries: v.sections.map((s) => ({ id: s.id, heading: s.heading, intent: s.intent, body: s.body, rev: s.rev })),
  };
}

/** One Doc, whole, read again whenever its thread says it changed; `null` until it's read. */
export function useProjectDoc(project: string, id: string): ProjectDocView | null {
  const [view, setView] = useState<ProjectDocView | null>(null);
  const seen = useRef(0);
  useEffect(() => {
    const again = () => {
      const n = ++seen.current;
      projectDocs.get(project, id).then(
        (v) => n === seen.current && setView(v),
        () => {},
      );
    };
    again();
    projectDocs.thread(id).catch(() => {});
    return chat.onObject(id, again);
  }, [project, id]);
  return view;
}

/** A project Doc has no pictures of its own here: an `assets/` image shows its alt text. */
export const noImages = async () => null;
