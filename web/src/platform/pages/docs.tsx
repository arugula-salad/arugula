// `/docs`, `/p/:project/docs` and `/p/:project/docs/:doc` (aru-118, #418): a
// project's Docs, on the one Docs list (Arugula #864, shared/docs/docs-list.tsx),
// and one Doc. A personal project's list is My Docs; a team's has My Docs and
// Team Docs. `/docs` is the current project's. A project Doc is the
// platform's (crates/platform/src/project_docs.rs): a title and sections of
// Markdown, in the machine Doc's shape, so it's drawn with the Doc block's
// renderer (charts and drawings included). Its members read it; owners and
// editors make, write and delete Docs (a viewer's page has no New Doc, writer
// or Delete); an agent session on a Cloud workspace makes them too, as
// itself. A write anywhere reaches an open page live, through the Doc's
// thread (as a board's does).

import { Card } from "../../shared/ui/card";
import { BlockLoading, EmptyLine, EmptyState } from "../../shared/ui/block-states";
import { useEffect, useMemo, useRef, useState } from "preact/hooks";
import type { BoardOfDoc, ProjectDoc, ProjectDocTrashed, ProjectDocView } from "../../shared/proto";
import { chat } from "../../shared/chat-store";
import { DocError, projectDocs } from "../../shared/docs/store";
import { boards } from "../../shared/boards/store";
import { onBoardWords } from "../../shared/boards/text";
import { whoLabel } from "../../shared/issues/text";
import { useSubscribe } from "../../shared/ui/hooks";
import { ListCounts, ListPreview, type ListTicked } from "../../shared/ui/list-page";
import { Modal } from "../../shared/ui/modal";
import { Time } from "../../shared/ui/time";
import { DocBlock } from "../../shared/docs/doc-render";
import { DocsList, goToSection, Outline } from "../../shared/docs/docs-list";
import { modeOf } from "../../shared/docs/docs-mode";
import { noImages, projectItem, shapeOf, stateOf, useProjectDoc, useProjectDocs } from "../../shared/docs/project-docs";
import { moveDocsToTeam, resultWords, type MoveResult } from "../../shared/docs/move-docs";
import "../../shared/docs/docs.css";
import { names } from "../links";
import { follow, go, pathOf } from "../router";
import { SectionHead } from "../../shared/ui/section-head";
import { session, type PlatformProject } from "../session";
import { toast } from "../toast";
import { mayWrite } from "./issues";
import { currentProject, onCurrentProject } from "../current-project";
import { scopeOf } from "../scope";
import { Missing } from "./project";
import { MoveConfirm, PlatformDocActions, RowMenu, TRASH_DAYS } from "./doc-actions";
import { PublicLinkBar } from "./doc-public";

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
    <ul class="platform-list card-list" data-docs>
      {list.map((d) => {
        const to = pathOf({ page: "doc", project: p.id, doc: d.id }, names);
        const copy = copyFrom(d);
        return (
          <Card as="li" dense key={d.id}>
            <a href={to} data-doc-link={d.id} onClick={(e) => follow(e, to)}>
              {d.title}
            </a>
            <span class="dim">
              {" "}
              {copy ? `a copy from ${copy}` : whoLabel(d.author, d.agent, who)} · <Time at={d.updated_at} />
            </span>
          </Card>
        );
      })}
    </ul>
  );
}

/** `/docs` is the current project's Docs, else the reader's own: one
 * project's, as `/p/:project/docs` is (Arugula #864). A team's project has
 * My Docs and Team Docs; a personal one, My Docs alone. */
export function DocsPage({ project }: { project?: string; all?: boolean }) {
  useSubscribe((fn) => session.subscribe(fn));
  useSubscribe((fn) => onCurrentProject(fn));
  const one = project ? session.project(project) : undefined;
  if (project && !one) return <Missing what="That project isn't one of yours (or it's gone)." />;
  const at = project ? undefined : scopeOf({ page: "docs" }, currentProject(), (id) => !!session.project(id));
  const p = one ?? (at ? session.project(at) : undefined) ?? session.projects.find((x) => modeOf(x).kind === "personal") ?? session.projects[0];
  if (!p) return <EmptyState where="page" data-docs-none title="No Docs yet." why="Ask any agent for a Doc (a write-up, a plan, a comparison) and it keeps it here." />;
  return <DocList key={p.id} p={p} />;
}

const docPath = (p: PlatformProject, d: ProjectDoc) => pathOf({ page: "doc", project: p.id, doc: d.id }, names);

