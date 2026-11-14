// The card a pane points back to (Arugula inversion): a chip in its bar
// (under the title on a phone) naming its session's project and card,
// `Arugula > #983 Title`. A click goes to the card's row on its board,
// whose drawer opens there; its menu changes the card or takes the session
// off it. A session in a project with no card has a dashed chip that puts
// it on one. A session in no project has no chip. The card is the
// session's (`Intent::SetSessionWork`); its title and board row are read
// from Arugula cloud as the person looking, once per project.

import type { Client } from "../client";
import type { BoardPointer, NativeIssue, OnBoard, PaneId } from "../../shared/proto";
import { boards } from "../../shared/boards/store";
import { issues } from "../../shared/issues/store";
import { askText, openMenu, type MenuItem } from "../../shared/ui/menu";
import { ObjectChip } from "../../shared/ui/state-chip";
import { Caret, ChevronIcon, IconButton } from "../../shared/ui/icons";
import { useSubscribe } from "../../shared/ui/hooks";
import { openPlatform, platformOrigin } from "../platform-links";
import { getSignedIn } from "../signed-in";
import { CHANGE, PUT_ON, TAKE_OFF, cardChipText, cardChoices, cardNumber, cardPath, cardPicked } from "./pane-card-text";

const ready = () => getSignedIn()?.phase === "ready";

/** Each project's open issues and its boards' issue rows, read once a page. */
const read = new Map<string, Promise<void>>();
/** A project's rows that point at a native issue: the issue's id, its board and row. */
const rows = new Map<string, { issue: string; board: string; row: string }[]>();
const heard = new Set<() => void>();

function readProject(project: string) {
  if (read.has(project) || !ready()) return;
  const issuesRead = issues.list(project, {}, undefined, 100).then(() => {});
  const rowsRead = boards.list(project).then(async (l) => {
    const views = await Promise.all(l.boards.map((b) => boards.get(b.id).catch(() => null)));
    const found = views.flatMap((v) =>
      v ? v.zones.flatMap((z) => z.rows.flatMap((r) => (isIssue(r.pointer) ? [{ issue: r.pointer.id, board: v.board.id, row: r.id }] : []))) : [],
    );
    rows.set(project, found);
    heard.forEach((fn) => fn());
  });
  read.set(project, Promise.allSettled([issuesRead, rowsRead]).then(() => {}));
}

const isIssue = (p: BoardPointer): p is Extract<BoardPointer, { kind: "issue" }> => p.kind === "issue";

/** The issue a card names, when the page has read it. */
function issueOf(project: string, card: string): NativeIssue | undefined {
  const n = cardNumber(card);
  return issues.all().find((i) => (n === null ? i.id === card : i.project === project && i.number === n));
}

function useCards(project: string | undefined) {
  useSubscribe((fn) => issues.subscribe(fn));
  useSubscribe((fn) => {
    heard.add(fn);
    return () => void heard.delete(fn);
  });
  if (project) readProject(project);
}

/** Where a card is: its row on a board (the one the block's own work names
 * first), else its issue's page. */
function pathOf(project: string, card: string, on: OnBoard | undefined): string {
  if (on) return cardPath(on.project, on, null);
  const issue = issueOf(project, card)?.id ?? (card.startsWith("iss_") ? card : null);
  const at = issue ? rows.get(project)?.find((r) => r.issue === issue) : undefined;
  return cardPath(project, at ?? null, issue);
}

/** Go to the card: its board row, with its drawer open, on Arugula cloud. */
export function goToCard(client: Client, project: string, card: string, on?: OnBoard) {
  if (!platformOrigin()) return client.toast("Cards are on Arugula cloud: open this machine there, signed in, to go to one");
  openPlatform(pathOf(project, card, on));
}

/** Put a pane's session on a card, or another, from the project's open issues. */
async function pickCard(client: Client, pane: PaneId) {
  const info = client.info(pane);
  const project = info?.session_project;
  const session = client.sessionOfPane(pane);
  if (!project || session === null) return;
  if (!ready()) return client.toast("Cards are read as you, through Arugula cloud: open this machine in Arugula cloud, signed in");
  const open = issues.list(project.id, {}, undefined, 100).then((l) => l.issues);
  const answer = await askText(info?.on_issue ? CHANGE : PUT_ON, "", "#number or title", open.then((l) => cardChoices(l, info?.on_issue?.issue ?? null)));
  if (answer === null || !answer.trim()) return;
  const card = cardPicked(answer, await open.catch(() => []));
  if (!card) return client.toast(`No open card in ${project.name} matches "${answer.trim()}"`);
  client.intent({ op: "set_session_work", session, work: { project: project.id, item: card }, project_name: project.name }, (why) => client.toast(why));
}

