// `/p/:project/docs` and `/p/:project/docs/:doc` (aru-118, #418): a
// project's Docs, and one Doc. The list (Lift 6b, #1484) is one list of kit
// Rows, the latest changed first, each its title, who made it and when; a
// team's has a Mine filter (the reader's and their agents'). `j`/`k` move,
// ↵ opens, `m` is Mine. `/docs` goes to the current project's (Go to…
// reaches the others). A project Doc is the platform's
// (crates/platform/src/project_docs.rs): a title and sections of Markdown,
// in the machine Doc's shape, so it's drawn with the Doc block's renderer
// (charts and drawings included). Its members read it; owners and editors
// make, write and delete Docs (a viewer's page has no New Doc, writer or
// Delete); an agent session on a Cloud workspace makes them too, as itself.
// A write anywhere reaches an open page live, through the Doc's thread (as
// a board's does).
//
// A personal project's Docs are kept in the reader's cloud, not on a
// machine (Arugula #865): a line above the list says so; a Doc's ⋯ moves it
// to a team or downloads it. At its foot, Shared by you: the Docs the
// reader shared with a link from a machine (Arugula #1315,
// ./shared-by-you.tsx).
//
// A team's deleted Docs wait in its trash for `TRASH_DAYS` (Arugula #866):
// a folded Deleted row at the list's foot, there only when there are any
// (#1133). Each opens in place with who deleted it, when, the days it has
// left, and Restore for those who may.

import type { ComponentChildren } from "preact";
import { Card } from "../../shared/ui/card";
import { BlockLoading, EmptyLine, EmptyState } from "../../shared/ui/block-states";
import { useEffect, useMemo, useRef, useState } from "preact/hooks";
import type { BoardOfDoc, ProjectDoc, ProjectDocTrash, ProjectDocTrashed, ProjectDocView } from "../../shared/proto";
import { chat } from "../../shared/chat-store";
import { DocError, projectDocs } from "../../shared/docs/store";
import { TITLE_MAX, titleRefusal } from "../../shared/docs/doc-title.ts";
import { boards } from "../../shared/boards/store";
import { onBoardWords } from "../../shared/boards/text";
import { whoLabel } from "../../shared/issues/text";
import { useSubscribe } from "../../shared/ui/hooks";
import { Modal } from "../../shared/ui/modal";
import { Time } from "../../shared/ui/time";
import { DocBlock } from "../../shared/docs/doc-render";
import { DocComments } from "../../shared/docs/doc-comments";
import { reachOf } from "../../shared/docs/doc-reach";
import { LayoutMark } from "../../shared/docs/layout-mark";
import { madeBy, modeOf } from "../../shared/docs/docs-mode";
import { stateOf, useProjectDocs, useProjectImages } from "../../shared/docs/project-docs";
import { both, useBindings } from "../../shared/keys";
import { Button } from "../../shared/kit/button";
import { Choice } from "../../shared/kit/field";
import { Row } from "../../shared/kit/row";
import { Surface } from "../../shared/kit/surface";
import "../../shared/docs/docs.css";
import { homes, names } from "../links";
import { follow, go, pathOf } from "../router";
import { SectionHead } from "../../shared/ui/section-head";
import { session, type PlatformProject } from "../session";
import { toast } from "../toast";
import { mayWrite } from "./issues";
import { currentProject, onCurrentProject } from "../current-project";
import { scopeOf } from "../scope";
import { Missing } from "./project";
import { PlatformDocActions, TRASH_DAYS } from "./doc-actions";
import { PublicLinkBar } from "./doc-public";
import { SharedByYou } from "./shared-by-you";

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
    <ul class="platform-list card-list" data-docs>
      {list.map((d) => {
        const to = pathOf({ page: "doc", project: p.id, doc: d.id }, names);
        return (
          <Card as="li" dense key={d.id}>
            <a href={to} data-doc-link={d.id} onClick={(e) => follow(e, to)}>
              {d.title}
            </a>
            <span class="dim">
              {" "}
              {whoLabel(d.author, d.agent, who)} · <Time at={d.updated_at} />
            </span>
          </Card>
        );
      })}
    </ul>
  );
}

/** `/docs` goes to the current project's Docs, else the reader's own
 * (Lift 6b: one project's, never all of them); `?shared=1`, control's
 * Shared Docs… (Arugula #1315), to the reader's own, at Shared by you. */