function DocList({ p }: { p: PlatformProject }) {
  const mode = modeOf(p);
  const me = session.me?.account;
  const { docs, reload } = useProjectDocs(p.id);
  const [making, setMaking] = useState(false);
  const [trashOpen, setTrashOpen] = useState(false);
  const writable = mayWrite(p);
  // A personal project's Docs can be put in a team (Arugula #818); a team's are there already.
  const teams = session.projects.filter((t) => t.team && mayWrite(t));
  return (
    <div class="docs-list docs-page" data-docs-page data-docs-mode={mode.kind}>
      <DocsList<ProjectDoc>
        mode={mode}
        docs={docs}
        item={(d) => projectItem(d, me)}
        splitKey="platform-docs"
        sub={mode.kind === "team" ? `${p.name}'s reports, plans and write-ups, by its people and its agent sessions` : undefined}
        actions={
          writable && (
            <button type="button" class="secondary small" data-doc-new-open onClick={() => setMaking(true)}>
              New Doc
            </button>
          )
        }
        stats={(shown) => (
          <>
            {docs !== null && <ListCounts counts={[{ key: "all", n: shown.length, label: shown.length === 1 ? "Doc" : "Docs" }]} on="all" pick={() => {}} />}
            <button type="button" class="link docs-trash-open" aria-expanded={trashOpen} data-docs-trash-open onClick={() => setTrashOpen(!trashOpen)}>
              Trash
            </button>
          </>
        )}
        under={
          <>
            {making && <NewDoc p={p} done={() => setMaking(false)} />}
            {trashOpen && <Trash p={p} restored={reload} />}
          </>
        }
        loading={<BlockLoading inline data-docs-loading what="the Docs" />}
        empty={<EmptyState where="page" data-docs-none title="No Docs yet." why="Ask any agent for a Doc (a write-up, a plan, a comparison) and it keeps it here." />}
        selectable={() => mode.kind === "personal" && teams.length > 0}
        selection={(ticked) => <ShareBar p={p} ticked={ticked} teams={teams} reload={reload} />}
        onOpen={(d) => go(docPath(p, d))}
        row={(d) => ({
          detail: mode.kind === "personal" && copyFrom(d) ? <span data-doc-machine>On {copyFrom(d)}</span> : undefined,
          actions: <RowMenu p={p} doc={d} reload={reload} />,
          attrs: { "data-private": String(mode.kind === "personal") },
        })}
        preview={(d) => <Preview key={d.id} p={p} doc={d} reload={reload} />}
      />
    </div>
  );
}

/** The project's Trash (Arugula #866): each deleted Doc's title, who deleted it, when, the days it has left, and Restore for those who may. */
function Trash({ p, restored }: { p: PlatformProject; restored: () => void }) {
  const [trash, setTrash] = useState<{ docs: ProjectDocTrashed[]; days: number } | null>(null);
  const [busy, setBusy] = useState("");
  const who = nameOf(p);
  const load = () =>
    projectDocs.trash(p.id).then(
      (t) => setTrash({ docs: t.docs, days: t.days }),
      () => setTrash({ docs: [], days: TRASH_DAYS }),
    );
  useEffect(() => {
    setTrash(null);
    void load();
  }, [p.id]);
  const restore = async (d: ProjectDocTrashed) => {
    setBusy(d.doc.id);
    try {
      await projectDocs.restore(p.id, d.doc.id);
      restored();
      await load();
    } catch (err) {
      toast(`Couldn't restore it: ${(err as Error).message}`);
    } finally {
      setBusy("");
    }
  };
  if (trash === null) return <BlockLoading inline what="the Trash" />;
  if (trash.docs.length === 0) return <EmptyLine data-docs-trash-empty>The Trash is empty. A deleted Doc waits here for {trash.days} days.</EmptyLine>;
  return (
    <ul class="platform-list card-list" data-docs-trash>
      {trash.docs.map((d) => {
        const left = Math.max(0, Math.ceil(trash.days - (Date.now() - d.trashed_at) / 86_400_000));
        return (
          <Card as="li" dense key={d.doc.id} data-trashed-doc={d.doc.id}>
            <span data-doc-title>{d.doc.title}</span>
            <span class="dim">
              {" "}
              deleted by {d.trashed_by ? (who(d.trashed_by) ?? "a member") : "someone who left"} · <Time at={d.trashed_at} /> · {left} {left === 1 ? "day" : "days"} left
            </span>
            {d.restorable && (
              <button type="button" class="secondary small" disabled={busy === d.doc.id} data-doc-restore onClick={() => void restore(d)}>
                Restore
              </button>
            )}
          </Card>
        );
      })}
    </ul>
  );
}

