// The workspace and agents blocks have their code in the
// Labs chunk. Here, in the main bundle, is what stands in their slot: a
// "Loading…" while the chunk arrives, then the real block inside it. A
// machine without the block's flag gets a plain note instead (an old saved
// layout can still hold such a block), and nothing is fetched for it.

import type { Client, FlagName } from "../client";
import { loadLabs, type Labs } from "../labs-load";
import type { BlockType, PaneId } from "../../shared/proto";
import { showLabsOff } from "./labs-off";
import { registerBlock, type BlockView } from "./view";

const LABS_BLOCKS = ["workspace", "agents"] as const;
type LabsBlock = (typeof LABS_BLOCKS)[number];

/** The Labs flag each needs. */
const FLAGS: Record<LabsBlock, FlagName> = { workspace: "chant-workspaces", agents: "agent-delegation" };
const TITLES: Record<LabsBlock, string> = { workspace: "Workspace", agents: "Team agents" };

function rendererOf(labs: Labs, type: LabsBlock) {
  const of = { workspace: labs.workspaceBlock, agents: labs.agentsBlock };
  return of[type];
}

function lazyView(type: LabsBlock, client: Client, id: PaneId): BlockView {
  const host = document.createElement("div");
  host.className = "block block-loading";
  host.textContent = `Loading the ${TITLES[type].toLowerCase()} block…`;
  let inner: BlockView | null = null;
  let gone = false;
  let visible = true;
  let state: unknown;
  let hasState = false;
  let size: [number, number] | null = null;
  let wantFocus = false;
  let off: (() => void) | null = null;

  const decide = () => {
    // The daemon's features arrive just after its layout: until they do, we
    // don't know whether this machine has the block's flag on.
    if (gone || inner || client.features === null) return;
    off?.();
    off = null;
    const flag = FLAGS[type];
    if (!client.flag(flag)) {
      showLabsOff(host, client, flag);
      return;
    }
    loadLabs().then(
      (labs) => {
        if (gone) return;
        const view = rendererOf(labs, type)(client, id);
        inner = view;
        host.className = "";
        host.style.display = "contents";
        host.textContent = "";
        host.append(view.host);
        view.setVisible(visible);
        if (hasState) view.update(state);
        if (size) view.layout?.(size[0], size[1]);
        if (wantFocus) view.focus();
      },
      (e) => {
        console.error(`the ${type} block didn't load`, e);
        host.textContent = `Couldn't load this ${TITLES[type].toLowerCase()} block.`;
      },
    );
  };
  off = client.subscribe(decide);
  decide();

  return {
    host,
    setVisible: (v) => {
      visible = v;
      if (inner) inner.setVisible(v);
      else host.style.display = v ? "" : "none";
    },
    update: (s) => {
      state = s;
      hasState = true;
      inner?.update(s);
    },
    title: () => inner?.title() ?? TITLES[type],
    text: () => inner?.text() ?? host.textContent ?? "",
    focus: () => {
      if (inner) inner.focus();
      else wantFocus = true;
    },
    layout: (cols, rows) => {
      size = [cols, rows];
      inner?.layout?.(cols, rows);
    },
    closing: () => inner?.closing?.(),
    dispose: () => {
      gone = true;
      off?.();
      inner?.dispose();
      host.remove();
    },
  };
}

for (const type of LABS_BLOCKS) registerBlock(type as BlockType, (client, id) => lazyView(type, client, id));