export function DocsPage({ project, shared }: { project?: string; all?: boolean; shared?: boolean }) {
  useSubscribe((fn) => session.subscribe(fn));
  useSubscribe((fn) => onCurrentProject(fn));
  const one = project ? session.project(project) : undefined;
  const at = project || shared ? undefined : scopeOf({ page: "docs" }, currentProject(), (id) => !!session.project(id));
  const p = one ?? (at ? session.project(at) : undefined) ?? session.projects.find((x) => modeOf(x).kind === "personal") ?? session.projects[0];
  const to = !project && !shared && p ? pathOf({ page: "docs", project: p.id }, names) : null;
  useEffect(() => void (to && go(to, true)), [to]);
  if (project && !one) return <Missing what="That project isn't one of yours (or it's gone)." />;
  if (!p) return <EmptyState where="page" data-docs-none title="No Docs yet." why="Ask any agent for a Doc (a write-up, a plan, a comparison) and it keeps it here." />;
  if (to) return null;
  return <DocList key={p.id} p={p} shared={shared} />;
}

const docPath = (p: PlatformProject, d: ProjectDoc) => pathOf({ page: "doc", project: p.id, doc: d.id }, names);

/** A team project's deleted Docs, read again with each read of its Docs (`docs`): none in a personal one, or when they can't be read. */
function useTrash(p: PlatformProject, docs: ProjectDoc[] | null): ProjectDocTrash | null {
  const [trash, setTrash] = useState<ProjectDocTrash | null>(null);
  useEffect(() => {
    if (!p.team || docs === null) return;
    let live = true;
    projectDocs.trash(p.id).then(
      (t) => live && setTrash(t),
      () => live && setTrash(null),
    );
    return () => void (live = false);
  }, [p.id, docs]);
  return p.team ? trash : null;
}

type Show = "all" | "mine";

function DocList({ p, shared }: { p: PlatformProject; shared?: boolean }) {
  const mode = modeOf(p);
  const team = mode.kind === "team";
  const me = session.me?.account;
  const { docs, reload } = useProjectDocs(p.id);
  const trash = useTrash(p, docs);
  const [making, setMaking] = useState(false);
  const [show, setShow] = useState<Show>("all");
  const [picked, setPicked] = useState<string | null>(null);
  const [opened, setOpened] = useState<string | null>(null);
  const [folded, setFolded] = useState(true);
  const writable = mayWrite(p);
  const teams = session.projects.filter((t) => t.team && mayWrite(t));
  // Said only when there's somewhere to move them: with no team, Download is all there is.
  const cloud = mode.kind === "personal" && teams.length > 0 && !!docs?.length;
  const who = nameOf(p);
  const rows = [...(docs ?? [])].filter((d) => !team || show === "all" || madeBy(d, me)).sort((a, b) => b.updated_at - a.updated_at);
  const gone = trash?.docs ?? [];
  const now = useRef({ rows, picked });
  now.current = { rows, picked };
  const step = (by: number) => {
    const { rows, picked } = now.current;
    if (!rows.length) return;
    const i = rows.findIndex((d) => d.id === picked);
    const next = rows[i === -1 ? (by > 0 ? 0 : rows.length - 1) : Math.max(0, Math.min(rows.length - 1, i + by))];
    setPicked(next.id);
    document.querySelector<HTMLElement>(`[data-doc="${next.id}"] > .k-row-line`)?.focus();
  };
  useBindings(
    [
      { id: "docs.next", label: "The next Doc", chip: "move", keys: both("key:j", "ArrowDown"), hint: { mac: "j k", other: "j k" }, run: () => step(1) },
      { id: "docs.prev", label: "The Doc before", keys: both("key:k", "ArrowUp"), run: () => step(-1) },
      {
        id: "docs.open",
        label: "Open the Doc",
        chip: "open",
        keys: both("Enter"),
        when: () => !!now.current.picked && !document.activeElement?.closest("button:not(.k-row-line), a, input, textarea, select"),
        run: () => {
          const d = now.current.rows.find((x) => x.id === now.current.picked);
          if (d) go(docPath(p, d));
        },
      },
      ...(team ? [{ id: "docs.mine", label: "Mine or all", chip: "mine", keys: both("key:m"), run: () => setShow((s) => (s === "mine" ? "all" : "mine")) }] : []),
    ],
    [p.id, team],
  );
  const empty = <EmptyState where="page" data-docs-none title="No Docs yet." why="Ask any agent for a Doc (a write-up, a plan, a comparison) and it keeps it here." />;
  return (
    <div class="docs-place" data-docs-page data-docs-mode={mode.kind} data-work>
      <header class="docs-head">
        <div class="docs-head-words">
          <h1 class="docs-title">Docs</h1>
          {docs !== null && (
            <p class="docs-sub m-meta" data-docs-count={rows.length}>
              {rows.length} in {p.name}
            </p>
          )}
        </div>
        {team && <Choice label="Whose Docs" value={show} onChange={setShow} options={SHOW} />}
        {writable && (
          <Button kind="primary" data-doc-new-open onClick={() => setMaking(true)}>
            New Doc
          </Button>
        )}
      </header>
      {cloud && (
        <p class="docs-cloud-line" data-docs-cloud-line>
          These Docs are kept in your cloud. A Doc's ⋯ moves it to a team, or downloads it.
        </p>
      )}
      {making && <NewDoc p={p} done={() => setMaking(false)} />}
      {docs === null ? (
        <BlockLoading inline data-docs-loading what="the Docs" />
      ) : docs.length === 0 && gone.length === 0 ? (
        empty
      ) : (
        <Surface kind="section" class="docs-rows" data-docs>
          {rows.length === 0 && docs.length > 0 && <EmptyLine data-docs-none>{`None of ${p.name}'s Docs is yours or your agents' yet.`}</EmptyLine>}
          {rows.map((d) => (
            <Row
              key={d.id}
              kind={picked === d.id ? "selected" : "plain"}
              title={
                <>
                  <LayoutMark layout={d.layout} />
                  <span data-doc-title>{d.title}</span>
                </>
              }
              meta={[whoLabel(d.author, d.agent, who), <Time key="t" at={d.updated_at} />]}
              onClick={() => (setPicked(d.id), go(docPath(p, d)))}
              data-doc={d.id}
              data-mine={String(madeBy(d, me))}
              data-private={String(mode.kind === "personal")}
            />
          ))}
          {gone.length > 0 && (
            <Row title={`Deleted · ${gone.length}`} kind="done" open={!folded} onClick={() => setFolded(!folded)} data-docs-deleted="">
              {gone.map((t) => (
                <Row key={t.doc.id} kind="done" title={t.doc.title} meta={[<Time key="t" at={t.trashed_at} />]} open={opened === t.doc.id} onClick={() => setOpened(opened === t.doc.id ? null : t.doc.id)} data-trashed-doc={t.doc.id}>
                  <Deleted p={p} t={t} days={trash?.days ?? TRASH_DAYS} restored={reload} />
                </Row>
              ))}
            </Row>
          )}
        </Surface>
      )}
      {/* The links are the account's, not a team's: under its own Docs. */}
      {mode.kind === "personal" && <SharedByYou focus={shared} />}
    </div>
  );
}

