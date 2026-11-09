// A Doc's actions on the platform (Arugula #820): the same Copy link, Share
// and Open in browser the machine's Doc bar has (shared/docs/doc-actions.tsx),
// in the Docs list's preview, in a row's ⋯ menu and in the Doc page's header,
// each without opening the Doc first. Share here is "with <team>", the
// opposite of the machine's "with a link", so the words say so:
//
// - On a Doc in the person's own project (Mine), Share puts it in the team
//   they write Docs in (`moveDocsToTeam`, as the list's selection does, so a
//   Doc written here works too), or asks which when there are several.
// - Once a team Doc made from it is older than it, Share reads Update and
//   runs Update copy, asking first if the team edited theirs.
// - A team's Doc has nowhere to go: no Share.

import { useEffect, useRef, useState } from "preact/hooks";
import type { ComponentChildren } from "preact";
import type { ProjectDoc, ProjectDocCopy } from "../../shared/proto";
import { DocActions, openTab } from "../../shared/docs/doc-actions";
import { moveDocsToTeam, resultWords } from "../../shared/docs/move-docs";
import { EditedDialog, ShareWith, type Edited } from "../../shared/docs/share-with";
import { DocError, projectDocs } from "../../shared/docs/store";
import { useSubscribe } from "../../shared/ui/hooks";
import { IconButton, MoreIcon } from "../../shared/ui/icons";
import { Modal } from "../../shared/ui/modal";
import { menuBelow, openMenu, type MenuItem } from "../../shared/ui/menu";
import { names } from "../links";
import { go, pathOf } from "../router";
import { session, type PlatformProject } from "../session";
import { toast } from "../toast";
import { mayWrite } from "./issues";

/** The Doc's page, whole, as a link. */
const linkOf = (p: PlatformProject, doc: ProjectDoc) => new URL(pathOf({ page: "doc", project: p.id, doc: doc.id }, names), location.href).href;

/** What Share is for this Doc, as the button and the menu say it. */
interface Share {
  /** The button's tooltip and name. */
  title: string;
  /** The menu item's words. */
  word: string;
  /** Update: the team's copy is older. */
  older: boolean;
  /** `from` is what was pressed, for a menu that hangs from it. */
  run: (from: HTMLElement) => void;
}

function useDocActions(p: PlatformProject, doc: ProjectDoc, eager: boolean, reload?: () => void) {
  useSubscribe((fn) => session.subscribe(fn));
  const mine = !p.team;
  const teams = session.projects.filter((t) => t.team && mayWrite(t));
  const [copies, setCopies] = useState<ProjectDocCopy[]>([]);
  const [asking, setAsking] = useState<Edited | null>(null);
  const [picking, setPicking] = useState(false);
  const busy = useRef(false);

  /** The team Docs made from this one, read again; a Doc written here has none. */
  const load = async (): Promise<ProjectDocCopy[]> => {
    if (!mine || !doc.machine) return [];
    let list: ProjectDocCopy[] = [];
    try {
      list = (await projectDocs.copies(p.id, doc.id)).copies;
    } catch {}
    setCopies(list);
    return list;
  };
  useEffect(() => {
    if (eager) void load();
  }, [p.id, doc.id, doc.rev]);

  const after = async () => {
    await load();
    reload?.();
  };
  const move = async (team: PlatformProject) => {
    if (busy.current) return;
    busy.current = true;
    const [r] = await moveDocsToTeam(p.id, team.id, [doc]);
    busy.current = false;
    toast(`${r.title}: ${resultWords(r, team.name)}${r.noPictures ? " · pictures stay in Mine" : ""}`);
    await after();
  };
  const update = async (c: ProjectDocCopy, replace: boolean): Promise<void> => {
    if (busy.current) return;
    busy.current = true;
    try {
      await projectDocs.updateCopy(p.id, doc.id, c.project, replace);
      toast(`${c.project_name}'s copy now matches`);
    } catch (e) {
      if (e instanceof DocError && e.status === 409 && e.body.edited) setAsking({ project: c.project_name, go: () => update(c, true) });
      else toast((e as Error).message);
    }
    busy.current = false;
    await after();
  };

  /** What Share is, given the team Docs made from this one: none for a team's Doc. */
  const shareOf = (made: ProjectDocCopy[]): Share | undefined => {
    if (!mine || teams.length === 0) return undefined;
    const stale = made.filter((c) => c.stale);
    if (stale.length === 1) {
      const c = stale[0];
      return { title: `Update ${c.project_name}'s copy: it's older than this Doc`, word: "Update", older: true, run: () => void update(c, false) };
    }
    if (stale.length > 1) return { title: "Update the teams' copies: they're older than this Doc", word: "Update…", older: true, run: () => setPicking(true) };
    if (teams.length === 1) {
      const only = teams[0];
      return { title: `Share with ${only.name}`, word: `Share with ${only.name}`, older: false, run: () => void move(only) };
    }
    const word = "Share with…";
    // A copy of a machine's Doc has the dialog, with each team's state; a Doc written here, a pick of team.
    const run = (from: HTMLElement) =>
      doc.machine ? setPicking(true) : openMenu(menuBelow(from), teams.map((t) => ({ label: t.name, run: () => void move(t) })), { from, label: word });
    return { title: word, word, older: false, run };
  };
  const share = shareOf(copies);

  const copy = async (): Promise<"ok" | false> => {
    try {
      await navigator.clipboard.writeText(linkOf(p, doc));
      return "ok";
    } catch {
      toast("Couldn't copy the link");
      return false;
    }
  };
  const open = () => openTab(linkOf(p, doc));

  /** The ⋯ menu: the same actions, and the dialog for a Doc with several teams to say. */
  const items = (from: HTMLElement, made: ProjectDocCopy[] = copies): MenuItem[] => {
    const s = shareOf(made);
    return [
      { label: "Copy link", run: () => void copy().then((ok) => ok && toast("Link copied")) },
      ...(s ? [{ label: s.word, run: () => s.run(from) }] : []),
      ...(s && doc.machine && s.word !== "Share with…" ? [{ label: "Share with…", run: () => setPicking(true) }] : []),
      { label: "Open in browser", run: open },
    ];
  };

  const dialogs: ComponentChildren = asking ? (
    <EditedDialog edited={asking} cancel={() => setAsking(null)} />
  ) : picking ? (
    <ShareWithModal
      project={p.id}
      id={doc.id}
      done={() => {
        setPicking(false);
        void after();
      }}
    />
  ) : null;
  return { share, copy, open, items, load, dialogs };
}

