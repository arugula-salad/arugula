// `/p/:project/docs` and `/p/:project/docs/:doc` (aru-118): a project's
// Docs, and one. A project Doc is the platform's (crates/platform/src/
// project_docs.rs): a title and sections of Markdown, in the machine Doc's
// shape, so it's drawn with the Doc block's renderer (charts and drawings
// included). Its members read it; owners and editors make, write and
// delete Docs (a viewer's page has no form); an agent session on a Cloud
// workspace makes them too, as itself. A write anywhere reaches an open
// page live, through the Doc's thread (as a board's does).

import { useEffect, useMemo, useRef, useState } from "preact/hooks";
import type { ProjectDoc, ProjectDocView } from "../../shared/proto";
import { chat } from "../../shared/chat-store";
import { DocError, projectDocs } from "../../shared/docs/store";
import { whoLabel } from "../../shared/issues/text";
import { useSubscribe } from "../../shared/ui/hooks";
import { PageHeader } from "../../shared/ui/page-header";
import { Time } from "../../shared/ui/time";
import { DocBlock, type DocState } from "../../shared/docs/doc-render";
import "../../shared/docs/docs.css";
import { names } from "../links";
import { follow, go, pathOf } from "../router";
import { SectionHead } from "../section-head";
import { session, type PlatformProject } from "../session";
import { toast } from "../toast";
import { mayWrite } from "./issues";
import { Missing } from "./project";

/** Who's who in `p`, for a Doc's maker. */
function nameOf(p: PlatformProject) {
  return (account: string) => (account === session.me?.account ? "you" : (p.team?.members.find((m) => m.account === account)?.name ?? null));
}

/** The project's Docs, `null` while they load. */
function useDocs(project: string): ProjectDoc[] | null {
  const [list, setList] = useState<ProjectDoc[] | null>(null);
  useEffect(() => {
    let live = true;
    projectDocs.list(project).then(
      (l) => live && setList(l.docs),
      () => live && setList([]),
    );
    return () => void (live = false);
  }, [project]);
  return list;
}

function DocRows({ p, list }: { p: PlatformProject; list: ProjectDoc[] }) {
  const who = nameOf(p);
  return (
    <ul class="platform-list sw-rows" data-docs>
      {list.map((d) => {
        const to = pathOf({ page: "doc", project: p.id, doc: d.id }, names);
        return (
          <li key={d.id} class="sw-row">
            <a href={to} data-doc-link={d.id} onClick={(e) => follow(e, to)}>
              {d.title}
            </a>
            <span class="dim project-doc-row">
              {" "}
              {d.written} of {d.sections} written · {whoLabel(d.author, d.agent, who)} · <Time at={d.updated_at} />
            </span>
          </li>
        );
      })}
    </ul>
  );
}

export function DocsPage({ project }: { project: string }) {
  useSubscribe((fn) => session.subscribe(fn));
  const p = session.project(project);
  if (!p) return <Missing what="That project isn't one of yours (or it's gone)." />;
  return <DocList p={p} />;
}

function DocList({ p }: { p: PlatformProject }) {
  const list = useDocs(p.id);
  const [title, setTitle] = useState("");
  const make = async (e: Event) => {
    e.preventDefault();
    if (!title.trim()) return;
    try {
      const v = await projectDocs.create(p.id, { title: title.trim(), sections: [] });
      go(pathOf({ page: "doc", project: p.id, doc: v.doc.id }, names));
    } catch (err) {
      toast(`Couldn't make the Doc: ${(err as Error).message}`);
    }
  };
  return (
    <section class="platform-section sw-page" data-docs-page>
      <PageHeader title="Docs" sub={`${p.name}'s reports, plans and write-ups, by its people and its agent sessions`} />
      {list === null && <p class="dim">Loading…</p>}
      {list?.length === 0 && (
        <p class="dim" data-docs-none>
          No Docs yet.
        </p>
      )}
      {list && <DocRows p={p} list={list} />}
      {mayWrite(p) && (
        <form class="project-doc-form" onSubmit={(e) => void make(e)}>
          <input type="text" value={title} maxLength={200} placeholder="a new Doc's title" aria-label="New Doc's title" data-doc-new-title onInput={(e) => setTitle(e.currentTarget.value)} />
          <button type="submit" class="primary" data-doc-new disabled={!title.trim()}>
            Make a Doc
          </button>
        </form>
      )}
    </section>
  );
}

