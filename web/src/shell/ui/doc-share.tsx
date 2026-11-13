// Sharing a Doc with a link (#1066): its three actions in its bar (Copy
// link, Share and Open in browser: shared/docs/doc-actions.tsx, Arugula #650, #782) and
// the block's menu (Copy link, and once it's shared, Update shared copy and
// Unshare). The daemon does it (`doc_share`
// and the rest, the owner's, never an agent's); this is the buttons, the one
// share dialog, and the confirms. Copy link copies at once (Arugula #708):
// its public link if it has one, else a team Doc's page on the platform,
// else it's disabled. Nothing here makes a public link except the share
// dialog's confirm: the share button opens it.
//
// What the menu knows without asking is in the Doc's state: `shared`
// ({made, updated}, no link) and `platform` (a team Doc, with a page on the
// platform). The links, and
// whether it can be shared at all (this machine has joined a control that
// keeps Docs), are `doc_shares`' answer, asked when the block's state
// arrives and kept here for the menu to read.

import { useEffect, useState } from "preact/hooks";
import { docHooks, type DocState } from "../blocks/doc";
import type { ApiResponse, Client } from "../client";
import { opPath, OPS } from "../ops";
import type { DocLinkUrl, DocShareRequest, PaneId } from "../../shared/proto";
import { ConfirmRemove } from "../../shared/ui/confirm";
import { CopyText } from "../../shared/ui/copy";
import type { MenuItem } from "../../shared/ui/menu";
import { Modal } from "../../shared/ui/modal";
import { useSubscribe } from "../../shared/ui/hooks";
import { ago, when } from "../../shared/time";
import { OpenOutside } from "../blocks/block-states";
import { Count } from "../../shared/ui/state-chip";
import { DocActions, NO_LINK, openTab } from "../../shared/docs/doc-actions";

/** What `doc_shares` said for a Doc that isn't shared. */
interface Can {
  can: boolean;
  why?: string;
}

const can = new Map<PaneId, Can>();

/** A shared Doc's link, for its bar's ↗ (UI audit #196), as `doc_shares`
 * last said it; and who to tell when it changes. */
const links = new Map<PaneId, string>();
/** A team Doc's page on the platform, as `doc_shares` last said it: what
 * Open in browser opens before it's shared. */
const platforms = new Map<PaneId, string>();
const linkWatchers = new Set<() => void>();
const onLinks = (fn: () => void) => {
  linkWatchers.add(fn);
  return () => void linkWatchers.delete(fn);
};

interface Shares {
  url?: string;
  /** A team Doc's page on the platform. */
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
  const platform = ok ? body.platform ?? undefined : undefined;
  if (url !== links.get(id) || platform !== platforms.get(id)) {
    if (url) links.set(id, url);
    else links.delete(id);
    if (platform) platforms.set(id, platform);
    else platforms.delete(id);
    linkWatchers.forEach((fn) => fn());
  }
}

/** In the Doc's bar (UI audit #196, Arugula #650, #782): its three actions,
 * at the right. Copy link copies the Doc's link at once ("Copied ✓" in
 * place, Arugula #708): its public link, else a team Doc's page; with
 * neither it's disabled. Share opens the share dialog, which says what
 * sharing means and makes the public link only on its confirm; it reads
 * Update, with a dot, while the shared copy is older than the Doc. Open in
 * browser opens the link Copy link copies. A viewer with a role has no Share. */
function ShareState({ client, id, s }: { client: Client; id: PaneId; s: DocState }) {
  useSubscribe(onLinks);
  const c = can.get(id);
  const blocked = !s.shared && c?.can === false;
  const share = () => (blocked ? client.toast(c?.why ?? "this Doc can't be shared") : show({ kind: "share", client, id }));
  const copy = () =>
    new Promise<"ok" | false>((done) => {
      void copyLink(client, id, () => done("ok")).then(() => done(false));
    });
  return (
    <DocActions
      none={!s.platform && !s.shared}
      copy={copy}
      open={() => void openLink(client, id)}
      share={client.state?.roles ? undefined : share}
      older={!!s.shared && !!s.shared_older}
      hooks={{ copy: { "data-doc-bar-link": "" }, share: { "data-doc-bar-share": "" } }}
    />
  );
}

docHooks.bar = (client, id, s) => <ShareState client={client} id={id} s={s} />;

docHooks.state = (client, id, s: DocState) => {
  // When it arrives, and each time it becomes shared or isn't, or its link
  // goes older or is updated: the bars and the dialog redraw then, not on every fill.
  const was = seen.get(id);
  const now = `${s.shared ? "shared" : "not"}${s.platform ? "+page" : ""}${s.shared_older ? "-older" : ""}`;
  if (was === now) return;
  seen.set(id, now);
  if (was?.split("-")[0] !== now.split("-")[0]) void refresh(client, id);
  linkWatchers.forEach((fn) => fn());
};
const seen = new Map<PaneId, string>();

