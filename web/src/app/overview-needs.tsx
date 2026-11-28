// The Overview's right third, above the agent panel (Lift 4, #1480): Needs
// you, this project's slice of the Inbox (`by_project`'s items), then To do,
// then a teammate's asks folded into one line each ("2 asks for Jake ›").
// Each is the Inbox's row (./needs), answered in place, with its keys; what
// the chief of staff put in a tab for you (Lift 9) says which and opens it.
// Alerts (checks or a run failed) aren't here: they're the board's and
// Running's, and the Inbox's.

import { useState } from "preact/hooks";
import type { InboxItem } from "../shared/proto";
import { ChevronIcon } from "../shared/ui/icons";
import { NeedList } from "../shared/kit/need-row";
import { Row } from "../shared/kit/row";
import { Surface } from "../shared/kit/surface";
import { needAge } from "../shared/needs-you";
import type { Fleet } from "../shell/fleet";
import { asksFor, byTeammate, forTeammate, memberName } from "../platform/inbox";
import { needsOf, useNeeds, type Tabbed } from "./needs";

/** The rows past which the list folds ("N more ›"), so the panel keeps its room. */
const SHOWN = 5;

export function OverviewNeeds({ items, me, fleet, toast, peek, tabOf }: { items: InboxItem[]; me: string | null | undefined; fleet: Fleet; toast: (text: string) => void; peek: (i: InboxItem) => void; tabOf?: (i: InboxItem) => Tabbed | null }) {
  const [unfolded, setUnfolded] = useState<ReadonlySet<string>>(new Set());
  const [more, setMore] = useState(false);
  const needs = useNeeds({ me, fleet, toast, peek, tabOf });
  const { receipts, row } = needs;

  const live = needsOf(items);
  const all = needs.kept(live);
  const mine = all.filter((i) => !forTeammate(i, me));
  const asks = mine.filter((i) => i.kind !== "todo");
  const todos = mine.filter((i) => i.kind === "todo");
  const theirs = byTeammate(all, me);
  const shownAsks = more ? asks : asks.slice(0, SHOWN);
  const folded = asks.length - shownAsks.length;
  needs.order([...shownAsks, ...todos, ...theirs.flatMap(([who, list]) => (unfolded.has(who) ? list : []))]);

  const count = live.filter((i) => i.counts && !forTeammate(i, me) && i.kind !== "todo").length;
  const oldest = needAge(asks.find((i) => !receipts.has(i.id))?.since);
  return (
    <div class="ov-needs" data-ov-needs>
      <Surface kind="section" as="section" aria-label="Needs you" data-ov-needs-asks>
        <header class="ov-head">
          <h2>Needs you</h2>
          <span class="m-meta" data-ov-needs-count>
            {count || ""}
          </span>
          {oldest && <span class="ov-head-end m-meta">oldest {oldest}</span>}
        </header>
        {asks.length === 0 ? (
          <p class="ov-quiet" data-ov-needs-none>
            Nothing needs you.
          </p>
        ) : (
          <NeedList label="Needs you">{shownAsks.map((i) => row(i))}</NeedList>
        )}
        {folded > 0 && (
          <button type="button" class="ov-more" data-ov-needs-more onClick={() => setMore(true)}>
            {folded} more <ChevronIcon dir="right" size="xs" />
          </button>
        )}
        {todos.length > 0 && (
          <>
            <header class="ov-head" data-ov-todo>
              <h2>To do</h2>
              <span class="m-meta">{todos.filter((i) => !receipts.has(i.id)).length || ""}</span>
            </header>
            <NeedList label="To do">{todos.map((i) => row(i))}</NeedList>
          </>
        )}
        {theirs.map(([who, list]) => (
          <div key={who} data-ov-asks-for={who}>
            <Row
              label=""
              title={asksFor(list.length, memberName(who))}
              open={unfolded.has(who)}
              onClick={() => setUnfolded((s) => (s.has(who) ? new Set([...s].filter((x) => x !== who)) : new Set([...s, who])))}
            >
              <NeedList label={`For ${memberName(who)}`}>{list.map((i) => row(i))}</NeedList>
            </Row>
          </div>
        ))}
      </Surface>
    </div>
  );
}
