// Overview's Needs you (#1333): the project's Inbox items (Arugula #1433),
// in the server's order, each a two-line row (a kind mark and the
// question, then whose and where). One about a pane this page has (#1431)
// is that pane's row, which opens to the rail card's body: the full
// question, its details and its actions. The body is Swarm's `Card`, so
// approving, answering and opening act as they do there; the row only
// decides when it's drawn. Any other item's row opens where it is. A
// teammate's are folded under "N asks for Jo ›" (Arugula #1393). Above
// them, what the reader's agents propose to do as them (Arugula #1278):
// the hub's open proposals in the project the Inbox lists, each its chat
// card (../shared/chat/ask-card.tsx), Approve and Deny in place.
import { useEffect, useRef, useState } from "preact/hooks";
import type { ComponentProps } from "preact";
import { platformFetch } from "../shared/maintenance";
import type { ChatAsk, ChatAsks, InboxItem } from "../shared/proto";
import { BY_CARD_EVERY_MS } from "../shared/project-sessions";
import type { Fleet, FleetPane } from "../shell/fleet";
import {
  cardTitle,
  headlineOf,
  REASON_COL,
  shortHost,
  shortPaths,
  whatItDoes,
} from "../shell/swarm/model";
import { Card, type Bundle } from "../shell/swarm/view";
import { yours } from "./overview-peek";
import { asksFor, byTeammate, forTeammate, kindWords, memberName } from "../platform/inbox-count";
import { go } from "../platform/router";

/** Whose a pane is: null for yours, else a name. A teammate's is theirs; a
 * team's box or a hosted session is its starter's, else the team's name.
 * Needs you groups by it, and a card names whom it asks by it (#1392). */
export function paneOwner(fleet: Fleet, p: FleetPane): string | null {
  if (p.person.kind === "me") return null;
  const name =
    p.person.kind === "person" && !fleet.host(p.host)?.hosted
      ? p.person.name
      : (p.owner ?? p.person.name);
  return name === fleet.me ? null : name;
}

/** The reader's open proposals in `project`, oldest first: read now, when
 * `moved` changes (the staff room's last message and mentions), on
 * `refocus`, and every minute; none where the hub has no such route.
 * `drop` takes one answered here out at once, and a read already on its
 * way doesn't bring it back; `dropped` are their ids, which the Inbox may
 * list a moment longer. */
export function useProposals(project: string, moved: string, refocus: number): { asks: ChatAsk[]; drop(a: ChatAsk): void; dropped: ReadonlySet<string> } {
  const [asks, setAsks] = useState<ChatAsk[]>([]);
  const gone = useRef(new Set<string>());
  useEffect(() => {
    let live = true;
    const read = () =>
      projectAsks(project).then(
        (v) => live && setAsks(v.filter((a) => !gone.current.has(a.proposal!.id))),
        // Not now: as it was, and the next read tries again.
        () => {},
      );
    void read();
    const t = setInterval(() => document.visibilityState === "visible" && void read(), BY_CARD_EVERY_MS);
    return () => {
      live = false;
      clearInterval(t);
    };
  }, [project, moved, refocus]);
  const drop = (a: ChatAsk) => {
    gone.current.add(a.proposal!.id);
    setAsks((was) => was.filter((x) => x.proposal!.id !== a.proposal!.id));
  };
  return { asks, drop, dropped: gone.current };
}

/** `GET /api/projects/:project/asks`, its proposals; none from a hub without it. */
async function projectAsks(project: string): Promise<ChatAsk[]> {
  const res = await platformFetch(`/api/projects/${encodeURIComponent(project)}/asks`, { credentials: "same-origin" });
  if (res.status === 404) return [];
  if (!res.ok) throw new Error(`HTTP ${res.status}`);
  return ((await res.json()) as ChatAsks).asks.filter((a) => a.proposal);
}

type CardProps = Omit<ComponentProps<typeof Card>, "b" | "fleet" | "bare">;