/** The bar over a personal project's rows while any is ticked: move them to a team, once asked, and what became of each. */
function ShareBar({ p, ticked, teams, reload }: { p: PlatformProject; ticked: ListTicked<ProjectDoc>; teams: PlatformProject[]; reload: () => void }) {
  const [team, setTeam] = useState(teams[0]?.id ?? "");
  const [run, setRun] = useState<{ done: number; of: number } | null>(null);
  const [results, setResults] = useState<{ team: PlatformProject; list: MoveResult[] } | null>(null);
  const to = teams.find((t) => t.id === team) ?? teams[0];
  const n = ticked.items.length;
  const [asking, setAsking] = useState(false);
  const share = async () => {
    setAsking(false);
    if (run) return;
    const docs = ticked.items;
    setResults(null);
    setRun({ done: 0, of: docs.length });
    const list = await moveDocsToTeam(p.id, to.id, docs, (_, done, of) => setRun({ done, of }));
    setRun(null);
    setResults({ team: to, list });
    ticked.clear();
    reload();
  };
  if (!run && !results && n === 0) return null;
  return (
    <div class="docs-share-bar" data-docs-share-bar>
      {run ? (
        <span data-docs-share-progress role="status">
          Moving {run.done} of {run.of}…
        </span>
      ) : (
        n > 0 && (
          <>
            <b data-docs-selected>{n} selected</b>
            {teams.length > 1 && (
              <select class="field small" value={to.id} aria-label="Move to which team" data-docs-share-team onChange={(e) => setTeam(e.currentTarget.value)}>
                {teams.map((t) => (
                  <option key={t.id} value={t.id}>
                    {t.name}
                  </option>
                ))}
              </select>
            )}
            <button type="button" class="primary small" data-docs-share onClick={() => setAsking(true)}>
              {`Move to ${to.name}…`}
            </button>
            <button type="button" class="link" data-docs-clear onClick={ticked.clear}>
              Clear
            </button>
          </>
        )
      )}
      {asking && <MoveConfirm title={ticked.items[0]?.title ?? ""} team={to.name} n={n} go={() => void share()} cancel={() => setAsking(false)} />}
      {results && !run && (
        <ul class="docs-share-results" data-docs-share-results>
          {results.list.map((r) => {
            const link = r.teamDoc && pathOf({ page: "doc", project: results.team.id, doc: r.teamDoc }, names);
            return (
              <li key={r.id} data-move-result={r.outcome}>
                <span class="docs-share-title">{r.title}</span>
                {r.outcome === "refused" ? (
                  <span class="dim">Refused: {r.why}</span>
                ) : (
                  <a href={link!} data-move-link onClick={(e) => follow(e, link!)}>
                    {resultWords(r, results.team.name)}
                  </a>
                )}
              </li>
            );
          })}
          <li>
            <button type="button" class="link" data-docs-share-done onClick={() => setResults(null)}>
              Done
            </button>
          </li>
        </ul>
      )}
    </div>
  );
}

/** New Doc: a title, in the page's project, then its page to write it. */
function NewDoc({ p, done }: { p: PlatformProject; done: () => void }) {
  const [title, setTitle] = useState("");
  const [busy, setBusy] = useState(false);
  const make = async (e: Event) => {
    e.preventDefault();
    if (!title.trim() || busy) return;
    setBusy(true);
    try {
      const v = await projectDocs.create(p.id, { title: title.trim(), sections: [] });
      done();
      go(pathOf({ page: "doc", project: p.id, doc: v.doc.id }, names));
    } catch (err) {
      toast(`Couldn't make the Doc: ${(err as Error).message}`);
      setBusy(false);
    }
  };
  return (
    <Modal
      title="New Doc"
      close={done}
      onSubmit={(e) => void make(e)}
      root={{ "data-doc-new-dialog": true }}
      buttons={
        <>
          <button type="button" class="secondary" onClick={done}>
            Cancel
          </button>
          <button type="submit" class="primary" data-doc-new disabled={busy || !title.trim()}>
            Make a Doc
          </button>
        </>
      }
    >
      <input class="field" type="text" value={title} maxLength={200} placeholder="Its title" aria-label="New Doc's title" data-doc-new-title autoFocus onInput={(e) => setTitle(e.currentTarget.value)} />
    </Modal>
  );
}

