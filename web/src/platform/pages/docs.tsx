// `/docs`, `/p/:project/docs` and `/p/:project/docs/:doc` (aru-118, #418): every
// Doc the reader can read (their private ones first), a project's, and one.
// The lists are on the shared list page, the preview draws the whole Doc. A project Doc is the platform's (crates/platform/src/
// project_docs.rs): a title and sections of Markdown, in the machine Doc's
// shape, so it's drawn with the Doc block's renderer (charts and drawings
// included). Its members read it; owners and editors make, write and
// delete Docs (a viewer's page has no form); an agent session on a Cloud
// workspace makes them too, as itself. A write anywhere reaches an open
// page live, through the Doc's thread (as a board's does).

import { useEffect, useMemo, useRef, useState } from "preact/hooks";
import type { BoardOfDoc, ProjectDoc, ProjectDocSection, ProjectDocView } from "../../shared/proto";
import { chat } from "../../shared/chat-store";
import { DocError, projectDocs } from "../../shared/docs/store";
import { boards } from "../../shared/boards/store";
import { onBoardWords } from "../../shared/boards/text";
import { whoLabel } from "../../shared/issues/text";
import { useSubscribe } from "../../shared/ui/hooks";
import { CopyLinkButton } from "../../shared/ui/copy-link";
import { ListCounts, ListPage, ListPreview, type ListCount, type ListGroup } from "../../shared/ui/list-page";
import { PageHeader } from "../../shared/ui/page-header";
import { Time } from "../../shared/ui/time";
import { DocBlock, type DocState } from "../../shared/docs/doc-render";
import "../../shared/docs/docs.css";
import { names } from "../links";
import { follow, go, pathOf } from "../router";
import { SectionHead } from "../../shared/ui/section-head";
import { session, type PlatformProject } from "../session";
import { toast } from "../toast";
import { mayWrite } from "./issues";
import { Missing } from "./project";

/** Who's who in `p`, for a Doc's maker. */
function nameOf(p: PlatformProject) {
  return (account: string) => (account === session.me?.account ? "you" : (p.team?.members.find((m) => m.account === account)?.name ?? null));
}

/** The machine a Doc is a copy from, by name (or that it left), or null
 * for one made here. */
export function copyFrom(d: ProjectDoc): string | null {
  return d.machine ? d.machine_name || "a machine that left" : null;
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
        const copy = copyFrom(d);
        return (
          <li key={d.id} class="sw-row">
            <a href={to} data-doc-link={d.id} onClick={(e) => follow(e, to)}>
              {d.title}
            </a>
            <span class="dim project-doc-row">
              {" "}
              {d.written} of {d.sections} written · {copy ? `a copy from ${copy}` : whoLabel(d.author, d.agent, who)} · <Time at={d.updated_at} />
            </span>
          </li>
        );
      })}
    </ul>
  );
}

/** The Doc's shape (as the machine's page draws it, shell/ui/docs-page.tsx):
 * a segment a section, as long as the section is, brighter where it has a figure. */