/** The pane an Inbox item is about (a `pane` subject, #1431), if this
 * page has it. */
export function paneOfItem(fleet: Fleet, i: InboxItem): FleetPane | undefined {
  const s = i.subject;
  if (s.type !== "pane" || !s.machine || !s.pane) return undefined;
  return fleet.panes.find((p) => String(p.id) === s.pane && (p.host === s.machine || fleet.host(p.host)?.id === s.machine));
}

/** The list: the reader's items, then each teammate's, folded. `railed`
 * are the rows of the items' panes, held still under the pointer as
 * Swarm's are; `pin` says which rows are open, so the list holds still
 * under them too. */
export function Needs({
  items,
  me,
  railed,
  fleet,
  pin,
  card,
}: {
  items: InboxItem[];
  me: string | null | undefined;
  railed: Bundle[];
  fleet: Fleet;
  pin: (key: string, open: boolean) => void;
  card: (b: Bundle) => CardProps;
}) {
  const [unfolded, setUnfolded] = useState<ReadonlySet<string>>(new Set());
  const drawn = new Set<string>();
  const rows = (list: InboxItem[]) =>
    list.flatMap((i) => {
      const p = paneOfItem(fleet, i);
      // A pane here says what it asks now: its row, once for its bundle;
      // a pane that no longer asks is answered, though the list hasn't
      // caught up.
      if (p) {
        const b = railed.find((x) => x.panes.some((y) => y.key === p.key));
        if (!b || drawn.has(b.key)) return [];
        drawn.add(b.key);
        return [<Ask key={b.key} b={b} fleet={fleet} pin={pin} card={card(b)} />];
      }
      return [<Item key={i.id} i={i} />];
    });
  const mine = rows(items.filter((i) => !forTeammate(i, me)));
  const folds = byTeammate(items, me).map(([who, list]) => ({ who, open: unfolded.has(who), list, rows: unfolded.has(who) ? rows(list) : [] }));
  // Rows whose ask cleared under the pointer: kept, faded, where they were.
  const held = railed.filter((b) => b.gone && !drawn.has(b.key)).map((b) => <Ask key={b.key} b={b} fleet={fleet} pin={pin} card={card(b)} />);
  const toggle = (who: string) => setUnfolded((s) => (s.has(who) ? new Set([...s].filter((k) => k !== who)) : new Set([...s, who])));
  return (
    <>
      {(mine.length > 0 || held.length > 0) && (
        <section class="ov-asks" data-ov-asks="me">
          {mine}
          {held}
        </section>
      )}
      {folds.map((f) => (
        <section key={f.who} class="ov-asks" data-ov-asks={f.who}>
          <h3 class="ov-asks-head" data-ov-asks-head>
            <button type="button" class="link" aria-expanded={f.open} data-ov-asks-for={f.who} onClick={() => toggle(f.who)}>
              {asksFor(f.list.length, memberName(f.who))}
            </button>
          </h3>
          {f.rows}
        </section>
      ))}
    </>
  );
}

/** Its mark's colour: a failure's, a gate's for what waits on the reader's
 * yes (a permission, a deploy, changes asked), else an ask's. */
function markOf(kind: string): keyof typeof REASON_COL {
  if (kind === "checks_failed" || kind === "run_failed" || kind === "agent_failed") return "failed";
  if (kind === "permission" || kind === "deploy_approval" || kind === "changes_requested") return "gate";
  return "ask";
}

/** An item no pane here answers: its title, then what it is and who from;
 * a click or Enter opens it where it is (here, or GitHub). A kind this
 * doesn't know says its title alone. */
