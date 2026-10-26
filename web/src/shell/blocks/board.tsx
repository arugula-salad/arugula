// A project's board as a block (live status board, step 2): a pin onto one
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
import type { PaneId } from "../../shared/proto";
import { BoardPanel } from "../../shared/boards/view";
import { PlatformApiContext } from "../../shared/platform-api";
import { useSubscribe } from "../../shared/ui/hooks";
import { shellPlatformApi } from "../platform-api";
import { platformHref } from "../platform-links";
import { getControlSession } from "../ui/people";
import { registerBlock, type BlockView } from "./view";

/** The block's state: its layout entry, which the daemon says whole. */
export interface BoardState {
  board: string;
}

function BoardBlock({ client, s, shown }: { client: Client; s: BoardState | null; shown: boolean }) {
  const session = getControlSession();
  useSubscribe((fn) => session?.subscribe(fn) ?? (() => {}));
  let body;
  if (!s) body = <p class="board-none dim">…</p>;
  else if (session?.phase === "ready") body = <BoardPanel id={s.board} shown={shown} platform={platformHref} />;
  else
    body = (
      <p class="board-none dim" data-board-needs-control>
        A board is your project's, on Arugula control, and you read it as yourself. Open this machine through control's page, signed in, to see it here.
      </p>
    );
  return (
    <PlatformApiContext.Provider value={shellPlatformApi(client)}>
      <div class="board-block" data-board-block={s?.board}>
        {body}
      </div>
    </PlatformApiContext.Provider>
  );
}

registerBlock("board", (client: Client, _id: PaneId): BlockView => {
  const host = document.createElement("div");
  host.className = "block block-board";
  let state: BoardState | null = null;
  let visible = true;
  const draw = () => render(<BoardBlock client={client} s={state} shown={visible} />, host);
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
    focus: () => host.querySelector<HTMLElement>("button, input")?.focus(),
    dispose() {
      render(null, host);
      host.remove();
    },
  };
});