const LONG_DOC = 20_000;
const MARK_W = 130;
const wordsIn = (s: ProjectDocSection) => (s.body?.match(/\S+/g) ?? []).length;
const figuresIn = (s: ProjectDocSection) => (s.body?.match(/^(```(chart|svg|prototype)|!\[)/gm) ?? []).length;

function Shape({ sections }: { sections: ProjectDocSection[] }) {
  const written = sections.filter((s) => s.body !== null);
  if (!written.length) return null;
  const words = written.reduce((n, s) => n + wordsIn(s), 0);
  const width = Math.max(26, Math.sqrt(Math.min(words, LONG_DOC) / LONG_DOC) * MARK_W);
  return (
    <span class="doc-shape" style={{ width: `${Math.round(width)}px` }} aria-hidden="true" data-doc-shape>
      {written.map((s) => (
        <i key={s.id} class={figuresIn(s) ? "fig" : undefined} style={{ flex: `${Math.max(wordsIn(s), words * 0.012, 1)} 0 0` }} />
      ))}
    </span>
  );
}

/** The Docs the page has read whole, by Doc and its rev: their shape and headings. */
const read = new Map<string, ProjectDocSection[]>();
const readKey = (d: ProjectDoc) => `${d.id}:${d.rev}`;

interface Row {
  doc: ProjectDoc;
  p: PlatformProject;
  /** In the reader's own project: nobody else reads it. */
  mine: boolean;
}

/** The projects' Docs, each with the project it's in; `null` while they load. A
 * project that can't be read has none. Read again when the tab comes back
 * or a listed Doc's thread says it changed. */
function useRows(ps: PlatformProject[]): { rows: Row[] | null; sections: Map<string, ProjectDocSection[]> } {
  const [lists, setLists] = useState<Map<string, ProjectDoc[]> | null>(null);
  const [, bump] = useState(0);
  const key = ps.map((p) => p.id).join(",");
  useEffect(() => {
    let live = true;
    setLists(null);
    const again = () =>
      Promise.all(ps.map((p) => projectDocs.list(p.id).then((l): [string, ProjectDoc[]] => [p.id, l.docs], (): [string, ProjectDoc[]] => [p.id, []]))).then((all) => {
        if (!live) return;
        setLists(new Map(all));
      });
    void again();
    const back = () => document.visibilityState === "visible" && void again();
    document.addEventListener("visibilitychange", back);
    return () => {
      live = false;
      document.removeEventListener("visibilitychange", back);
    };
  }, [key]);
  // Each Doc is read whole, a few at a time, for its headings and shape.
  useEffect(() => {
    if (!lists) return;
    let live = true;
    const todo = ps.flatMap((p) => (lists.get(p.id) ?? []).filter((d) => !read.has(readKey(d))).map((d) => [p.id, d] as const));
    const worker = async () => {
      for (let next = todo.shift(); next && live; next = todo.shift()) {
        const [pid, d] = next;
        try {
          read.set(readKey(d), (await projectDocs.get(pid, d.id)).sections);
          if (live) bump((n) => n + 1);
        } catch {
          read.set(readKey(d), []);
        }
      }
    };
    void Promise.all([worker(), worker(), worker()]);
    return () => void (live = false);
  }, [lists]);
  const rows = lists
    ? ps.flatMap((p) => (lists.get(p.id) ?? []).map((doc) => ({ doc, p, mine: !p.team })))
    : null;
  return { rows, sections: read };
}

export function DocsPage({ project }: { project?: string }) {
  useSubscribe((fn) => session.subscribe(fn));
  const one = project ? session.project(project) : undefined;
  if (project && !one) return <Missing what="That project isn't one of yours (or it's gone)." />;
  return <DocList ps={one ? [one] : session.projects} narrowed={!!one} />;
}

function groupsOf(shown: Row[], narrowed: boolean): ListGroup<Row>[] {
  const by = new Map<string, ListGroup<Row> & { mine: boolean }>();
  for (const r of shown) {
    const g = by.get(r.p.id) ?? { title: r.mine ? "Yours, private" : narrowed ? undefined : r.p.name, items: [], mine: r.mine };
    g.items.push(r);
    by.set(r.p.id, g);
  }
  const out = [...by.values()];
  for (const g of out) g.items.sort((a, b) => b.doc.updated_at - a.doc.updated_at);
  // Their own project's first; the others as the session lists them.
  return out.sort((a, b) => Number(b.mine) - Number(a.mine));
}

function DocList({ ps, narrowed }: { ps: PlatformProject[]; narrowed: boolean }) {
  const { rows, sections } = useRows(ps);
  const [filter, setFilter] = useState("all");
  const [query, setQuery] = useState("");
  const [making, setMaking] = useState(false);
  const all = rows ?? [];
  const q = query.trim().toLowerCase();
  const shown = all.filter(
    (r) =>
      (filter === "all" || (filter === "private" && r.mine) || (filter === "shared" && !r.mine)) &&
      (!q || r.doc.title.toLowerCase().includes(q) || (sections.get(readKey(r.doc)) ?? []).some((s) => s.heading.toLowerCase().includes(q))),
  );
  const writable = ps.filter(mayWrite);
  const counts: ListCount[] = [
    { key: "all", n: all.length, label: all.length === 1 ? "Doc" : "Docs" },
    ...(narrowed ? [] : [{ key: "private", n: all.filter((r) => r.mine).length, label: "private" }, { key: "shared", n: all.filter((r) => !r.mine).length, label: "shared" }]),
  ];
  const status =
    rows === null ? (
      <p class="dim list-status" data-docs-loading>
        Reading the Docs…
      </p>
    ) : all.length === 0 ? (
      <p class="dim list-status" data-docs-none>
        No Docs yet.
      </p>
    ) : shown.length === 0 ? (
      <p class="dim list-status" data-docs-none>
        {q ? "No Doc's title or headings match." : filter === "private" ? "None of your Docs is private." : "No Doc is shared."}
      </p>
    ) : undefined;
  const sub = narrowed ? `${ps[0].name}'s reports, plans and write-ups, by its people and its agent sessions` : "Every Doc you can read, in your projects";
  return (
    <div class="docs-list" data-docs-page>
      <ListPage<Row>
        label="Docs"
        splitKey="platform-docs"
        header={
          <>
            <PageHeader
              title="Docs"
              sub={sub}
              actions={
                writable.length > 0 && (
                  <button type="button" class="secondary small" aria-expanded={making} data-doc-new-open onClick={() => setMaking(!making)}>
                    New Doc
                  </button>
                )
              }
              stats={all.length > 0 && <ListCounts counts={counts} on={filter} pick={(k) => setFilter(k === filter ? "all" : k)} />}
            />
            {making && <NewDoc ps={writable} done={() => setMaking(false)} />}
          </>
        }
        searchLabel="Search titles and headings"
        query={query}
        setQuery={setQuery}
        groups={groupsOf(shown, narrowed)}
        keyOf={(r) => r.doc.id}
        onOpen={(r) => go(docPath(r))}
        status={status}
        row={(r) => {
          const s = sections.get(readKey(r.doc));
          return {
            title: <span data-doc-title>{r.doc.title}</span>,
            age: <Time at={r.doc.updated_at} title="Updated" />,
            mark: s ? <Shape sections={s} /> : undefined,
            attrs: { "data-doc": r.doc.id, "data-private": String(r.mine) },
          };
        }}
        preview={(r) => <Preview key={r.doc.id} row={r} />}
      />
    </div>
  );
}

const docPath = (r: Row) => pathOf({ page: "doc", project: r.p.id, doc: r.doc.id }, names);

/** Make a Doc (a title) in one of the projects the reader may write, their own first. */
function NewDoc({ ps, done }: { ps: PlatformProject[]; done: () => void }) {
  const [title, setTitle] = useState("");
  const [project, setProject] = useState(ps[0].id);
  const make = async (e: Event) => {
    e.preventDefault();
    if (!title.trim()) return;
    try {
      const v = await projectDocs.create(project, { title: title.trim(), sections: [] });
      go(pathOf({ page: "doc", project, doc: v.doc.id }, names));
    } catch (err) {
      toast(`Couldn't make the Doc: ${(err as Error).message}`);
    }
  };
  return (
    <form class="project-doc-form" onSubmit={(e) => void make(e)}>
      <input type="text" value={title} maxLength={200} placeholder="a new Doc's title" aria-label="New Doc's title" data-doc-new-title onInput={(e) => setTitle(e.currentTarget.value)} />
      {ps.length > 1 && (
        <select value={project} aria-label="In which project" data-doc-new-project onChange={(e) => setProject(e.currentTarget.value)}>
          {ps.map((p) => (
            <option key={p.id} value={p.id}>
              {p.team ? p.name : "Yours, private"}
            </option>
          ))}
        </select>
      )}
      <button type="submit" class="primary" data-doc-new disabled={!title.trim()}>
        Make a Doc
      </button>
      <button type="button" onClick={done}>
        Cancel
      </button>
    </form>
  );
}

/** The selected Doc, whole, kept live through its thread; Open and Copy link above it. */
function Preview({ row }: { row: Row }) {
  useSubscribe((fn) => chat.subscribe(fn));
  const { doc, p } = row;
  const [view, setView] = useState<ProjectDocView | null>(null);
  const seen = useRef(0);
  useEffect(() => {
    const again = () => {
      const n = ++seen.current;
      projectDocs.get(p.id, doc.id).then(
        (v) => n === seen.current && setView(v),
        () => {},
      );
    };
    again();
    projectDocs.thread(doc.id).catch(() => {});
    const off = chat.onObject(doc.id, again);
    return off;
  }, [doc.id]);
  const state = useMemo(() => (view ? stateOf(view) : null), [view]);
  const to = docPath(row);
  const copy = copyFrom(doc);
  return (
    <ListPreview
      data-doc-preview={doc.id}
      title={doc.title}
      facts={
        <>
          <span>{row.mine ? "Yours, private" : p.name}</span>
          <span>
            {doc.written} of {doc.sections} written
          </span>
          <span>
            updated <Time at={doc.updated_at} on />
          </span>
          {copy ? <span data-doc-copy-fact>a copy from {copy}</span> : <span>made by {whoLabel(doc.author, doc.agent, nameOf(p))}</span>}
        </>
      }
      actions={
        <>
          <a class="button primary small" href={to} data-open-doc onClick={(e) => follow(e, to)}>
            Open
          </a>
          <CopyLinkButton link={new URL(to, location.href).href} />
        </>
      }
    >
      <div class="project-doc docs-preview">
        <DocBlock id={0} docKey={doc.id} s={state} fresh={NO_FRESH} load={noImages} />
      </div>
    </ListPreview>
  );
}
const NO_FRESH = new Map<string, number>();

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
  /** The boards that list this Doc (#517). */
  const [onBoards, setOnBoards] = useState<BoardOfDoc[]>([]);

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
    boards.ofDoc(project, doc).then(
      (b) => n === seen.current && setOnBoards(b.boards),
      () => n === seen.current && setOnBoards([]),
    );
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
  const copy = view ? copyFrom(view.doc) : null;
  // A copy has no writer, whatever an older platform says (#678).
  const writer = !!view?.writer && !copy;
  // A copy is the project's to delete (native #673); an older platform
  // sends no `deleter`, and then its `writer` says.
  const deleter = !!view && (view.deleter ?? view.writer);
  return (
    <section class="platform-section project-doc" data-doc-page={doc} data-writer={view ? String(writer) : undefined}>
      <p class="dim project-doc-head">
        <a href={all} onClick={(e) => follow(e, all)}>
          {p.name}'s Docs
        </a>
      </p>
      {view && copy && (
        <p class="dim project-doc-copy" data-doc-copy={view.doc.machine}>
          A copy of a Doc in {copy}'s library, updated <Time at={view.doc.updated_at} on />. It's read-only here: change it on {copy}, and its next copy brings the change.
        </p>
      )}
      {missing && !view && (
        <p class="dim" data-doc-missing>
          That Doc can't be read: {missing}.
        </p>
      )}
      {!missing || view ? (
        <DocBlock
          id={0}
          docKey={doc}
          s={state}
          fresh={fresh}
          load={noImages}
          share={<CopyLinkButton link={new URL(pathOf({ page: "doc", project, doc }, names), location.href).href} />}
          byline={
            view &&
            (copy ? (
              <>
                <span data-doc-maker>a copy from {copy}</span> · updated <Time at={view.doc.updated_at} />
              </>
            ) : (
              <>
                <span data-doc-maker>made by {whoLabel(view.doc.author, view.doc.agent, nameOf(p))}</span> · changed <Time at={view.doc.updated_at} />
              </>
            ))
          }
        />
      ) : null}
      {onBoards.length > 0 && (
        <p class="dim project-doc-boards" data-doc-boards>
          On boards:{" "}
          {onBoards.map((b, i) => {
            const to = pathOf({ page: "board", project, board: b.board }, names);
            return (
              <span key={b.board}>
                {i > 0 && ", "}
                <a href={to} onClick={(e) => follow(e, to)} data-doc-board={b.board}>
                  {b.title}
                </a>{" "}
                ({onBoardWords(b)})
              </span>
            );
          })}
        </p>
      )}
      {view && writer && <DocWriter project={project} view={view} wrote={show} />}
      {view && copy && deleter && <CopyDeleter project={project} view={view} machine={copy} mine={view.doc.author === session.me?.account} />}
    </section>
  );
}

/** For owners and editors: delete a copy from the project (native #673).
 * The machine's next change to the Doc brings a new copy: the warning
 * says so, and that the machine is theirs when it is. */
function CopyDeleter({ project, view, machine, mine }: { project: string; view: ProjectDocView; machine: string; mine: boolean }) {
  const [gone, setGone] = useState(false);
  const remove = async () => {
    try {
      await projectDocs.remove(project, view.doc.id);
      go(pathOf({ page: "docs", project }, names));
    } catch (err) {
      toast(`Couldn't delete it: ${(err as Error).message}`);
    }
  };
  return (
    <div class="project-doc-form project-doc-copy-delete" data-doc-copy-delete>
      <button type="button" class="secondary danger" onClick={() => setGone(true)} data-doc-delete>
        Delete the copy
      </button>
      {gone && (
        <p class="project-doc-confirm" data-doc-confirm>
          Delete “{view.doc.title}” from the project? {mine ? `${machine} is your machine: its` : `${machine}'s`} next change to the Doc brings a new copy here.{" "}
          <button type="button" class="danger" data-doc-confirm-delete onClick={() => void remove()}>
            Delete
          </button>{" "}
          <button type="button" onClick={() => setGone(false)}>
            Keep
          </button>
        </p>
      )}
    </div>
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
      <SectionHead title="Docs" all={all} onAll={(e) => follow(e, all)} data="project-docs-link" />
      {list.length === 0 && <p class="dim">No Docs yet.</p>}
      <div data-project-docs>
        <DocRows p={p} list={list.slice(0, 5)} />
      </div>
    </>
  );
}
