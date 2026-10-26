// Shared Docs… in the account menu (#1066): every Doc this account has
// shared with a link, from control's list. Control knows a link's size,
// dates and a rough count of opens, and nothing of what it says; the link
// itself (it holds the key) is on the machine that made it. So Copy link
// asks that machine, while it's online, and Unshare asks it too (it clears
// the Doc's block) or, when it can't be reached, control's own delete. Open
// (#1494) asks the machine for the Doc's block by the link's id (control has
// no Doc id), and shows that pane there, as the swarm opens a pane on its host.

import { useEffect, useState } from "preact/hooks";
import type { ControlSession } from "../control";
import { opPath, OPS } from "../ops";
import type { DocOpenRequest, DocOpened } from "../../shared/proto";
import { ConfirmRemove } from "../../shared/ui/confirm";
import { CopyText } from "../../shared/ui/copy";
import { getFleet } from "./hosts";

/** One row of control's `GET /api/doc-links`. */
interface Row {
  id: string;
  daemon: string;
  size: number;
  made: number;
  updated: number;
  opens: number;
}

export const OFFLINE = "its machine is offline";

const size = (b: number) => (b < 1 << 20 ? `${Math.max(1, Math.round(b / 1024))} KB` : `${(b / (1 << 20)).toFixed(1)} MB`);
const when = (ms: number) => new Date(ms).toLocaleDateString(undefined, { year: "numeric", month: "short", day: "numeric" });
const opened = (n: number) => (n === 0 ? "not opened yet" : n === 1 ? "opened about once" : `opened about ${n} times`);

export function SharedDocs({ s, close }: { s: ControlSession; close: () => void }) {
  const [rows, setRows] = useState<Row[] | null>(null);
  const [err, setErr] = useState("");
  const [removing, setRemoving] = useState<Row | null>(null);
  const [link, setLink] = useState<string | null>(null);
  const [note, setNote] = useState("");

  const load = async () => {
    try {
      const res = await fetch("/api/doc-links", { credentials: "same-origin" });
      if (!res.ok) throw new Error(`control said ${res.status}`);
      const v = (await res.json()) as Row[] | { links: Row[] };
      setRows(Array.isArray(v) ? v : v.links);
    } catch (e) {
      setErr((e as Error).message);
    }
  };
  useEffect(() => void load(), []);

  /** The machine that made it, if it is online and this page reaches it. */
  const machine = (r: Row) => {
    const d = s.daemons.find((x) => x.id === r.daemon);
    return d?.online && getFleet()?.clientOf(d.name) ? d : undefined;
  };
  const name = (r: Row) => s.daemons.find((x) => x.id === r.daemon)?.name ?? "a machine that was removed";

  const copy = async (r: Row) => {
    const d = machine(r)!;
    try {
      const res = await getFleet()!.request(d.name, OPS["doc_link.get"].method, opPath("doc_link.get", { id: r.id }));
      const v = await res.json<{ url?: string; error?: string }>();
      if (!res.ok || !v.url) throw new Error(v.error ?? `${d.name} said ${res.status}`);
      setErr("");
      setNote("");
      await navigator.clipboard.writeText(v.url).then(
        () => setNote("Link copied"),
        () => setLink(v.url!),
      );
    } catch (e) {
      setErr((e as Error).message);
    }
  };

  const openIt = async (r: Row) => {
    const d = machine(r)!;
    setNote("");
    try {
      const res = await getFleet()!.request(d.name, OPS["doc.open"].method, opPath("doc.open"), { link: r.id } satisfies DocOpenRequest);
      const v = await res.json<Partial<DocOpened> & { error?: string }>().catch(() => null);
      if (!res.ok || typeof v?.pane !== "number") throw new Error(v?.error ?? `${d.name} said ${res.status}`);
      setErr("");
      getFleet()!.open(d.name, v.pane);
      close();
    } catch (e) {
      setErr((e as Error).message);
    }
  };

  const unshare = async (r: Row) => {
    setRemoving(null);
    setNote("");
    try {
      const d = machine(r);
      if (d) {
        const res = await getFleet()!.request(d.name, OPS["doc_link.delete"].method, opPath("doc_link.delete", { id: r.id }));
        if (res.ok) return void (await load());
        // The machine is there but wouldn't (or didn't know it): control's own.
      }
      const res = await fetch(`/api/doc-links/${r.id}`, { method: "DELETE", credentials: "same-origin" });
      if (!res.ok) throw new Error(((await res.json().catch(() => ({}))) as { error?: string }).error ?? `control said ${res.status}`);
      setErr("");
      await load();
    } catch (e) {
      setErr((e as Error).message);
    }
  };

  return (
    <div data-shared-docs>
      <h2>Shared Docs</h2>
      <p class="hint">
        Docs you shared with a link. Control keeps them encrypted and can't read them; the link, with its key, is on the machine that shared it.
      </p>
      {err && <p class="control-error">{err}</p>}
      {note && <p class="hint" data-shared-docs-note>{note}</p>}
      {rows && rows.length === 0 && <p class="hint">No Doc is shared.</p>}
      <ul class="rules-list">
        {(rows ?? []).map((r) => (
          <li key={r.id} data-shared-doc={r.id}>
            <span class="rule-text">
              <b>{name(r)}</b>
              <br />
              <span class="dim">
                {size(r.size)} · shared {when(r.made)} · updated {when(r.updated)} · {opened(r.opens)}
              </span>
            </span>
            <button data-open-shared-doc disabled={!machine(r)} title={machine(r) ? undefined : OFFLINE} onClick={() => void openIt(r)}>
              Open
            </button>
            <button data-copy-doc-link disabled={!machine(r)} title={machine(r) ? undefined : OFFLINE} onClick={() => void copy(r)}>
              Copy link
            </button>
            <button class="quiet-danger" data-unshare-doc onClick={() => setRemoving(r)}>
              Unshare
            </button>
          </li>
        ))}
      </ul>
      {link && <CopyText text={link} share data-doc-link-url />}
      <div class="prompt-buttons">
        <button type="button" onClick={close}>
          Done
        </button>
      </div>
      {removing && (
        <ConfirmRemove title="Unshare this Doc?" label="Unshare" go={() => void unshare(removing)} cancel={() => setRemoving(null)}>
          <p>The link stops working at once. Sharing it again makes a new link; the old one never comes back.</p>
        </ConfirmRemove>
      )}
    </div>
  );
}
