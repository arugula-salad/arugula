// A person's test results in a Doc. The shared renderer gets a callback
// only from the owner's live machine view; shared snapshots stay read-only.
import { useState } from "preact/hooks";
import type { ComponentChildren } from "preact";

export interface ChecklistItem {
  id: string;
  text: string;
  owner: { role: string; pane: number };
  status: "todo" | "passed" | "failed";
  note: string;
  screenshot: string | null;
  revision: number;
  report_revision: number;
  report_owner: { role: string; pane: number } | null;
  sent: number;
  delivery_error: string | null;
}

export type Feedback = Pick<ChecklistItem, "revision" | "status" | "note" | "screenshot"> & { item: string };
export type Report = (feedback: Feedback, screenshot?: File) => Promise<void>;

export function checklistMarkdown(items: ChecklistItem[]): string {
  return items.map((t) => {
    const result = t.status === "failed" ? `  **Failed — ${t.sent === t.report_revision ? "sent to owner" : "waiting to send to owner"}.**\n` : "";
    const note = t.note ? t.note.split("\n").map((line) => `  > ${line}\n`).join("") : "";
    const recipient = t.status === "failed" && t.report_owner ? `  Report recipient: ${t.report_owner.role}, agent %${t.report_owner.pane}.\n` : "";
    return `- [${t.status === "passed" ? "x" : " "}] ${t.text} (owner: ${t.owner.role}, agent %${t.owner.pane})\n${recipient}${result}${note}${t.screenshot ? `  ![Test screenshot](assets/${t.screenshot})\n` : ""}`;
  }).join("");
}

function TestItem({ task, report, image }: { task: ChecklistItem; report?: Report; image: (name: string) => ComponentChildren }) {
  const [editing, setEditing] = useState(false);
  const [note, setNote] = useState(task.note);
  const [editingRevision, setEditingRevision] = useState(task.revision);
  const [file, setFile] = useState<File | undefined>();
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const submit = async (status: ChecklistItem["status"]) => {
    if (!report || busy) return;
    setBusy(true);
    setError("");
    try {
      await report({ item: task.id, revision: status === "failed" ? editingRevision : task.revision, status, note: status === "failed" ? note : task.note, screenshot: task.screenshot }, status === "failed" ? file : undefined);
      setEditing(false);
      setFile(undefined);
    } catch (e) {
      setError(e instanceof Error ? e.message : "Couldn't save the result. Try again.");
    } finally {
      setBusy(false);
    }
  };
  return (
    <li class="doc-test" data-doc-test={task.id} data-test-status={task.status}>
      <label class="doc-test-label">
        <input type="checkbox" checked={task.status === "passed"} disabled={!report || busy} onChange={(e) => void submit(e.currentTarget.checked ? "passed" : "todo")} />
        <span>{task.text}</span>
      </label>
      <p class="doc-test-meta">
        <strong>{task.status === "failed" ? "Failed" : task.status === "passed" ? "Passed" : "Not tested"}</strong> · Assigned to {task.owner.role}
        {task.status === "failed" && <span> · {task.sent === task.report_revision ? "Sent to owner" : "Saved; waiting to send"}{task.report_owner && task.report_owner.pane !== task.owner.pane ? ` (${task.report_owner.role}); report again to send to the new owner` : ""}</span>}
      </p>
      {task.note && <blockquote class="doc-test-note">{task.note}</blockquote>}
      {task.screenshot && <div class="doc-test-image">{image(task.screenshot)}</div>}
      {report && !editing && <button type="button" class="secondary small" disabled={busy} onClick={() => { setNote(task.note); setEditingRevision(task.revision); setEditing(true); setError(""); }}>Report a failure</button>}
      {report && editing && (
        <form class="doc-test-form" onSubmit={(e) => { e.preventDefault(); void submit("failed"); }}>
          <label>What went wrong?
            <textarea class="field" value={note} rows={3} disabled={busy} aria-label={`Failure note for ${task.text}`} onInput={(e) => setNote(e.currentTarget.value)} />
          </label>
          <label>Screenshot (optional)
            <input type="file" accept="image/png,image/jpeg,image/gif,image/webp" disabled={busy} onChange={(e) => setFile(e.currentTarget.files?.[0])} />
          </label>
          <div class="doc-test-actions">
            <button type="submit" class="primary" disabled={busy || (!note.trim() && !file && !task.screenshot)}>{busy ? "Saving…" : `Report failure to ${task.owner.role}`}</button>
            <button type="button" class="secondary" disabled={busy} onClick={() => { setEditing(false); setFile(undefined); }}>Cancel</button>
          </div>
        </form>
      )}
      {error && <p role="alert">{error}</p>}
      {error && editing && editingRevision !== task.revision && <button type="button" class="secondary" onClick={() => { setEditingRevision(task.revision); setError(""); }}>Review latest result and keep my draft</button>}
    </li>
  );
}

export function Checklist({ items, report, image }: { items: ChecklistItem[]; report?: Report; image: (name: string) => ComponentChildren }) {
  return <ul class="doc-checklist" aria-label="Test checklist">{items.map((task) => <TestItem key={task.id} task={task} report={report} image={image} />)}</ul>;
}