/** The three actions, for the preview and the page's header. `menu` adds the ⋯ after them. */
export function PlatformDocActions({ p, doc, reload, menu }: { p: PlatformProject; doc: ProjectDoc; reload?: () => void; menu?: boolean }) {
  const a = useDocActions(p, doc, true, reload);
  return (
    <>
      <DocActions
        none={false}
        copy={a.copy}
        open={a.open}
        share={a.share && ((e) => a.share!.run(e.currentTarget as HTMLElement))}
        older={a.share?.older}
        labels={{ share: a.share?.title, update: a.share?.title }}
        hooks={{ copy: { "data-doc-copy-link": "", "data-link": linkOf(p, doc) }, share: { "data-doc-share": "" }, open: { "data-doc-open-tab": "" } }}
      >
        {menu && <MoreButton doc={doc} items={a.items} />}
      </DocActions>
      {a.dialogs}
    </>
  );
}

/** A row's ⋯: the same actions as a menu. It reads the team's copies when it's pressed, not for every row. */
export function RowMenu({ p, doc, reload }: { p: PlatformProject; doc: ProjectDoc; reload?: () => void }) {
  const a = useDocActions(p, doc, false, reload);
  return (
    <>
      <MoreButton doc={doc} items={a.items} load={a.load} row />
      {a.dialogs}
    </>
  );
}

/** The ⋯ button. A press is the button's own: it doesn't select or open the row it's on.
 * In a row it's a mouse target only, not a button: a row is one option, and a
 * button in it is a nested control. The keys reach the menu as the list's
 * ContextMenu key (or Shift+F10), and a right click, on the row. */
function MoreButton({ doc, items, load, row }: { doc: ProjectDoc; items: (from: HTMLElement, made?: ProjectDocCopy[]) => MenuItem[]; load?: () => Promise<ProjectDocCopy[]>; row?: boolean }) {
  const press = async (e: MouseEvent) => {
    e.stopPropagation();
    e.preventDefault();
    const from = e.currentTarget as HTMLElement;
    // Update shows in the menu only once the copies are known.
    const made = await load?.();
    openMenu(menuBelow(from, true), items(from, made), { from: row ? undefined : from, end: true, label: `More for ${doc.title}` });
  };
  if (row)
    return (
      <span class="bar-btn sq row-more" aria-hidden="true" data-doc-more data-row-more onClick={(e) => void press(e)} onDblClick={(e) => e.stopPropagation()}>
        <MoreIcon size="sm" />
      </span>
    );
  return (
    <IconButton label={`More for ${doc.title}`} title="More" class="bar-btn sq" aria-haspopup="menu" data-doc-more onClick={(e) => void press(e)}>
      <MoreIcon size="sm" />
    </IconButton>
  );
}

/** Share with…: the projects the reader writes Docs in, and what each has of
 * this copy. Update copy that would replace the team's edits asks first, in
 * this same dialog. */
function ShareWithModal({ project, id, done }: { project: string; id: string; done: () => void }) {
  const [asking, setAsking] = useState<Edited | null>(null);
  if (asking) return <EditedDialog edited={asking} cancel={() => setAsking(null)} />;
  return (
    <Modal
      title="Share with…"
      close={done}
      root={{ "data-share-with-dialog": true }}
      buttons={
        <button class="secondary" type="button" onClick={done}>
          Done
        </button>
      }
    >
      <ShareWith
        project={project}
        id={id}
        ask={setAsking}
        say={toast}
        done={toast}
        open={(c) => {
          done();
          go(pathOf({ page: "doc", project: c.project, doc: c.id }, names));
        }}
        unreachable={<p class="dim">Couldn't reach the platform.</p>}
      />
    </Modal>
  );
}
