// Sharing a Doc with a link (#1066), from its block's menu: Share link…,
// and once it's shared, Copy link, Update shared copy and Unshare. The
// daemon does it (`doc_share` and the rest, the owner's, never an agent's);
// this is the menu, the one dialog that shows the link, and the confirms.
//
// What the menu knows without asking is in the Doc's state: `shared`
// ({made, updated}, no link). Whether it can be shared at all (this machine
// has joined a control that keeps Docs) is `doc_shares`' answer, asked when
// the block's state arrives and kept here for the menu to read.

import { useEffect, useRef, useState } from "preact/hooks";
import { docHooks, type DocState } from "../blocks/doc";
import type { Client } from "../client";
import type { PaneId } from "../../shared/proto";
import { ConfirmRemove } from "../../shared/ui/confirm";
import { CopyText } from "../../shared/ui/copy";
import { CopyLinkButton } from "../../shared/ui/copy-link";
import type { MenuItem } from "../../shared/ui/menu";
import { useModal } from "../../shared/ui/modal";
import { useSubscribe } from "../../shared/ui/hooks";
import { ago, when } from "../../shared/time";
import { OpenOutside } from "../blocks/block-states";
import { LinkIcon } from "./shell/icons";

/** What `doc_shares` said for a Doc that isn't shared. */
interface Can {
  can: boolean;
  why?: string;
}

const can = new Map<PaneId, Can>();

/** A shared Doc's link, for its bar's ↗ (UI audit #196), as `doc_shares`
 * last said it; and who to tell when it changes. */
const links = new Map<PaneId, string>();
const linkWatchers = new Set<() => void>();
const onLinks = (fn: () => void) => {
  linkWatchers.add(fn);
  return () => void linkWatchers.delete(fn);
};

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
  if (client.state?.roles) return;
  const { ok, body } = await call(client, id, "doc_shares");
  if (ok && body.shared === null) can.set(id, { can: body.can !== false, why: body.why });
  else can.delete(id);
  const url = ok ? body.url : undefined;
  if (url !== links.get(id)) {
    if (url) links.set(id, url);
    else links.delete(id);
    linkWatchers.forEach((fn) => fn());
  }
}

/** In the Doc's bar (UI audit #196): the Copy link button (#411), "shared"
 * once it has a link, with when its copy was last updated, and ↗ to open
 * the link. This Doc lives on one machine, so it has no team link yet: the
 * button's click is the public link's flow (Share link…, or Copy link once
 * it's shared) and the ▾ holds the rest. */
function ShareState({ client, id, s }: { client: Client; id: PaneId; s: DocState }) {
  useSubscribe(onLinks);
  const items = docShareItems(client, id);
  const main = items[0];
  // Unshared, "Share link…" is the click and also in the ▾; shared, the click is Copy link.
  const more = s.shared ? items.slice(1) : items;
  const menu = more.filter((i, n) => i !== "separator" || n < more.length - 1);
  const url = links.get(id);
  return (
    <>
      {main && typeof main === "object" && "run" in main && (
        <CopyLinkButton
          link={null}
          onClick={() => (main.disabled ? client.toast(main.hint ?? "this Doc can't be shared") : main.run())}
          menu={menu}
        />
      )}
      {s.shared && (
        <>
          <span class="ws-tag doc-bar-shared" data-doc-bar-shared title={`Shared with a link; the link shows it as it was ${ago(s.shared.updated, Date.now(), "long")} (${when(s.shared.updated)})`}>
            <LinkIcon /> shared
          </span>
          {url && <OpenOutside href={url} data-doc-bar-open />}
        </>
      )}
    </>
  );
}

docHooks.bar = (client, id, s) => <ShareState client={client} id={id} s={s} />;

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
  // UI audit #150: the link's and the update's dialog take the focus and
  // close on Escape (Unshare's ConfirmRemove does its own).
  const ref = useRef<HTMLDivElement>(null);
  useModal(ref, () => show(null), !!shown && shown.kind !== "unshare");
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
        <div class="prompt control-prompt" role="alertdialog" aria-modal="true" aria-label="Update shared copy" data-doc-update-dialog ref={ref}>
          <h2>Update shared copy?</h2>
          <p>{UPDATE_NOTE}</p>
          <div class="prompt-buttons">
            <button class="secondary" type="button" onClick={close}>
              Cancel
            </button>
            <button class="secondary"
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
    <div class="prompt-backdrop" role="dialog" aria-modal="true" aria-label={o.title} onPointerDown={(e) => e.target === e.currentTarget && close()}>
      <div ref={ref} class="prompt control-prompt" data-doc-link-dialog>
        <h2>{o.title}</h2>
        <p class="hint">Anyone with this link can open the Doc, with no account. It shows the Doc as it is now.</p>
        <CopyText text={o.url} share data-doc-link-url />
        <div class="prompt-buttons">
          <button class="secondary" type="button" onClick={close}>
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

/** The Doc block's share items, for its menu: the owner's. */
export function docShareItems(client: Client, id: PaneId): MenuItem[] {
  if (client.state?.roles) return [];
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
