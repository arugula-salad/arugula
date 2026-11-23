// A project's Docs as the one Docs list reads them (Arugula #864): the list,
// each Doc read whole a few at a time for its headings and length, and the
// one in the preview kept live through its thread. The platform's page and
// a team machine's page both list a project's Docs this way.

import { useEffect, useMemo, useRef, useState } from "preact/hooks";
import type { ProjectDoc, ProjectDocSection, ProjectDocView } from "../proto";
import { chat } from "../chat-store";
import type { DocState, ImageCache, ImageLoader } from "./doc-render";
import type { DocShapeSection, DocsListItem } from "./docs-list";
import { madeBy } from "./docs-mode";
import { projectDocs } from "./store";

const wordsIn = (s: ProjectDocSection) => (s.body?.match(/\S+/g) ?? []).length;
const figuresIn = (s: ProjectDocSection) => (s.body?.match(/^(```(chart|svg|prototype)|!\[)/gm) ?? []).length;

/** A project Doc's written sections, as the facts and the outline count them. */
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
  // Each Doc is read whole, a few at a time, for its headings and length.
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
    assets: v.assets.map((a) => ({ name: a.name, mime: a.mime, size: a.size, rev: a.rev })),
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

/** The pictures read, by Doc, each by `name/rev` as its data: URL: a Doc
 * opened again, or redrawn, shows them without asking. */
const pictures = new Map<string, ImageCache>();

/** A project Doc's pictures, from the platform (Arugula #1409): a picture
 * once, by `name/rev`, and the name's older revs are done with. A data: URL,
 * as the Doc block's are: control's pages take no blob: image. */
export function useProjectImages(project: string, id: string): { load: ImageLoader; cached: ImageCache } {
  return useMemo(() => {
    let cached = pictures.get(id);
    if (!cached) pictures.set(id, (cached = new Map()));
    const kept = cached;
    const asking = new Map<string, Promise<string | null>>();
    const load = (name: string, rev: number) => {
      const key = `${name}/${rev}`;
      const have = kept.get(key);
      if (have) return Promise.resolve(have);
      let p = asking.get(key);
      if (!p) {
        p = projectDocs
          .assetBytes(project, id, name)
          .then((b) => {
            const url = `data:${mimeOf(b)};base64,${toBase64(b)}`;
            for (const k of kept.keys()) if (k.startsWith(`${name}/`)) kept.delete(k);
            kept.set(key, url);
            return url;
          })
          .catch(() => null)
          .finally(() => asking.delete(key));
        asking.set(key, p);
      }
      return p;
    };
    return { load, cached: kept };
  }, [project, id]);
}

/** What a picture's bytes say it is: the four the platform keeps. */
function mimeOf(b: Uint8Array): string {
  if (b[0] === 0xff && b[1] === 0xd8) return "image/jpeg";
  if (b[0] === 0x47 && b[1] === 0x49 && b[2] === 0x46) return "image/gif";
  if (b[8] === 0x57 && b[9] === 0x45 && b[10] === 0x42 && b[11] === 0x50) return "image/webp";
  return "image/png";
}

function toBase64(b: Uint8Array): string {
  let s = "";
  for (const x of b) s += String.fromCharCode(x);
  return btoa(s);
}