/** The Doc's view as the Doc block's renderer takes it. */
function stateOf(v: ProjectDocView): DocState {
  return {
    title: v.doc.title,
    rev: v.doc.rev,
    entries: v.sections.map((s) => ({ id: s.id, heading: s.heading, intent: s.intent, body: s.body, rev: s.rev })),
  };
}

/** A project Doc has no pictures of its own: an `assets/` image shows its alt text. */
const noImages = async () => null;

export function DocPage({ project, doc }: { project: string; doc: string }) {
  useSubscribe((fn) => session.subscribe(fn));
  // Chat says when the Doc's thread's object changed.
  useSubscribe((fn) => chat.subscribe(fn));
  const p = session.project(project);
  const [view, setView] = useState<ProjectDocView | null>(null);
  const [missing, setMissing] = useState<string | null>(null);
  const [fresh, setFresh] = useState(new Map<string, number>());
  const seen = useRef(0);
  const last = useRef<ProjectDocView | null>(null);

  const show = (v: ProjectDocView) => {
    // A section written since the page last had it is highlighted, as in the block.
    const before = new Map(last.current?.sections.map((s) => [s.id, s.rev]) ?? []);
    if (last.current) setFresh(new Map(v.sections.filter((s) => s.body !== null && before.get(s.id) !== s.rev).map((s) => [s.id, s.rev])));
    last.current = v;
    setView(v);
    setMissing(null);
  };
  const again = () => {
    const n = ++seen.current;
    projectDocs.get(project, doc).then(
      (v) => n === seen.current && show(v),
      (e: Error) => n === seen.current && setMissing(e instanceof DocError && e.status === 404 ? "it isn't there, or isn't yours to read" : e.message),
    );
  };
  useEffect(() => {
    last.current = null;
    again();
    // The thread is what the platform pushes a change on: open it so there is one.
    projectDocs.thread(doc).catch(() => {});
    const off = chat.onObject(doc, again);
    const back = () => document.visibilityState === "visible" && again();
    document.addEventListener("visibilitychange", back);
    return () => {
      off();
      document.removeEventListener("visibilitychange", back);
    };
  }, [project, doc]);

  const state = useMemo(() => (view ? stateOf(view) : null), [view]);
  if (!p) return <Missing what="That project isn't one of yours (or it's gone)." />;
  const all = pathOf({ page: "docs", project }, names);
  return (
    <section class="platform-section project-doc" data-doc-page={doc} data-writer={view ? String(view.writer) : undefined}>
      <p class="dim project-doc-head">
        <a href={all} onClick={(e) => follow(e, all)}>
          {p.name}'s Docs
        </a>
        {view && (
          <>
            {" · "}
            <span data-doc-maker>made by {whoLabel(view.doc.author, view.doc.agent, nameOf(p))}</span> · changed <Time at={view.doc.updated_at} />
          </>
        )}
      </p>
      {missing && !view && (
        <p class="dim" data-doc-missing>
          That Doc can't be read: {missing}.
        </p>
      )}
      {!missing || view ? <DocBlock id={0} s={state} fresh={fresh} load={noImages} /> : null}
      {view?.writer && <DocWriter project={project} view={view} wrote={show} />}
    </section>
  );
}

/** For owners and editors: write a section (its Markdown replaced), add
 * one, or delete the Doc. */