const SHOW = [
  { id: "all", label: "All" },
  { id: "mine", label: "Mine" },
] as const;

/** A deleted Doc, opened in its row (Arugula #1133): who deleted it, when, the days it has left, and Restore for those who may. */
function Deleted({ p, t, days, restored }: { p: PlatformProject; t: ProjectDocTrashed; days: number; restored: () => void }) {
  const [busy, setBusy] = useState(false);
  const left = Math.max(0, Math.ceil(days - (Date.now() - t.trashed_at) / 86_400_000));
  const by = t.trashed_by ? (nameOf(p)(t.trashed_by) ?? "a member") : "someone who left";
  const restore = async () => {
    setBusy(true);
    try {
      await projectDocs.restore(p.id, t.doc.id);
      restored();
    } catch (err) {
      toast(`Couldn't restore it: ${(err as Error).message}`);
      setBusy(false);
    }
  };
  return (
    <div class="docs-deleted" data-doc-deleted={t.doc.id}>
      <p class="m-meta">
        Deleted by {by} · <Time at={t.trashed_at} /> · {left} {left === 1 ? "day" : "days"} left
      </p>
      <p data-doc-deleted-why>
        {left === 0 ? "It goes for good today." : `It goes for good in ${left} ${left === 1 ? "day" : "days"}.`} {t.restorable ? "Restore brings it back whole, where it was." : "An owner, an editor or its maker can restore it."}
      </p>
      {t.restorable && (
        <Button kind="primary" disabled={busy} data-doc-restore onClick={() => void restore()}>
          Restore
        </Button>
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
  /** Opens the title's editor in place (#979), for ⋯'s Rename: TitleEdit's own. */
  const rename = useRef<(() => void) | null>(null);

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
      (v) => {
        if (n !== seen.current) return;
        // A Doc moved to another project (Arugula #972): the page's address goes to its new one.
        if (v.doc.project !== project) return homes.set(doc, v.doc.project);
        show(v);
      },
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
  const images = useProjectImages(project, doc);
  if (!p) return <Missing what="That project isn't one of yours (or it's gone)." />;
  const writer = !!view?.writer;
  // An older platform sends no `deleter`, and then its `writer` says.
  // Delete is in ⋯.
  const deleter = !!view && (view.deleter ?? view.writer);
  return (
    <section class="platform-section project-doc" data-doc-page={doc} data-writer={view ? String(writer) : undefined}>
      {missing && !view && (
        <p class="dim" data-doc-missing>
          That Doc can't be read: {missing}.
        </p>
      )}
      {!missing || view ? (
        <Commented project={project} view={view}>
        <DocBlock
          id={0}
          docKey={doc}
          s={state}
          fresh={fresh}
          load={images.load}
          cached={images.cached}
          link="page"
          titleView={view && writer ? <TitleEdit project={project} view={view} opener={rename} wrote={show} /> : undefined}
          share={view && <PlatformDocActions p={p} doc={view.doc} menu deleter={deleter} gone={() => go(pathOf({ page: "docs", project }, names))} undone={() => go(pathOf({ page: "doc", project, doc }, names))} writer={writer} edit={() => rename.current?.()} />}
          byline={
            view && (
              <>
                <span data-doc-maker>{whoLabel(view.doc.author, view.doc.agent, nameOf(p))}</span>
              </>
            )
          }
        />
        </Commented>
      ) : null}
      {view && <PublicLinkBar project={project} view={view} reload={again} />}
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

/** The Doc with its comments in the margin (#1258), once it's read. */
function Commented({ project, view, children }: { project: string; view: ProjectDocView | null; children: ComponentChildren }) {
  const headings = useMemo(() => new Map(view?.sections.map((s) => [s.id, s.heading]) ?? []), [view]);
  if (!view) return <>{children}</>;
  const reach = reachOf(view.doc);
  return (
    <DocComments doc={view.doc.id} url={`${location.origin}/p/${project}/docs/${view.doc.id}`} headings={headings} mention={reach.mention} reaches={reach.label}>
      {children}
    </DocComments>
  );
}

/** For owners and editors: the title, as a button that edits it in place (#979).
 * Enter saves, Escape cancels and leaving the field saves; a title the platform
 * refuses stays in the field with its reason. */
function TitleEdit({ project, view, opener, wrote }: { project: string; view: ProjectDocView; opener: { current: (() => void) | null }; wrote: (v: ProjectDocView) => void }) {
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState(view.doc.title);
  const [why, setWhy] = useState<string | null>(null);
  const input = useRef<HTMLInputElement | null>(null);
  // Set when Escape or a save has settled it, so the blur that follows doesn't save again.
  const settled = useRef(false);
  // The draft is set with the editor's opening, not after it's drawn, so a key
  // typed at once isn't overwritten by the old title (Arugula #991).
  const open = () => {
    settled.current = false;
    setDraft(view.doc.title);
    setWhy(null);
    setEditing(true);
  };
  opener.current = open;
  // The field has the keys, its title selected, as it's put in the page.
  const mounted = useMemo(
    () => (el: HTMLInputElement | null) => {
      input.current = el;
      el?.focus();
      el?.select();
    },
    [],
  );
  const stop = () => {
    settled.current = true;
    setEditing(false);
  };
  const save = async () => {
    if (settled.current) return;
    const t = draft.trim();
    if (t === view.doc.title) return stop();
    const bad = titleRefusal(draft);
    if (bad) return setWhy(bad);
    settled.current = true;
    try {
      wrote(await projectDocs.rename(project, view.doc.id, t));
      setEditing(false);
    } catch (err) {
      settled.current = false;
      setWhy((err as Error).message);
      input.current?.focus();
    }
  };
  if (!editing)
    return (
      <button type="button" class="doc-title-edit" title="Rename" data-doc-title-edit onClick={open}>
        {view.doc.title || "Untitled"}
      </button>
    );
  return (
    <>
      <input
        ref={mounted}
        class="field doc-title-input"
        type="text"
        value={draft}
        maxLength={TITLE_MAX}
        aria-label="The Doc's title"
        aria-invalid={why ? "true" : undefined}
        data-doc-title-input
        onInput={(e) => {
          setDraft(e.currentTarget.value.replace(/[\r\n]/g, " "));
          setWhy(null);
        }}
        onKeyDown={(e) => {
          if (e.key === "Enter") {
            e.preventDefault();
            void save();
          } else if (e.key === "Escape") {
            e.preventDefault();
            e.stopPropagation();
            stop();
          }
        }}
        onBlur={() => void save()}
      />
      {why && (
        <span class="dim doc-title-why" role="alert" data-doc-title-why>
          {why}
        </span>
      )}
    </>
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
