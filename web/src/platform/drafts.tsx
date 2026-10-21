// Drafts waiting on a project's linked repos (M170): a comment, a review or
// a new issue that a person or their agent wrote, for a person to send
// (M168). The platform reads them through the viewer check, so a private
// repo's drafts show only to those GitHub lets read it. The list is its own
// component so the inbox (M171) can show "a draft waiting for you" with it.

import { useEffect, useState } from "preact/hooks";
import type { ForgeDraft, ForgeDrafts } from "../shared/proto.gen";
import { platformFetch, refusalOf } from "../shared/maintenance";
import { toast } from "./toast";

async function call<T>(method: string, path: string): Promise<T> {
  const r = await platformFetch(path, { method, credentials: "same-origin" });
  const v = (await r.json().catch(() => ({}))) as T & { error?: string; reason?: string };
  if (!r.ok) throw new Error(refusalOf(r.status, v));
  return v;
}

/** A project's waiting drafts: `null` until the platform has said. */
export function useProjectDrafts(project: string) {
  const [drafts, setDrafts] = useState<ForgeDraft[] | null>(null);
  const [error, setError] = useState("");
  const load = () =>
    call<ForgeDrafts>("GET", `/api/projects/${encodeURIComponent(project)}/drafts`).then(
      (v) => {
        setDrafts(v.drafts);
        setError("");
      },
      (e: Error) => setError(e.message),
    );
  useEffect(() => void load(), [project]);
  return { drafts, error, reload: load };
}

/** The reader's own waiting drafts, across projects (the inbox's "a draft
 * waiting for you"): `null` until the platform has said. */
export function useMyDrafts() {
  const [drafts, setDrafts] = useState<ForgeDraft[] | null>(null);
  const [error, setError] = useState("");
  const load = () =>
    call<ForgeDrafts>("GET", "/api/forge/drafts").then(
      (v) => {
        setDrafts(v.drafts);
        setError("");
      },
      (e: Error) => setError(e.message),
    );
  useEffect(() => void load(), []);
  return { drafts, error, reload: load };
}

const KIND: Record<string, string> = { comment: "Comment", issue: "New issue" };
const EVENT: Record<string, string> = { APPROVE: "approve", REQUEST_CHANGES: "request changes", COMMENT: "comment" };

/** What it is: "Review (approve)", "Comment", "New issue". */
export function draftKind(d: ForgeDraft): string {
  if (d.kind === "review") return `Review (${EVENT[d.event ?? ""] ?? "comment"})`;
  return KIND[d.kind] ?? d.kind;
}

/** Where it goes: `owner/name#7 Title`, or the repo and title of a new issue. */
export function draftTarget(d: ForgeDraft): string {
  if (d.number == null) return `${d.repo}: ${d.title ?? ""}`;
  return `${d.repo}#${d.number}${d.item_title ? ` ${d.item_title}` : ""}`;
}

/** Who wrote it: an agent (on which machine) for its person, or the person. */
export function draftBy(d: ForgeDraft, nameOf: (account: string) => string): string {
  const author = nameOf(d.author);
  if (!d.agent) return author;
  const on = d.machine_name ? ` on ${d.machine_name}` : "";
  return `${d.agent.replace(/^mcp:/, "")}${on}, for ${author}`;
}

const firstLine = (body: string) => body.split("\n").find((l) => l.trim()) ?? "";

/** Drafts as a list, each with Discard. */
export function DraftList({ drafts, nameOf, onChange }: { drafts: ForgeDraft[]; nameOf: (account: string) => string; onChange: () => void }) {
  const [busy, setBusy] = useState<string | null>(null);
  const discard = async (d: ForgeDraft) => {
    setBusy(d.id);
    try {
      await call("POST", `/api/forge/drafts/${encodeURIComponent(d.id)}/discard`);
      toast("Draft discarded");
      onChange();
    } catch (e) {
      toast((e as Error).message);
    } finally {
      setBusy(null);
    }
  };
  return (
    <ul class="platform-list platform-drafts" data-drafts>
      {drafts.map((d) => (
        <li key={d.id} data-draft={d.id}>
          <div class="platform-draft-head">
            <span class="platform-draft-kind">{draftKind(d)}</span>
            <span>{draftTarget(d)}</span>
          </div>
          <div class="dim">{draftBy(d, nameOf)}</div>
          {d.body.trim() && <div class="platform-draft-body">{firstLine(d.body)}</div>}
          <button class="control-linkish platform-draft-discard" data-draft-discard={d.id} disabled={busy === d.id} onClick={() => discard(d)}>
            Discard
          </button>
        </li>
      ))}
    </ul>
  );
}

/** A project's waiting drafts under a heading; nothing when there are none. */
export function ProjectDrafts({ project, nameOf }: { project: string; nameOf: (account: string) => string }) {
  const { drafts, error, reload } = useProjectDrafts(project);
  if (!error && !drafts?.length) return null;
  return (
    <>
      <h2>Drafts waiting</h2>
      {error && <p class="dim">{error}</p>}
      {drafts && <DraftList drafts={drafts} nameOf={nameOf} onChange={reload} />}
    </>
  );
}
