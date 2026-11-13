// A project's board as a block (boards, step 2): a pin onto one
// of the platform's boards (crates/daemon/src/board_block.rs, config
// `{board}`), drawn with the platform page's board view
// (../../shared/boards/view). The daemon keeps nothing of it but which
// board it is: the zones and rows are Arugula control's, read as the person
// looking and written the same way, so no machine holds a copy.
//
// It reads and listens only while the block is shown (an `only_shown`
// block, mux/blocks.rs): a pinned board in a tab nobody has open asks for
// nothing. Without a control session on this page there is nothing to read
// it as, and it says so.

import { render } from "preact";
import type { Client } from "../client";
import type { PaneId, PlatformProjects } from "../../shared/proto";
import { BoardPanel } from "../../shared/boards/view";
import { boards } from "../../shared/boards/store";
import { platformFetch } from "../../shared/maintenance";
import { askText, type PromptChoice } from "../../shared/ui/menu";
import { PlatformApiContext, type PlatformApi } from "../../shared/platform-api";
import { useSubscribe } from "../../shared/ui/hooks";
import { BlockLoading, EmptyState } from "./block-states";
import { shellPlatformApi } from "../platform-api";
import { platformHref } from "../platform-links";
import { getControlSession } from "../ui/people";
import { registerBlock, type BlockView } from "./view";

/** The block's state: its layout entry, which the daemon says whole. */
export interface BoardState {
  board: string;
}

function BoardBlock({ client, id, s, shown }: { client: Client; id: PaneId; s: BoardState | null; shown: boolean }) {
  const session = getControlSession();
  useSubscribe((fn) => session?.subscribe(fn) ?? (() => {}));
  let body;
  if (!s) body = <BlockLoading what="the board" />;
  else if (session?.phase === "ready") body = <BoardPanel id={s.board} shown={shown} platform={platformHref} />;
  else
    body = (
      <EmptyState
        data-board-needs-control
        title="A board is on Arugula cloud."
        why="It's your project's, and you read it as yourself. Open this machine in Arugula cloud, signed in, to see it here."
      />
    );
  return (
    <PlatformApiContext.Provider value={boardApi(client, id)}>
      <div class="board-block" data-board-block={s?.board}>
        {body}
      </div>
    </PlatformApiContext.Provider>
  );
}

/** The shell's api for a board in this pane: a machine Doc opens beside it, and an agent block of this layout comes forward where it is. */
function boardApi(client: Client, id: PaneId): PlatformApi {
  const base = shellPlatformApi(client);
  return {
    ...base,
    machineDoc: base.machineDoc && { ...base.machineDoc, open: (machine, doc) => base.machineDoc!.open(machine, doc, id) },
    goToPane(machine, pane, quote) {
      const n = Number(pane);
      if (!quote && base.machineDoc?.here(machine) && client.info(n)) return client.setActive(n);
      base.goToPane(machine, pane, quote);
    },
  };
}

/** A board's id in what was typed: `brd_…` alone, or in a board page's
 * link. */
export function boardIdIn(typed: string): string | null {
  return /\bbrd_[0-9a-f]{32}\b/i.exec(typed)?.[0].toLowerCase() ?? null;
}

/** The open boards in the reader's projects, as choices: "Project · Board".
 * Those people made come first, then those from Plane epics (started ones
 * first), marked with the epic's key: "Project · ARUGU-10 · Launch". */
async function boardChoices(): Promise<PromptChoice[]> {
  const r = await platformFetch("/api/projects", { credentials: "same-origin" });
  if (!r.ok) return [];
  const { projects } = (await r.json()) as PlatformProjects;
  const lists = await Promise.all(
    projects.map((p) =>
      boards
        .list(p.id)
        .then((l) =>
          l.boards
            .filter((b) => b.closed_at == null)
            .map((b) => ({ value: b.id, label: `${p.name} · ${b.source ? `${b.source.key} · ` : ""}${b.title}`, rank: b.source ? (b.source.started ? 1 : 2) : 0 })),
        )
        .catch(() => []),
    ),
  );
  // A stable sort keeps each group in the order the projects gave.
  return lists.flat().sort((a, b) => a.rank - b.rank);
}

/** "Board…": a block for one of the reader's boards, beside `from`. */
export async function pinBoard(client: Client, o: { from: PaneId }) {
  if (getControlSession()?.phase !== "ready") {
    return client.toast("A board is read as you, through Arugula cloud: open this machine in Arugula cloud, signed in");
  }
  const choices = boardChoices();
  const v = (await askText("Board", "", "board, or its link", choices))?.trim();
  if (!v) return;
  const typed = v.toLowerCase();
  const all = await choices.catch(() => []);
  const board = boardIdIn(v) ?? all.find((c) => c.label.toLowerCase() === typed || c.label.toLowerCase().endsWith(`· ${typed}`))?.value;
  if (!board) return client.toast(`No board of yours is called ${v}`);
  await client.openBlock({ type: "board", config: { board }, local: true, split: o.from, from_pane: o.from }, "couldn't pin that board");
}

registerBlock("board", (client: Client, id: PaneId): BlockView => {
  const host = document.createElement("div");
  host.className = "block block-board";
  let state: BoardState | null = null;
  let visible = true;
  const draw = () => render(<BoardBlock client={client} id={id} s={state} shown={visible} />, host);
  draw();
  return {
    host,
    setVisible(v) {
      host.style.display = v ? "" : "none";
      if (v !== visible) {
        visible = v;
        draw();
      }
    },
    update(s) {
      state = s as BoardState;
      draw();
    },
    title: () => "Board",
    text: () => host.textContent ?? "",
    focus: () => host.querySelector<HTMLElement>("button, input")?.focus({ preventScroll: true }),
    dispose() {
      render(null, host);
      host.remove();
    },
  };
});
