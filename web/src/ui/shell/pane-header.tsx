// The row above a pane, when a tab has more than one.

import type { Client } from "../../client";
import type { PaneId, Rect } from "../../proto";
import type { Cell } from "../cells";
import { len } from "../panes/geometry";

export /** What names a pane, above it: what it's running (its title, without the
 * spinner a program puts in front), its project or directory, the
 * machine when it isn't the tab's, and which pane has the keys. A label
 * only: the divider under it stays draggable. */
function PaneHeader({ client, id, rect, cell, active }: { client: Client; id: PaneId; rect: Rect; cell: Cell; active: boolean }) {
  const info = client.info(id);
  const title = client.title(id) || info?.command || `pane %${id}`;
  const cwd = client.cwd(id);
  const where = info?.project?.name ?? (cwd ? cwd.split("/").filter(Boolean).pop() ?? "/" : null);
  const m = client.machine(id);
  return (
    <div
      class={active ? "pane-header active" : "pane-header"}
      data-pane-header={id}
      aria-current={active ? "true" : undefined}
      title={active ? "This pane has the keyboard" : undefined}
      style={{ left: len(rect.x * cell.width), top: len((rect.y - 1) * cell.height), width: len(rect.cols * cell.width), height: len(cell.height) }}
    >
      <span class="pane-header-title">{title}</span>
      {where && title !== where ? <span class="pane-header-where">{where}</span> : null}
      {m ? <span class="pane-header-where">{m.name ?? m.sprite}</span> : null}
    </div>
  );
}
