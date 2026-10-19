// Sharing a Doc with a link (#1066), from its block's menu: Share link…,
// and once it's shared, Copy link, Update shared copy and Unshare. The
// daemon does it (`doc_share` and the rest, the owner's, never an agent's);
// this is the menu, the one dialog that shows the link, and the confirms.
//
// What the menu knows without asking is in the Doc's state: `shared`
// ({made, updated}, no link). Whether it can be shared at all (this machine
// has joined a control that keeps Docs) is `doc_shares`' answer, asked when
// the block's state arrives and kept here for the menu to read.

import { useEffect, useState } from "preact/hooks";
import { docHooks, type DocState } from "../blocks/doc";
import type { Client } from "../client";
import type { PaneId } from "../proto";
import { ConfirmRemove } from "./confirm";
import { CopyText } from "./copy";
import type { MenuItem } from "./menu";

/** What `doc_shares` said for a Doc that isn't shared. */
interface Can {
  can: boolean;
  why?: string;
}

const can = new Map<PaneId, Can>();

interface Shares {
  url?: string;
  made?: number;
  updated?: number;
  shared?: null;
  can?: boolean;
  why?: string;
  error?: string;
}

async function call(client: Client, id: PaneId, method: string): Promise<{ ok: boolean; body: Shares }> {
  try {
    const res = await client.request("POST", `/api/blocks/${id}/call/${method}`, {});
    return { ok: res.ok, body: await res.json<Shares>().catch(() => ({})) };
  } catch {
    return { ok: false, body: { error: "couldn't reach the machine" } };
  }
}

/** Ask whether this Doc can be shared, and keep the answer for its menu. */
async function refresh(client: Client, id: PaneId) {
  if (client.state?.roles || (client.features !== null && !client.flag("docs"))) return;
  const { ok, body } = await call(client, id, "doc_shares");
  if (ok && body.shared === null) can.set(id, { can: body.can !== false, why: body.why });
  else can.delete(id);
}

docHooks.state = (client, id, s: DocState) => {
  // When it arrives, and each time it becomes shared or isn't.
  const was = seen.get(id);
  const now = s.shared ? "shared" : "not";
  if (was === now) return;
  seen.set(id, now);
  void refresh(client, id);
};
const seen = new Map<PaneId, string>();

// ---------------------------------------------------------------- dialogs

type Open =
  | { kind: "link"; title: string; url: string }
  | { kind: "update"; run: () => void }
  | { kind: "unshare"; run: () => void };

let shown: Open | null = null;
const listeners = new Set<() => void>();
const show = (o: Open | null) => {
  shown = o;
  listeners.forEach((fn) => fn());
};

export const UPDATE_NOTE = "Update changes what the link shows. To take something back, Unshare.";

export function DocShareLayer() {
  const [, setTick] = useState(0);
  useEffect(() => {
    const fn = () => setTick((t) => t + 1);
    listeners.add(fn);
    return () => {
      listeners.delete(fn);
    };
  }, []);
  const o = shown;
  if (!o) return null;
  const close = () => show(null);
  if (o.kind === "unshare") {
    return (
      <ConfirmRemove
        title="Unshare this Doc?"
        label="Unshare"
        go={() => {
          close();
          o.run();
        }}
        cancel={close}
      >
        <p>The link stops working at once. Sharing it again makes a new link; the old one never comes back.</p>
      </ConfirmRemove>
    );
  }
  if (o.kind === "update") {
    return (
      <div class="prompt-backdrop" onPointerDown={(e) => e.target === e.currentTarget && close()}>
        <div class="prompt control-prompt" role="alertdialog" aria-modal="true" aria-label="Update shared copy" data-doc-update-dialog onKeyDown={(e) => e.key === "Escape" && close()}>
          <h2>Update shared copy?</h2>
          <p>{UPDATE_NOTE}</p>
          <div class="prompt-buttons">
            <button type="button" onClick={close}>
              Cancel
            </button>
            <button
              type="button"
              data-doc-update-go
              onClick={() => {
                close();
                o.run();
              }}
            >
              Update
            </button>
          </div>
        </div>
      </div>
    );
  }
  return (
    <div class="prompt-backdrop" role="dialog" aria-label={o.title} onPointerDown={(e) => e.target === e.currentTarget && close()}>
      <div class="prompt control-prompt" data-doc-link-dialog onKeyDown={(e) => e.key === "Escape" && close()}>
        <h2>{o.title}</h2>
        <p class="hint">Anyone with this link can open the Doc, with no account. It shows the Doc as it is now.</p>
        <CopyText text={o.url} share data-doc-link-url />
        <div class="prompt-buttons">
          <button type="button" onClick={close}>
            Done
          </button>
        </div>
      </div>
    </div>
  );
}

// ------------------------------------------------------------------- menu

async function copy(url: string): Promise<boolean> {
  try {
    await navigator.clipboard.writeText(url);
    return true;
  } catch {
    return false;
  }
}

/** Say a call's refusal, or run `then` with its answer. */
async function run(client: Client, id: PaneId, method: string, then: (b: Shares) => void | Promise<void>) {
  const { ok, body } = await call(client, id, method);
  if (!ok) return client.toast(body.error ?? "that didn't work");
  await then(body);
}

/** The Doc block's share items, for its menu: the owner's, and only where
 * the `docs` flag is on. */
export function docShareItems(client: Client, id: PaneId): MenuItem[] {
  if (client.state?.roles || (client.features !== null && !client.flag("docs"))) return [];
  const s = client.blocks.get(id)?.state as DocState | null | undefined;
  if (s?.shared) {
    return [
      {
        label: "Copy link",
        run: () =>
          void run(client, id, "doc_shares", async (b) => {
            if (!b.url) return client.toast("this Doc isn't shared any more");
            if (await copy(b.url)) client.toast("Link copied");
            else show({ kind: "link", title: "Link to this Doc", url: b.url });
          }),
      },
      {
        label: "Update shared copy",
        run: () =>
          show({
            kind: "update",
            run: () => void run(client, id, "doc_share_update", () => client.toast("The link now shows this Doc as it is")),
          }),
      },
      {
        label: "Unshare",
        danger: true,
        run: () => show({ kind: "unshare", run: () => void run(client, id, "doc_unshare", () => client.toast("Unshared")) }),
      },
      "separator",
    ];
  }
  const c = can.get(id);
  return [
    {
      label: "Share link…",
      disabled: c?.can === false,
      hint: c?.can === false ? c.why : undefined,
      run: () =>
        void run(client, id, "doc_share", async (b) => {
          if (!b.url) return;
          const copied = await copy(b.url);
          show({ kind: "link", title: copied ? "Link to this Doc (copied)" : "Link to this Doc", url: b.url });
        }),
    },
    "separator",
  ];
}
