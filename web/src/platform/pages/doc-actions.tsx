// A Doc's actions on the platform (Arugula #820): the same Copy link, Share
// and Open in browser the machine's Doc bar has (shared/docs/doc-actions.tsx),
// in the Docs list's preview, in a row's ⋯ menu and in the Doc page's header,
// each without opening the Doc first. Here it's Move to <team>, the
// opposite of the machine's Share with a link, so the words say so:
//
// - On a Doc in the person's own project (Mine), Move to <team>… gives it to
//   the team they write Docs in (`moveDocsToTeam`, as the list's selection
//   does), or asks which when there are several, and confirms first
//   (`MoveConfirm`, Arugula #863): the team owns it, and nobody keeps a copy.
// - A team's Doc has nowhere to go: no Share.
//
// A Doc in the person's own project also has Download in ⋯ (Arugula #865):
// the Doc as Markdown, built here (shared/docs/download.ts). Delete is last
// in ⋯ for those who may (Arugula #864): a Doc goes to the project's Trash
// (#866).

import { useEffect, useRef, useState } from "preact/hooks";
import type { ComponentChildren } from "preact";
import type { ProjectDoc } from "../../shared/proto";
import { DocActions, openTab } from "../../shared/docs/doc-actions";
import { moveDocsToTeam, resultWords } from "../../shared/docs/move-docs";
import { saveDoc } from "../../shared/docs/download";
import { projectDocs } from "../../shared/docs/store";
import { revokePublic, sharePublic } from "../../shared/docs/public-link";
import { useSubscribe } from "../../shared/ui/hooks";
import { IconButton, MoreIcon } from "../../shared/ui/icons";
import { ConfirmRemove } from "../../shared/ui/confirm";
import { menuBelow, openMenu, type MenuItem } from "../../shared/ui/menu";
import { names } from "../links";
import { pathOf } from "../router";
import { session, type PlatformProject } from "../session";
import { toast } from "../toast";
import { mayWrite } from "./issues";

/** How long a deleted team Doc waits in the Trash (the wire's `TRASH_DAYS`; the list says its own). */
export const TRASH_DAYS = 30;

/** The Doc's page, whole, as a link. */
const linkOf = (p: PlatformProject, doc: ProjectDoc) => new URL(pathOf({ page: "doc", project: p.id, doc: doc.id }, names), location.href).href;

/** What Share is for this Doc, as the button and the menu say it. */
interface Share {
  /** The button's tooltip and name. */
  title: string;
  /** The menu item's words. */
  word: string;
  /** `from` is what was pressed, for a menu that hangs from it. */
  run: (from: HTMLElement) => void;
}

/** Who may delete it, and what follows a delete. */
interface Remove {
  deleter?: boolean;
  gone?: () => void;
}