/** The selected Doc, whole, kept live through its thread: Open and the actions above it, its outline beside it. */
function Preview({ p, doc, reload }: { p: PlatformProject; doc: ProjectDoc; reload: () => void }) {
  const view = useProjectDoc(p.id, doc.id);
  const state = useMemo(() => (view ? stateOf(view) : null), [view]);
  const reader = useRef<HTMLDivElement>(null);
  const copy = copyFrom(doc);
  const team = modeOf(p).kind === "team";
  return (
    <ListPreview
      data-doc-preview={doc.id}
      facts={copy && <span data-doc-copy-fact>a copy from {copy}</span>}
      factsTip={`${team ? p.name : "Yours, private"} · ${doc.written} of ${doc.sections} written${copy ? "" : ` · made by ${whoLabel(doc.author, doc.agent, nameOf(p))}`}`}
      actions={
        <>
          <button type="button" class="primary small" data-open-doc onClick={() => go(docPath(p, doc))}>
            Open
          </button>
          <PlatformDocActions p={p} doc={doc} reload={reload} menu deleter={mayWrite(p)} gone={reload} />
        </>
      }
      aside={view && shapeOf(view.sections).length > 0 && <Outline sections={shapeOf(view.sections)} go={(i) => goToSection(reader.current, i)} />}
    >
      <div ref={reader} class="docs-reader" data-doc-reader>
        <DocBlock id={0} docKey={doc.id} s={state} fresh={NO_FRESH} load={noImages} />
      </div>
    </ListPreview>
  );
}
const NO_FRESH = new Map<string, number>();

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
  const copy = view ? copyFrom(view.doc) : null;
  // A copy has no writer, whatever an older platform says (#678).
  const writer = !!view?.writer && !copy;
  // A copy is the project's to delete (native #673); an older platform
  // sends no `deleter`, and then its `writer` says. Delete is in ⋯.
  const deleter = !!view && (view.deleter ?? view.writer);
  return (
    <section class="platform-section project-doc" data-doc-page={doc} data-writer={view ? String(writer) : undefined}>
      {view && copy && (
        <p class="dim project-doc-copy" data-doc-copy={view.doc.machine}>
          Copy from {copy} · <Time at={view.doc.updated_at} />
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
          link="page"
          share={view && <PlatformDocActions p={p} doc={view.doc} menu deleter={deleter} gone={() => go(pathOf({ page: "docs", project }, names))} />}
          byline={
            // A copy's line above the Doc already says where it's from and when.
            view &&
            !copy && (
              <>
                <span data-doc-maker>{whoLabel(view.doc.author, view.doc.agent, nameOf(p))}</span>
              </>
            )
          }
        />
      ) : null}
      {view && !copy && <PublicLinkBar project={project} view={view} reload={again} />}
      {onBoards.length > 0 && (
        <p class="dim doc-lead" data-doc-boards>
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
    </section>
  );
}

/** For owners and editors: write a section (its Markdown replaced), or add one. */
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
  const ready = section === NEW ? !!heading.trim() : markdown !== (current?.body ?? "");
  return (
    <form class="doc-lead doc-writer" onSubmit={(e) => void save(e)} data-doc-writer>
      <select class="field" value={section} aria-label="Which section to write" onChange={(e) => pick(e.currentTarget.value)} data-doc-section-pick>
        {view.sections.map((s) => (
          <option key={s.id} value={s.id}>
            {s.heading}
            {s.body === null ? " (pending)" : ""}
          </option>
        ))}
        <option value={NEW}>A new section at the end…</option>
      </select>
      {section === NEW && <input class="field" type="text" value={heading} maxLength={200} placeholder="Its heading" aria-label="The new section's heading" data-doc-heading-input onInput={(e) => setHeading(e.currentTarget.value)} />}
      <textarea class="field" rows={8} value={markdown} placeholder="Markdown: lists, tables, ```chart and ```svg fences" aria-label="The section's Markdown" data-doc-markdown-input onInput={(e) => setMarkdown(e.currentTarget.value)} />
      <button type="submit" class="primary" disabled={busy || !ready} data-doc-save>
        {section === NEW ? "Add the section" : "Save the section"}
      </button>
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
      {list.length === 0 && <EmptyState where="page" title="No Docs yet." why="Ask any agent for a Doc (a write-up, a plan, a comparison) and it keeps it here." />}
      <div data-project-docs>
        <DocRows p={p} list={list.slice(0, 5)} />
      </div>
    </>
  );
}