function takeOff(client: Client, pane: PaneId) {
  const session = client.sessionOfPane(pane);
  if (session !== null) client.intent({ op: "set_session_work", session, work: null }, (why) => client.toast(why));
}

/** The chip's menu: go to the card, change it, take the session off it; or put it on one. */
export function cardItems(client: Client, pane: PaneId): MenuItem[] {
  const info = client.info(pane);
  const project = info?.session_project;
  if (!project) return [];
  const on = info?.on_issue;
  // Putting a session on a card reads the project's cards as you: nothing to offer signed out.
  if (!on) return ready() ? [{ label: `${PUT_ON}…`, run: () => void pickCard(client, pane), attrs: { "data-card-put": "" } }] : [];
  return [
    { label: `Open ${on.issue}`, run: () => goToCard(client, project.id, on.issue, on.boards?.[0]), external: true, attrs: { "data-card-open": "" } },
    { label: `${CHANGE}…`, run: () => void pickCard(client, pane), attrs: { "data-card-change": "" } },
    { label: TAKE_OFF, run: () => takeOff(client, pane), attrs: { "data-card-off": "" } },
  ];
}

function menuFrom(client: Client, pane: PaneId, el: HTMLElement) {
  const r = el.getBoundingClientRect();
  openMenu({ clientX: r.left, clientY: r.bottom, preventDefault() {} }, cardItems(client, pane), { from: el, label: "Card", attrs: { "data-card-menu": "" } });
}

/** A pane's card chip, in `room` characters; nothing for a session in no
 * project, or when even its number doesn't fit; nor a dashed one while
 * nobody is signed in here. On a phone a tap opens the menu, a sheet there. */
export function PaneCardChip({ client, pane, room, phone }: { client: Client; pane: PaneId; room: number; phone?: boolean }) {
  useSubscribe((fn) => getSignedIn()?.subscribe(fn) ?? (() => {}));
  const info = client.info(pane);
  const project = info?.session_project ?? undefined;
  useCards(project?.id);
  if (!project) return null;
  const on = info?.on_issue ?? null;
  const card = on?.issue ?? null;
  // No card, and nobody signed in to put it on one: no chip.
  if (!card && !ready()) return null;
  const issue = card ? issueOf(project.id, card) : undefined;
  const said = cardChipText(project.name, card, issue?.title ?? null, room);
  if (!said) return null;
  const menu = (e: MouseEvent) => {
    e.preventDefault();
    e.stopPropagation();
    menuFrom(client, pane, e.currentTarget as HTMLElement);
  };
  const where = said.project && (
    <span class="pane-work-project">
      {said.project}
      <ChevronIcon dir="right" />
    </span>
  );
  if (!card) {
    return (
      <span class="pane-work-wrap">
        <ObjectChip proof class="pane-work" title={`${project.name}: put this session on a card`} data-pane-card="" onClick={() => void pickCard(client, pane)}>
          {where}
          <span class="pane-work-title">{PUT_ON}</span>
        </ObjectChip>
      </span>
    );
  }
  const title = `${project.name}: ${card}${issue ? ` ${issue.title}` : ""}`;
  return (
    <span class="pane-work-wrap">
      <ObjectChip
        class="pane-work"
        title={phone ? title : `${title}: go to the card`}
        data-pane-card={card}
        onClick={(e: MouseEvent) => (phone ? menu(e) : goToCard(client, project.id, card, on?.boards?.[0]))}
        onContextMenu={menu}
      >
        {where}
        <code class="chip-object-id">{card}</code>
        {said.title && <span class="pane-work-title">{said.title}</span>}
      </ObjectChip>
      {!phone && (
        <IconButton class="pane-header-button bar-btn sq" label="Card menu" title={`${CHANGE}, or take it off`} data-pane-card-menu={pane} onClick={menu}>
          <Caret />
        </IconButton>
      )}
    </span>
  );
}
