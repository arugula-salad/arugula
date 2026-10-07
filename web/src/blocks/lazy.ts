// The Fountain, studio app and workspace blocks are Labs: their code is in
// the Labs chunk. Here, in the main bundle, is what stands in their slot: a
// "Loading…" while the chunk arrives, then the real block inside it. A
// machine without labs gets a plain note instead (an old saved layout can
// still hold such a block), and nothing is fetched for it.

import type { Client } from "../client";
import { loadLabs, type Labs } from "../labs-load";
import type { BlockType, PaneId } from "../proto";
import { registerBlock, type BlockView } from "./view";

const LABS_BLOCKS = ["fountain", "app", "workspace"] as const;
type LabsBlock = (typeof LABS_BLOCKS)[number];

const TITLES: Record<LabsBlock, string> = { fountain: "Fountain", app: "App", workspace: "Workspace" };

function rendererOf(labs: Labs, type: LabsBlock) {
  return type === "fountain" ? labs.fountainBlock : type === "app" ? labs.appBlock : labs.workspaceBlock;
}

function lazyView(type: LabsBlock, client: Client, id: PaneId): BlockView {
  const host = document.createElement("div");
  host.className = "block block-loading";
  host.textContent = "Loading…";
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
    // don't know whether this machine has labs.
    if (gone || inner || client.features === null) return;
    off?.();
    off = null;
    if (!client.hasLabs()) {
      host.textContent = "This block is in Labs";
      host.className = "block block-unknown";
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
        host.textContent = `Couldn't load this ${TITLES[type]} block`;
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
