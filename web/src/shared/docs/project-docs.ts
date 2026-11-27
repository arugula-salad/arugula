// A project's Docs (Arugula #864): the list, read again when the tab comes
// back, and a Doc's state and pictures as the Doc block's renderer takes
// them.

import { useEffect, useMemo, useRef, useState } from "preact/hooks";
import type { ProjectDoc, ProjectDocView } from "../proto";
import type { DocState, ImageCache, ImageLoader } from "./doc-render";
import { projectDocs } from "./store";

/** A project Doc's times, as a Doc's line says them. */
export const timesOf = (d: ProjectDoc) => ({ made: d.created_at, updated: d.updated_at });

/** The project's Docs, `null` while they load; none when it can't be read.
 * Read again when the tab comes back, or on `reload`. */
export function useProjectDocs(project: string | null): { docs: ProjectDoc[] | null; reload: () => void } {
  const [docs, setDocs] = useState<ProjectDoc[] | null>(null);
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
