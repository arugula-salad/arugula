// The Docs page (#1494, Labs: `docs`): every Doc this machine keeps, the
// ones an agent wrote and left (#1493), newest first. A Doc outlives its
// pane, so closing the pane isn't losing it: Open shows it again in the
// current session (an open one comes forward), with its content and, if it
// was shared with a link, its share menu. Delete removes it for good, and a
// shared one unshares first; with control out of reach the machine refuses,
// and the reason is shown here.

import { useEffect, useState } from "preact/hooks";
import type { Client } from "../client";
import { opPath, OPS } from "../ops";
import type { DocRow } from "../../shared/proto";
import { ConfirmRemove } from "../../shared/ui/confirm";
import { AppBar } from "./app-bar";
import { directory } from "../hosts";
import { docsRoute, openDoc } from "./docs-route";
import { LinkIcon } from "./shell/icons";

/** The page, while the route says so, the machine has the flag and this is
 * its owner. */
export function DocsPage({ client }: { client: Client }) {
  const [, setTick] = useState(0);
  useEffect(() => {
    const fn = () => setTick((t) => t + 1);
    window.addEventListener("hashchange", fn);
    return () => window.removeEventListener("hashchange", fn);
  }, []);
  if (!docsRoute() || !client.flag("docs") || client.state?.roles) return null;
  return <Page client={client} />;
}

/** "3h ago": when a Doc was last updated. */
function ago(ms: number): string {
  const s = Math.max(0, (Date.now() - ms) / 1000);
  if (s < 90) return "just now";
  if (s < 3600) return `${Math.round(s / 60)}m ago`;
  if (s < 86400) return `${Math.round(s / 3600)}h ago`;
  if (s < 86400 * 30) return `${Math.round(s / 86400)}d ago`;
  return `on ${new Date(ms).toLocaleDateString(undefined, { year: "numeric", month: "short", day: "numeric" })}`;
}

/** Who made it: an agent (`mcp:NAME`), a person's driver, a Doc from
 * before the library (`migrated`), or `unknown` (a block opened by hand). */
function madeBy(by: string): string {
  if (by === "migrated") return "made before Docs were kept";
  if (by === "unknown") return "made by someone";
  return `made by ${by.replace(/^mcp:/, "")}`;
}

function Page({ client }: { client: Client }) {
  const [rows, setRows] = useState<DocRow[] | null>(null);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const [deleting, setDeleting] = useState<DocRow | null>(null);

  const load = async () => {
    try {
      const res = await client.request(OPS["docs.list"].method, opPath("docs.list"));
      const v = await res.json<DocRow[] & { error?: string }>().catch(() => null);
      if (!res.ok || !Array.isArray(v)) throw new Error(v?.error ?? `HTTP ${res.status}`);
      setRows(v);
    } catch (e) {
      setError((e as Error).message);
    }
  };
  useEffect(() => void load(), []);

  const open = async (r: DocRow) => {
    setBusy(true);
    const why = await openDoc(client, r.id);
    if (why) setError(why);
    setBusy(false);
  };

  const remove = async (r: DocRow) => {
    setDeleting(null);
    setBusy(true);
    try {
      const res = await client.request(OPS["doc.delete"].method, opPath("doc.delete", { id: r.id }));
      if (!res.ok) throw new Error(((await res.json<{ error?: string }>().catch(() => null))?.error) ?? `HTTP ${res.status}`);
      setError("");
      await load();
    } catch (e) {
      // A shared Doc that control couldn't unshare says so, and stays.
      setError((e as Error).message);
    }
    setBusy(false);
  };

  return (
    <div class="chat agents-page docs-page" data-docs-page>
      <AppBar client={client} at="docs" />
      <div class="agents-body">
        {/* Whose Docs, and what they are, like the Recipes tab (UI audit #173). */}
        <h3 class="agents-machine" data-docs-machine>
          Docs on {directory.current || "this machine"}
        </h3>
        <p class="agents-lead">The Docs your agents write on this machine, newest first. Closing a Doc's pane keeps it: open it again here.</p>
        {error && (
          <p class="control-error agents-error" data-docs-error>
            {error}
          </p>
        )}
        {!rows && !error && (
          <p class="dim agents-loading" data-docs-loading>
            Reading this machine's Docs…
          </p>
        )}
        {rows && rows.length === 0 && (
          <div class="agents-empty" data-docs-empty>
            <b>Docs your agents write show up here.</b>
          </div>
        )}
        <ul class="agents-cards">
          {(rows ?? []).map((r) => (
            <li key={r.id} class="agents-card" data-doc={r.id} data-shared={String(!!r.shared)} data-open={r.open ?? ""}>
              <div class="agents-card-text">
                <div class="agents-card-head">
                  <b data-doc-title>{r.title}</b>
                  {r.open !== null && r.open !== undefined && (
                    <span class="ws-tag" data-doc-open-now title="Open in a pane now">
                      open
                    </span>
                  )}
                  {r.shared && (
                    <span class="ws-tag" data-doc-shared title="Shared with a link">
                      <LinkIcon /> shared
                    </span>
                  )}
                </div>
                <p class="dim">
                  updated {ago(r.updated)} · {madeBy(r.by)}
                </p>
              </div>
              <div class="agents-buttons">
                <button class="primary small" data-open-doc disabled={busy} title={r.open === null || r.open === undefined ? "Show it in this session" : "It's open: bring it forward"} onClick={() => void open(r)}>
                  Open
                </button>
                <button class="secondary danger small" data-delete-doc disabled={busy} title="Delete it for good" onClick={() => setDeleting(r)}>
                  Delete
                </button>
              </div>
            </li>
          ))}
        </ul>
      </div>
      {deleting && (
        <ConfirmRemove title={`Delete “${deleting.title}”?`} label="Delete" cancel={() => setDeleting(null)} go={() => void remove(deleting)}>
          <p>The Doc and its pictures are removed for good.</p>
          {deleting.shared && <p data-delete-shared>Its link stops working.</p>}
        </ConfirmRemove>
      )}
    </div>
  );
}