function Item({ i }: { i: InboxItem }) {
  const where = [kindWords[i.kind], i.from.name, i.escalated ? "no agent answered it" : null].filter(Boolean).join(" · ");
  const open = () => (i.subject.url.startsWith("/") ? go(i.subject.url) : window.open(i.subject.url, "_blank", "noopener,noreferrer"));
  const mark = markOf(i.kind);
  return (
    <div class={`ov-ask t-${mark}`} data-ov-ask={i.id} data-kind={i.kind} style={{ "--c": `rgb(${REASON_COL[mark]})` }}>
      <div
        class="ov-ask-row"
        role="link"
        tabIndex={0}
        aria-label={`${i.title}${where ? `: ${where}` : ""}`}
        onClick={open}
        onKeyDown={(e) => {
          if (e.target !== e.currentTarget || (e.key !== "Enter" && e.key !== " ")) return;
          e.preventDefault();
          open();
        }}
      >
        <i class="ov-ask-mark" aria-hidden="true" />
        <span class="ov-ask-q">{i.title}</span>
        {where && <span class="ov-ask-where dim">{where}</span>}
      </div>
    </div>
  );
}

/** One ask. It opens on a click, and on the pointer's hover only where
 * there is hover; Enter and Space toggle it and Escape closes it. Its
 * actions exist only while it's open, so a stray tap can't approve. */
function Ask({
  b,
  fleet,
  pin,
  card,
}: {
  b: Bundle;
  fleet: Fleet;
  pin: (key: string, open: boolean) => void;
  card: CardProps;
}) {
  const [clicked, setClicked] = useState(false);
  const [hovered, setHovered] = useState(false);
  const open = clicked || hovered;
  useEffect(() => {
    pin(b.key, clicked);
    return () => pin(b.key, false);
  }, [clicked]);
  const r = b.reason;
  const n = b.panes.length;
  const first = b.panes[0];
  const machines = [
    ...new Set(
      b.panes.map((p) => shortHost(fleet.host(p.host)?.name ?? p.host)),
    ),
  ];
  const session = n === 1 ? first.session?.name : null;
  // A teammate's ask you only watch: no action here, the card says why.
  const watching = !b.panes.every((p) => fleet.role(p) !== "viewer");
  const kind =
    r.kind === "exited" || r.kind === "errors"
      ? "failed"
      : r.kind === "diff"
        ? "ask"
        : r.kind;
  const title = cardTitle(r, n, machines);
  const question = shortPaths(headlineOf(r));
  const where = [
    n === 1 ? whatItDoes(first) : `${n} panes`,
    session,
    machines.length > 1 ? `${machines.length} machines` : machines[0],
    watching && !yours(fleet, first) ? "watching" : null,
  ]
    .filter(Boolean)
    .join(" · ");
  // A click keeps it open, even one the pointer opened; the next closes it, though the pointer is still on it.
  const toggle = () => {
    setClicked(!clicked);
    setHovered(false);
  };
  return (
    <div
      class={`ov-ask t-${kind}${open ? " open" : ""}${b.gone ? " gone" : ""}`}
      data-ov-ask={b.key}
      data-kind={r.kind}
      data-open={open ? "" : undefined}
      style={{ "--c": `rgb(${REASON_COL[r.kind]})` }}
      // Where there's hover, resting on it opens it; the pointer leaving closes what it opened, never what was clicked.
      onPointerEnter={(e) =>
        e.pointerType === "mouse" &&
        matchMedia("(hover: hover)").matches &&
        setHovered(true)
      }
      onPointerLeave={() => setHovered(false)}
    >
      <div
        class="ov-ask-row"
        role="button"
        tabIndex={0}
        aria-expanded={open}
        aria-label={`${title}: ${question}`}
        onClick={toggle}
        onKeyDown={(e) => {
          if (e.target !== e.currentTarget) return;
          if (e.key === "Enter" || e.key === " ") {
            e.preventDefault();
            toggle();
          } else if (e.key === "Escape" && open) {
            setClicked(false);
            setHovered(false);
          }
        }}
      >
        <i class="ov-ask-mark" aria-hidden="true" />
        <span class="ov-ask-q">{open ? title : question}</span>
        <span class="ov-ask-where dim">{where}</span>
      </div>
      {open && (
        <div class="ov-ask-body">
          <Card b={b} fleet={fleet} bare {...card} />
        </div>
      )}
    </div>
  );
}
