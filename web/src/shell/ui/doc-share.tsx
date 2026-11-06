// Sharing a Doc with a link (#1066): two icon buttons in its bar (link and
// share, Arugula #650) and the block's menu (Copy link, and once it's
// shared, Update shared copy and Unshare). The daemon does it (`doc_share`
// and the rest, the owner's, never an agent's); this is the buttons, the one
// share dialog, and the confirms. Copy link copies at once (Arugula #708):
// the Doc's copy on the platform, which only its owner can open until it's
// shared, else its public link if it has one, else it's disabled. Nothing
// here makes a public link except the share dialog's confirm: the share
// button opens it. Share with… (#418 lane E) adds a section to the dialog.
//
// What the menu knows without asking is in the Doc's state: `shared`
// ({made, updated}, no link) and `platform` (it has a copy). The links, and
// whether it can be shared at all (this machine has joined a control that
// keeps Docs), are `doc_shares`' answer, asked when the block's state
// arrives and kept here for the menu to read.

import { useEffect, useRef, useState } from "preact/hooks";
import { docHooks, type DocState } from "../blocks/doc";
import type { Client } from "../client";
import type { PaneId } from "../../shared/proto";
import { ConfirmRemove } from "../../shared/ui/confirm";
import { CopyText } from "../../shared/ui/copy";
import type { MenuItem } from "../../shared/ui/menu";
import { useModal } from "../../shared/ui/modal";
import { useSubscribe } from "../../shared/ui/hooks";
import { ago, when } from "../../shared/time";
import { OpenOutside } from "../blocks/block-states";
import { LinkIcon } from "./shell/icons";
import { Count } from "../../shared/ui/state-chip";

const ShareIcon = () => (
  <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
    <path d="M4 12v7a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2v-7" />
    <path d="M16 6l-4-4-4 4" />
    <path d="M12 2v13" />
  </svg>
);

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
  /** Its copy on the platform: the page, which opens for the Doc's owner. */
  platform?: string | null;
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

/** In the Doc's bar (UI audit #196, Arugula #650): two icon buttons. Link
 * copies the Doc's link at once ("Copied ✓" in place, Arugula #708): its
 * copy on the platform, else its public link; with neither it's disabled.
 * Share opens the share dialog, which says what sharing means and makes the
 * public link only on its confirm. A dot on Share says the shared copy is
 * older than the Doc. */
function ShareState({ client, id, s }: { client: Client; id: PaneId; s: DocState }) {
  useSubscribe(onLinks);
  const [copied, setCopied] = useState<"ok" | "private" | null>(null);
  const timer = useRef<number | undefined>(undefined);
  if (client.state?.roles) return null;
  const c = can.get(id);
  const blocked = !s.shared && c?.can === false;
  const open = () => (blocked ? client.toast(c?.why ?? "this Doc can't be shared") : show({ kind: "share", client, id }));
  const link = () =>
    void copyLink(client, id, (private_) => {
      setCopied(private_ ? "private" : "ok");
      clearTimeout(timer.current);
      timer.current = window.setTimeout(() => setCopied(null), 1500);
    });
  const older = !!s.shared && !!s.shared_older;
  const none = !s.platform && !s.shared;
  const title = none ? NO_LINK : copied === "private" ? `Copied: ${ONLY_YOU}` : "Copy link";
  return (
    <span class="doc-bar-share" data-doc-bar-share-group>
      <button type="button" class="doc-bar-icon" data-doc-bar-link data-copied={copied ?? undefined} disabled={none} title={title} aria-label="Copy link" onClick={link}>
        {copied ? <span class="doc-bar-copied">Copied ✓</span> : <LinkIcon />}
      </button>
      <button
        type="button"
        class="doc-bar-icon"
        data-doc-bar-share
        data-older={older ? "" : undefined}
        title={older ? "Share… (the shared copy is older than this Doc)" : "Share…"}
        aria-label={older ? "Share… (the shared copy is older)" : "Share…"}
        onClick={open}
      >
        <ShareIcon />
      </button>
    </span>
  );
}

docHooks.bar = (client, id, s) => <ShareState client={client} id={id} s={s} />;

docHooks.state = (client, id, s: DocState) => {
  // When it arrives, and each time it becomes shared or isn't, or its copy
  // goes older or is updated: the bars and the dialog redraw then, not on every fill.
  const was = seen.get(id);
  const now = `${s.shared ? "shared" : "not"}${s.platform ? "+copy" : ""}${s.shared_older ? "-older" : ""}`;
  if (was === now) return;
  seen.set(id, now);
  if (was?.split("-")[0] !== now.split("-")[0]) void refresh(client, id);
  linkWatchers.forEach((fn) => fn());
};
const seen = new Map<PaneId, string>();

// ---------------------------------------------------------------- dialogs

type Open =
  | { kind: "share"; client: Client; id: PaneId }
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
  // UI audit #150: the update's dialog takes the focus and closes on
  // Escape (the share dialog and Unshare's ConfirmRemove do their own).
  const ref = useRef<HTMLDivElement>(null);
  useModal(ref, () => show(null), !!shown && shown.kind === "update");
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
  return <ShareDialog client={o.client} id={o.id} close={close} />;
}