function useDocActions(p: PlatformProject, doc: ProjectDoc, eager: boolean, reload?: () => void, remove: Remove = {}) {
  useSubscribe((fn) => session.subscribe(fn));
  const mine = !p.team;
  const teams = session.projects.filter((t) => t.team && mayWrite(t));
  const [deleting, setDeleting] = useState(false);
  /** The team Move to… asks about. */
  const [moving, setMoving] = useState<PlatformProject | null>(null);
  const busy = useRef(false);
  // Arugula #871: a team Doc's owners and editors share it with a public link. The list's
  // rows don't say whether it has one, so the menu reads the Doc when it opens, and offers
  // Share or Stop by what it finds.
  const publicly = !!p.team && mayWrite(p);
  const shared = useRef(false);

  /** Whether it has a public link, read again. */
  const load = async (): Promise<void> => {
    if (!publicly) return;
    try {
      const v = await projectDocs.get(p.id, doc.id);
      shared.current = !!v.public && v.public.ver > 0;
    } catch {}
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
    toast(`${r.title}: ${resultWords(r, team.name)}`);
    await after();
  };
  /** Move to `team`, once confirmed. */
  const ask = (team: PlatformProject) => setMoving(team);
  /** What Share is: Move to a team, for a Doc of the person's own; none for a team's Doc. */
  const shareOf = (): Share | undefined => {
    if (!mine || teams.length === 0) return undefined;
    if (teams.length === 1) {
      const only = teams[0];
      return { title: `Move to ${only.name}`, word: `Move to ${only.name}…`, run: () => ask(only) };
    }
    const word = "Move to…";
    const run = (from: HTMLElement) => openMenu(menuBelow(from), teams.map((t) => ({ label: t.name, run: () => ask(t) })), { from, label: word });
    return { title: word, word, run };
  };
  const share = shareOf();

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
  /** Download: the Doc as Markdown, read whole first. */
  const download = async () => {
    try {
      saveDoc(await projectDocs.get(p.id, doc.id));
    } catch (e) {
      toast(`Couldn't download it: ${(e as Error).message}`);
    }
  };

  /** Arugula #871, from the list: make (or update) the Doc's public link and copy it. */
  const sharePublicly = async () => {
    try {
      const link = await sharePublic(p.id, doc.id);
      if (!link.url) return toast("This control keeps no shared Docs");
      await navigator.clipboard.writeText(link.url).catch(() => {});
      toast("Public link copied");
      reload?.();
    } catch (e) {
      toast((e as Error).message);
    }
  };
  const stopPublic = async () => {
    try {
      const v = await projectDocs.get(p.id, doc.id);
      if (!v.public) return toast("It has no public link");
      await revokePublic(p.id, doc.id);
      toast("The link no longer opens");
      reload?.();
    } catch (e) {
      toast((e as Error).message);
    }
  };

  /** The ⋯ menu: the same actions, and Download for a Doc of the person's own. */
  const items = (from: HTMLElement): MenuItem[] => {
    const s = share;
    return [
      { label: "Copy link", run: () => void copy().then((ok) => ok && toast("Link copied")) },
      ...(s ? [{ label: s.word, run: () => s.run(from) }] : []),
      ...(mine ? [{ label: "Download", hint: "As Markdown, without its pictures", attrs: { "data-doc-download": true }, run: () => void download() }] : []),
      ...(publicly
        ? [
            ...(shared.current
              ? [
                  { label: "Update public link", run: () => void sharePublicly() },
                  { label: "Stop public link", run: () => void stopPublic() },
                ]
              : [{ label: "Share public link", run: () => void sharePublicly() }]),
          ]
        : []),
      { label: "Open in browser", run: open },
      ...(remove.deleter ? ["separator" as const, { label: "Delete…", danger: true, attrs: { "data-doc-delete": true }, run: () => setDeleting(true) }] : []),
    ];
  };
  const drop = async () => {
    setDeleting(false);
    try {
      await projectDocs.remove(p.id, doc.id);
      remove.gone?.();
    } catch (err) {
      toast(`Couldn't delete it: ${(err as Error).message}`);
    }
  };
  const dialogs: ComponentChildren = moving ? (
    <MoveConfirm
      title={doc.title}
      team={moving.name}
      cancel={() => setMoving(null)}
      go={() => {
        setMoving(null);
        void move(moving);
      }}
    />
  ) : deleting ? (
    <ConfirmRemove title={`Delete “${doc.title}”?`} label="Delete" go={() => void drop()} cancel={() => setDeleting(false)}>
      <p data-doc-confirm>
        It goes to the Trash for {TRASH_DAYS} days, out of sight of everyone in the project. An owner, an editor or its maker can restore it from the Docs page's Trash.
      </p>
    </ConfirmRemove>
  ) : null;
  return { share, copy, open, items, load, dialogs };
}

/** The three actions, for the preview and the page's header. `menu` adds the ⋯ after them. */
export function PlatformDocActions({ p, doc, reload, menu, deleter, gone }: { p: PlatformProject; doc: ProjectDoc; reload?: () => void; menu?: boolean } & Remove) {
  const a = useDocActions(p, doc, true, reload, { deleter, gone });
  return (
    <>
      <DocActions
        none={false}
        copy={a.copy}
        open={a.open}
        share={a.share && ((e) => a.share!.run(e.currentTarget as HTMLElement))}
        labels={{ share: a.share?.title }}
        hooks={{ copy: { "data-doc-copy-link": "", "data-link": linkOf(p, doc) }, share: { "data-doc-share": "" }, open: { "data-doc-open-tab": "" } }}
      >
        {menu && <MoreButton doc={doc} items={a.items} />}
      </DocActions>
      {a.dialogs}
    </>
  );
}

/** A row's ⋯: the same actions as a menu. It reads whether the Doc has a public link when it's pressed, not for every row. */
export function RowMenu({ p, doc, reload }: { p: PlatformProject; doc: ProjectDoc; reload?: () => void }) {
  const a = useDocActions(p, doc, false, reload, { deleter: mayWrite(p), gone: reload });
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
function MoreButton({ doc, items, load, row }: { doc: ProjectDoc; items: (from: HTMLElement) => MenuItem[]; load?: () => Promise<void>; row?: boolean }) {
  const press = async (e: MouseEvent) => {
    e.stopPropagation();
    e.preventDefault();
    const from = e.currentTarget as HTMLElement;
    // The public link's items show only once it's known.
    await load?.();
    openMenu(menuBelow(from, true), items(from), { from: row ? undefined : from, end: true, label: `More for ${doc.title}` });
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

/** Move to <team>…, asked first (Arugula #863): the team owns it, its link
 * keeps working, and nobody keeps a copy. `title` is one Doc's, or how many. */
export function MoveConfirm({ title, team, n = 1, go, cancel }: { title: string; team: string; n?: number; go: () => void; cancel: () => void }) {
  const what = n === 1 ? "it" : "them";
  return (
    <ConfirmRemove title={n === 1 ? `Move “${title}” to ${team}?` : `Move ${n} Docs to ${team}?`} label="Move" go={go} cancel={cancel}>
      <p data-move-confirm>
        {team} will own {what}, and nobody keeps a copy: {n === 1 ? "it leaves" : "they leave"} your Docs. {n === 1 ? "Its link keeps" : "Their links keep"} working.
      </p>
    </ConfirmRemove>
  );
}