// ---------------------------------------------------------------- dialogs

type Open =
  | { kind: "share"; client: Client; id: PaneId }
  | { kind: "closed"; run: () => void }
  | { kind: "update"; run: () => void }
  | { kind: "unshare"; run: () => void };

let shown: Open | null = null;
const listeners = new Set<() => void>();
const show = (o: Open | null) => {
  shown = o;
  listeners.forEach((fn) => fn());
};

/** Share a Doc by its id, or Update its link, with no block showing it
 * (Arugula #819): the Docs page's Share. The same confirms as the block's,
 * and the same outcome (the link copied, or the link now showing the Doc as
 * it is). A shared Doc whose link is current has nothing to update: Share
 * copies its link, as Copy link does, with no dialog. `done` hears the
 * reason it failed, or nothing. */
export function shareClosedDoc(
  client: Client,
  doc: { id: string; shared: { link: string } | null; older: boolean },
  done: (why?: string) => void,
) {
  const fail = (v: { error?: string } | null, res: { status: number }) => done(v?.error ?? `HTTP ${res.status}`);
  /** A request and its JSON, for copySoon to start on before either arrives. */
  const ask = (res: Promise<ApiResponse>) =>
    res.then(async (res) => ({ res, v: await res.json<Partial<DocLinkUrl> & { error?: string }>().catch(() => null) }));
  const go = async (update: boolean) => {
    try {
      const body = { update } satisfies DocShareRequest;
      const asked = ask(client.request(OPS["doc.share"].method, opPath("doc.share", { id: doc.id }), body));
      const copied = update ? null : copySoon(asked.then(({ res, v }) => (res.ok ? v?.url : null)));
      const { res, v } = await asked;
      if (!res.ok || !v?.url) return fail(v, res);
      if (update) client.toast("The link now shows this Doc as it is");
      else if (await copied) client.toast("Link copied");
      done();
    } catch (e) {
      done((e as Error).message);
    }
  };
  const copyCurrent = async () => {
    try {
      const asked = ask(client.request(OPS["doc_link.get"].method, opPath("doc_link.get", { id: doc.shared!.link })));
      const copied = copySoon(asked.then(({ res, v }) => (res.ok ? v?.url : null)));
      const { res, v } = await asked;
      if (!res.ok || !v?.url) return fail(v, res);
      if (await copied) client.toast("Link copied");
      else return done(`Couldn't copy the link: ${v.url}`);
      done();
    } catch (e) {
      done((e as Error).message);
    }
  };
  if (doc.shared && !doc.older) return void copyCurrent();
  show({ kind: doc.shared ? "update" : "closed", run: () => void go(!!doc.shared) });
}

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
  if (o.kind === "closed") {
    return (
      <Modal
        x={false}
        title="Share this Doc with a link?"
        close={close}
        root={{ "data-doc-link-dialog": true, "data-doc-share-dialog": "unshared" }}
        buttons={
          <>
            <button class="secondary" type="button" onClick={close}>
              Cancel
            </button>
            <button
              class="primary"
              type="button"
              data-doc-share-go
              onClick={() => {
                close();
                o.run();
              }}
            >
              Create link
            </button>
          </>
        }
      >
        <p class="dim">Anyone with the link can open it</p>
      </Modal>
    );
  }
  if (o.kind === "update") {
    return (
      <Modal
        alert
        x={false}
        title="Update shared copy?"
        label="Update shared copy"
        close={close}
        root={{ "data-doc-update-dialog": true }}
        buttons={
          <>
            <button class="secondary" type="button" onClick={close}>
              Cancel
            </button>
            <button
              class="secondary"
              type="button"
              data-doc-update-go
              onClick={() => {
                close();
                o.run();
              }}
            >
              Update
            </button>
          </>
        }
      >
        <p>{UPDATE_NOTE}</p>
      </Modal>
    );
  }
  return <ShareDialog client={o.client} id={o.id} close={close} />;
}

/** The share dialog (Arugula #650): what sharing means and its confirm, for
 * a Doc that isn't shared; for one that is, its link and Update shared
 * copy, Unshare and the "shared copy is older" signal. */
