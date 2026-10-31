// "Waiting for your approval" at the top of a project's Triage (Arugula
// #359): the writes its agents asked to make to a connected Plane project,
// each with Approve, Edit and Drop. Approving sends it as that agent's, with
// the "via Arugula" footer added then; editing changes its text, never the
// footer. Only owners and editors see the list, so a viewer's is empty.

import { useEffect, useState } from "preact/hooks";
import type { TrackerPendingWrite, TrackerPendingWrites, TrackerWrite } from "../../shared/proto.gen";
import { platformFetch } from "../../shared/maintenance";
import { askText } from "../../shared/ui/menu";
import { toast } from "../toast";

async function call<T>(method: string, path: string, body?: unknown): Promise<T> {
  const r = await platformFetch(path, {
    method,
    credentials: "same-origin",
    headers: body === undefined ? {} : { "Content-Type": "application/json" },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  const v = (await r.json().catch(() => ({}))) as { error?: string };
  if (!r.ok) throw new Error(v.error ?? `Request failed (${r.status})`);
  return v as T;
}

/** What the write says it will do, in a few words. */
export function kindWords(w: TrackerWrite): string {
  switch (w.kind) {
    case "create":
      return "Create";
    case "comment":
      return "Comment";
    case "close":
      return w.as === "cancelled" ? "Cancel" : "Close";
    case "reopen":
      return "Reopen";
    case "edit":
      return "Edit";
  }
}

/** The text a person may change: a comment's, a new item's body, an edit's body (or title). */
export function editable(w: TrackerWrite): string | null {
  switch (w.kind) {
    case "create":
      return w.body ?? "";
    case "comment":
      return w.text;
    case "edit":
      return w.body ?? w.title;
    default:
      return null;
  }
}

/** The write with that text in place of the old. */
export function withText(w: TrackerWrite, text: string): TrackerWrite {
  switch (w.kind) {
    case "create":
      return { ...w, body: text };
    case "comment":
      return { ...w, text };
    case "edit":
      return w.body !== null && w.body !== undefined ? { ...w, body: text } : { ...w, title: text };
    default:
      return w;
  }
}

/** The fields a create or edit sets, in one line: state, module, parent, type, labels. */
export function fieldsLine(w: TrackerWrite): string {
  if (w.kind !== "create" && w.kind !== "edit") return "";
  const labels = [...w.labels.map((l) => `+${l}`), ...(w.kind === "edit" ? w.labels_remove.map((l) => `-${l}`) : [])];
  const parts = [
    w.state && `state ${w.state}`,
    w.module && `module ${w.module}`,
    w.parent && `parent ${w.parent}`,
    w.type && `type ${w.type}`,
    labels.length > 0 && `labels ${labels.join(" ")}`,
  ].filter(Boolean);
  return parts.join(" · ");
}

/** What the write shows as its text (a title first, for a new item), then its fields. */
export function shown(w: TrackerWrite): string {
  const fields = fieldsLine(w);
  const text = (() => {
    switch (w.kind) {
      case "create":
        return w.body ? `${w.title}: ${w.body}` : w.title;
      case "comment":
        return w.text;
      case "edit":
        return [w.title, w.body].filter(Boolean).join(": ");
      default:
        return "";
    }
  })();
  return fields ? (text ? `${text} (${fields})` : fields) : text;
}

export function WaitingWrites({ project }: { project: string }) {
  const [writes, setWrites] = useState<TrackerPendingWrite[]>([]);
  const [busy, setBusy] = useState(false);
  const base = `/api/projects/${encodeURIComponent(project)}/tracker-writes`;
  const load = () =>
    call<TrackerPendingWrites>("GET", base).then(
      (v) => setWrites(v.writes),
      () => setWrites([]),
    );
  useEffect(() => void load(), [project]);
  const act = async (f: () => Promise<unknown>) => {
    setBusy(true);
    try {
      await f();
    } catch (e) {
      toast((e as Error).message);
    } finally {
      setBusy(false);
      await load();
    }
  };
  if (writes.length === 0) return null;
  return (
    <section class="triage-waiting" aria-label="Waiting for your approval" data-waiting-writes>
      <h2>Waiting for your approval</h2>
      <ul class="platform-list">
        {writes.map((w) => {
          const text = editable(w.write);
          return (
            <li key={w.id} class="triage-row" data-waiting-write={w.id}>
              <span class="triage-label">
                {kindWords(w.write)} {w.target ?? "new item"}
              </span>
              <span class="triage-title">{shown(w.write)}</span>
              <span class="triage-who dim">
                {w.person}'s agent on {w.machine_name}
              </span>
              <button class="primary" disabled={busy} data-waiting-approve onClick={() => void act(() => call("POST", `${base}/${w.id}/approve`))}>
                Approve
              </button>
              <button
                class="secondary"
                disabled={busy || text === null}
                data-waiting-edit
                onClick={() =>
                  void act(async () => {
                    const t = await askText(`Edit the ${kindWords(w.write).toLowerCase()}`, text ?? "");
                    if (t !== null && t.trim()) await call("PATCH", `${base}/${w.id}`, withText(w.write, t));
                  })
                }
              >
                Edit
              </button>
              <button class="secondary danger" disabled={busy} data-waiting-drop onClick={() => void act(() => call("POST", `${base}/${w.id}/drop`))}>
                Drop
              </button>
            </li>
          );
        })}
      </ul>
    </section>
  );
}