/** The share dialog (Arugula #650): what sharing means and its confirm, for
 * a Doc that isn't shared; for one that is, its link and Update shared
 * copy, Unshare and the "shared copy is older" signal. Share with… (#418
 * lane E) adds a section here. */
function ShareDialog({ client, id, close }: { client: Client; id: PaneId; close: () => void }) {
  useSubscribe(onLinks);
  const ref = useRef<HTMLDivElement>(null);
  useModal(ref, close, true);
  const [made, setMade] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const s = client.blocks.get(id)?.state as DocState | null | undefined;
  useEffect(() => {
    // A Doc that is shared already: fetch its link for the dialog.
    if (s?.shared && !links.has(id)) void refresh(client, id);
  }, [!!s?.shared]);
  const url = links.get(id) ?? made;
  const shared = !!s?.shared || made !== null;
  const confirm = async () => {
    setBusy(true);
    await run(client, id, "doc_share", async (b) => {
      if (!b.url) return;
      setMade(b.url);
      links.set(id, b.url);
      linkWatchers.forEach((fn) => fn());
      if (await copy(b.url)) client.toast("Link copied");
    });
    setBusy(false);
  };
  return (
    <div class="prompt-backdrop" role="dialog" aria-modal="true" aria-label={shared ? "Link to this Doc" : "Share this Doc with a link?"} onPointerDown={(e) => e.target === e.currentTarget && close()}>
      <div ref={ref} class="prompt control-prompt" data-doc-link-dialog data-doc-share-dialog={shared ? "shared" : "unshared"}>
        <h2>{shared ? "Link to this Doc" : "Share this Doc with a link?"}</h2>
        <p class="hint">Anyone with this link can open the Doc, with no account. It shows the Doc as it is now.</p>
        {shared ? (
          <>
            {url ? <CopyText text={url} share data-doc-link-url /> : <p class="hint">Loading the link…</p>}
            {s?.shared && (
              <p class="hint" data-doc-bar-shared title={when(s.shared.updated)}>
                Shared; the link shows it as it was {ago(s.shared.updated, Date.now(), "long")}.
              </p>
            )}
            {s?.shared && s.shared_older && (
              <Count
                for="you"
                data-doc-bar-older
                title="A section changed after the link was updated. One tap makes the link show the Doc as it is; the link stays the same."
                onClick={() => void run(client, id, "doc_share_update", () => client.toast("The link now shows this Doc as it is"))}
              >
                Shared copy is older · Update
              </Count>
            )}
            <div class="prompt-buttons">
              <button
                class="secondary danger"
                type="button"
                data-doc-share-unshare
                onClick={() => show({ kind: "unshare", run: () => void run(client, id, "doc_unshare", () => client.toast("Unshared")) })}
              >
                Unshare
              </button>
              <button
                class="secondary"
                type="button"
                data-doc-share-update
                onClick={() => show({ kind: "update", run: () => void run(client, id, "doc_share_update", () => client.toast("The link now shows this Doc as it is")) })}
              >
                Update shared copy
              </button>
              {url && <OpenOutside href={url} data-doc-bar-open />}
              <button class="secondary" type="button" onClick={close}>
                Done
              </button>
            </div>
          </>
        ) : (
          <div class="prompt-buttons">
            <button class="secondary" type="button" onClick={close}>
              Cancel
            </button>
            <button class="primary" type="button" data-doc-share-go disabled={busy} onClick={() => void confirm()}>
              Create link
            </button>
          </div>
        )}
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

export const NO_LINK = "Not on the platform yet: sign in to Arugula control to get a link";
export const ONLY_YOU = "only you can open it until it's shared";

/** Copy the Doc's link, with no dialog and no public link made: the public
 * link it already has (anyone can open it), else its copy on the platform
 * (only its owner can, until #418 E). `done` hears whether
 * only its owner can open what was copied ("Copied ✓" in place). */
async function copyLink(client: Client, id: PaneId, done: (private_: boolean) => void) {
  await run(client, id, "doc_shares", async (b) => {
    const url = b.url ?? b.platform;
    if (!url) return client.toast(NO_LINK);
    if (await copy(url)) done(url === b.platform && !b.url);
    else client.toast(`Couldn't copy the link: ${url}`);
  });
}

/** The Doc block's share items, for its menu: the owner's. */
export function docShareItems(client: Client, id: PaneId): MenuItem[] {
  if (client.state?.roles) return [];
  const s = client.blocks.get(id)?.state as DocState | null | undefined;
  const none = !s?.platform && !s?.shared;
  const copyItem: MenuItem = {
    label: "Copy link",
    disabled: none,
    hint: none ? NO_LINK : undefined,
    run: () => void copyLink(client, id, (private_) => client.toast(private_ ? `Link copied: ${ONLY_YOU}` : "Link copied")),
  };
  if (s?.shared) {
    return [
      copyItem,
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
  return [copyItem, "separator"];
}