function ShareDialog({ client, id, close }: { client: Client; id: PaneId; close: () => void }) {
  useSubscribe(onLinks);
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
    // Start the copy inside the click (copySoon), then say what happened.
    const asked = call(client, id, "doc_share");
    const copied = copySoon(asked.then(({ ok, body: b }) => (ok ? b.url : null)));
    const { ok, body: b } = await asked;
    if (!ok) client.toast(b.error ?? "that didn't work");
    else if (b.url) {
      setMade(b.url);
      links.set(id, b.url);
      linkWatchers.forEach((fn) => fn());
      if (await copied) client.toast("Link copied");
    }
    setBusy(false);
  };
  const unshare = () => show({ kind: "unshare", run: () => void run(client, id, "doc_unshare", () => client.toast("Unshared")) });
  const update = () => show({ kind: "update", run: () => void run(client, id, "doc_share_update", () => client.toast("The link now shows this Doc as it is")) });
  /** The link's own buttons, in the footer. */
  const linkButtons = shared ? (
    <>
      <button class="secondary danger" type="button" data-doc-share-unshare onClick={unshare}>
        Unshare
      </button>
      <button class="secondary" type="button" data-doc-share-update onClick={update}>
        Update shared copy
      </button>
      {url && <OpenOutside href={url} data-doc-bar-open />}
    </>
  ) : (
    <button class="primary" type="button" data-doc-share-go disabled={busy} onClick={() => void confirm()}>
      Create link
    </button>
  );
  const linkBody = (
    <>
      <p class="dim">Anyone with the link can open it</p>
      {shared && (
        <>
          {url ? <CopyText text={url} share data-doc-link-url /> : <p class="dim">Loading the link…</p>}
          {s?.shared && s.shared_older && (
            <Count
              for="you"
              data-doc-bar-older
              title={`The link shows the Doc as it was ${ago(s.shared.updated, Date.now(), "long")} (${when(s.shared.updated)}). A section changed since. One tap makes the link show the Doc as it is; the link stays the same.`}
              onClick={() => void run(client, id, "doc_share_update", () => client.toast("The link now shows this Doc as it is"))}
            >
              Shared copy is older · Update
            </Count>
          )}
        </>
      )}
    </>
  );
  const root = { "data-doc-link-dialog": true, "data-doc-share-dialog": shared ? "shared" : "unshared" };
  return (
    <Modal
      x={false}
      title={shared ? "Link to this Doc" : "Share this Doc with a link?"}
      close={close}
      root={root}
      buttons={
        shared ? (
          <>
            {linkButtons}
            <button class="secondary" type="button" onClick={close}>
              Done
            </button>
          </>
        ) : (
          <>
            <button class="secondary" type="button" onClick={close}>
              Cancel
            </button>
            {linkButtons}
          </>
        )
      }
    >
      {linkBody}
    </Modal>
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

/** Copy a link that a request is still fetching (Arugula #877). WebKit (the
 * desktop app, Safari, the phone) lets a click write the clipboard only
 * while the click is still going: a `writeText` after an awaited request is
 * refused, and Copy link said it couldn't copy. A ClipboardItem takes a
 * promise, so the write starts here, in the click, and the text follows.
 * Call it before any await. Where ClipboardItem isn't there, it waits and
 * writes, as before. */
async function copySoon(url: Promise<string | null | undefined>): Promise<boolean> {
  if (typeof ClipboardItem === "function" && typeof navigator.clipboard?.write === "function") {
    const blob = url.then((u) => {
      if (!u) throw new Error("no link");
      return new Blob([u], { type: "text/plain" });
    });
    try {
      await navigator.clipboard.write([new ClipboardItem({ "text/plain": blob })]);
      return true;
    } catch {
      // Refused, or no link: say so below from what the request said.
      if (!(await url.catch(() => null))) return false;
    }
  }
  const u = await url.catch(() => null);
  return u ? copy(u) : false;
}

/** Say a call's refusal, or run `then` with its answer. */
async function run(client: Client, id: PaneId, method: string, then: (b: Shares) => void | Promise<void>) {
  const { ok, body } = await call(client, id, method);
  if (!ok) return client.toast(body.error ?? "that didn't work");
  await then(body);
}

export { NO_LINK };

/** Copy the Doc's link, with no dialog and no public link made: the public
 * link it already has (anyone can open it), else a team Doc's page. `done`
 * hears that it was copied ("Copied ✓" in place). */
async function copyLink(client: Client, id: PaneId, done: () => void) {
  // Ask and start the copy in the same tick, inside the click (copySoon).
  const asked = call(client, id, "doc_shares");
  const copied = copySoon(asked.then(({ ok, body: b }) => (ok ? (b.url ?? b.platform) : null)));
  const { ok, body: b } = await asked;
  if (!ok) return client.toast(b.error ?? "that didn't work");
  const url = b.url ?? b.platform;
  if (!url) return client.toast(NO_LINK);
  if (await copied) done();
  else client.toast(`Couldn't copy the link: ${url}`);
}

/** Open the Doc's link, the one Copy link copies, in a new tab. As last
 * said, at once (a new tab opened after a wait can be blocked); else asked. */
async function openLink(client: Client, id: PaneId) {
  const have = links.get(id) ?? platforms.get(id);
  if (have) return openTab(have);
  await run(client, id, "doc_shares", async (b) => {
    const url = b.url ?? b.platform;
    if (url) openTab(url);
    else client.toast(NO_LINK);
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
    run: () => void copyLink(client, id, () => client.toast("Link copied")),
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
