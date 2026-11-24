// Overview's Needs you (#1333): the asks grouped by owner, each a two-line
// row (a kind mark and the question, then whose and where) that opens to
// the rail card's body: the full question, its details and its actions.
// The body is Swarm's `Card`, so approving, answering and opening act as
// they do there; the row only decides when it's drawn. Above them, what
// the reader's agents propose to do as them (Arugula #1278): the hub's
// open proposals in the project, each its chat card (../shared/chat/
// ask-card.tsx), Approve and Deny in place.
import { useEffect, useRef, useState } from "preact/hooks";
import type { ComponentProps } from "preact";
import { platformFetch } from "../shared/maintenance";
import type { ChatAsk, ChatAsks } from "../shared/proto";
import { BY_CARD_EVERY_MS } from "../shared/project-sessions";
import type { Fleet, FleetPane } from "../shell/fleet";
import { groupByOwner } from "../shell/swarm/overview";
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

/** Whose an ask is: null for yours, else the name its group carries. */
export const askOwner = (fleet: Fleet, b: Bundle): string | null => paneOwner(fleet, b.panes[0]);

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
 * way doesn't bring it back. */
export function useProposals(project: string, moved: string, refocus: number): { asks: ChatAsk[]; drop(a: ChatAsk): void } {
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
  return { asks, drop };
}

/** `GET /api/projects/:project/asks`, its proposals; none from a hub without it. */
async function projectAsks(project: string): Promise<ChatAsk[]> {
  const res = await platformFetch(`/api/projects/${encodeURIComponent(project)}/asks`, { credentials: "same-origin" });
  if (res.status === 404) return [];
  if (!res.ok) throw new Error(`HTTP ${res.status}`);
  return ((await res.json()) as ChatAsks).asks.filter((a) => a.proposal);
}

type CardProps = Omit<ComponentProps<typeof Card>, "b" | "fleet" | "bare">;

/** The groups and their rows. `pin` says which rows are open, so the list
 * holds still under them as it does under the pointer. */
export function Asks({
  asks,
  fleet,
  pin,
  card,
}: {
  asks: Bundle[];
  fleet: Fleet;
  pin: (key: string, open: boolean) => void;
  card: (b: Bundle) => CardProps;
}) {
  const groups = groupByOwner(asks, (b) => askOwner(fleet, b));
  // Only yours: no heading to say so.
  const headed = groups.length > 1 || groups[0]?.name !== null;
  return (
    <>
      {groups.map((g) => (
        <section key={g.key} class="ov-asks" data-ov-asks={g.name ?? "me"}>
          {headed && (
            <h3 class="ov-asks-head" data-ov-asks-head>
              <span>{g.name ?? "Yours"}</span>
              <span class="ov-asks-n">{g.items.length}</span>
            </h3>
          )}
          {g.items.map((b) => (
            <Ask key={b.key} b={b} fleet={fleet} pin={pin} card={card(b)} />
          ))}
        </section>
      ))}
    </>
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