function DocWriter({ project, view, wrote }: { project: string; view: ProjectDocView; wrote: (v: ProjectDocView) => void }) {
  const NEW = "";
  const firstPending = view.sections.find((s) => s.body === null)?.id ?? NEW;
  const [section, setSection] = useState(firstPending);
  const [heading, setHeading] = useState("");
  const current = view.sections.find((s) => s.id === section);
  const [markdown, setMarkdown] = useState(current?.body ?? "");
  const [busy, setBusy] = useState(false);
  const pick = (id: string) => {
    setSection(id);
    setMarkdown(view.sections.find((s) => s.id === id)?.body ?? "");
  };
  const save = async (e: Event) => {
    e.preventDefault();
    setBusy(true);
    try {
      const v = section === NEW ? await projectDocs.write(project, view.doc.id, { heading: heading.trim(), markdown }) : await projectDocs.write(project, view.doc.id, { section, markdown });
      wrote(v);
      if (section === NEW) {
        // Emptied for the next one, unless that's already being typed.
        const [sentHeading, sentMarkdown] = [heading, markdown];
        setHeading((h) => (h === sentHeading ? "" : h));
        setMarkdown((m) => (m === sentMarkdown ? "" : m));
      }
    } catch (err) {
      toast(`Couldn't write it: ${(err as Error).message}`);
    } finally {
      setBusy(false);
    }
  };
  const [gone, setGone] = useState(false);
  const remove = async () => {
    try {
      await projectDocs.remove(project, view.doc.id);
      go(pathOf({ page: "docs", project }, names));
    } catch (err) {
      toast(`Couldn't delete it: ${(err as Error).message}`);
    }
  };
  const ready = section === NEW ? !!heading.trim() : markdown !== (current?.body ?? "");
  return (
    <form class="project-doc-form project-doc-writer" onSubmit={(e) => void save(e)} data-doc-writer>
      <label>
        Write{" "}
        <select value={section} onChange={(e) => pick(e.currentTarget.value)} data-doc-section-pick>
          {view.sections.map((s) => (
            <option key={s.id} value={s.id}>
              {s.heading}
              {s.body === null ? " (pending)" : ""}
            </option>
          ))}
          <option value={NEW}>A new section at the end…</option>
        </select>
      </label>
      {section === NEW && <input type="text" value={heading} maxLength={200} placeholder="its heading" aria-label="The new section's heading" data-doc-heading-input onInput={(e) => setHeading(e.currentTarget.value)} />}
      <textarea rows={8} value={markdown} placeholder="Markdown: lists, tables, ```chart and ```svg fences" aria-label="The section's Markdown" data-doc-markdown-input onInput={(e) => setMarkdown(e.currentTarget.value)} />
      <span class="project-doc-buttons">
        <button type="submit" class="primary" disabled={busy || !ready} data-doc-save>
          {section === NEW ? "Add the section" : "Save the section"}
        </button>
        <button type="button" class="secondary danger" onClick={() => setGone(true)} data-doc-delete>
          Delete the Doc
        </button>
      </span>
      {gone && (
        <p class="project-doc-confirm" data-doc-confirm>
          Delete “{view.doc.title}” for everyone in the project?{" "}
          <button type="button" class="danger" data-doc-confirm-delete onClick={() => void remove()}>
            Delete
          </button>{" "}
          <button type="button" onClick={() => setGone(false)}>
            Keep
          </button>
        </p>
      )}
    </form>
  );
}

/** The project home's Docs: the latest few, and the way to the rest. */
export function ProjectDocs({ p }: { p: PlatformProject }) {
  const list = useDocs(p.id);
  if (list === null) return null;
  const all = pathOf({ page: "docs", project: p.id }, names);
  return (
    <>
      <SectionHead title="Docs" all={all} data="project-docs-link" />
      {list.length === 0 && <p class="dim">No Docs yet.</p>}
      <div data-project-docs>
        <DocRows p={p} list={list.slice(0, 5)} />
      </div>
    </>
  );
}
