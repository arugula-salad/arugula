// Drafts waiting on a project's linked repos (M170): a comment, a review or
// a new issue that a person or their agent wrote, for a person to send
// (M168). The platform reads them through the viewer check, so a private
// repo's drafts show only to those GitHub lets read it. The list is its own
// component so the inbox (M171) can show "a draft waiting for you" with it.
//
// Send (M168) goes out as the person: the platform hands back GitHub's
// authorization of the App, which the browser opens (silent once they've
// authorized it), and GitHub comes back through control to this page with
// `?draft_send=<id>`, which the page reads the draft for: sent and where,
// or why not. With no way to authorize, one of their machines sends it.

import { useEffect, useState } from "preact/hooks";
import type { DraftSent, ForgeDraft, ForgeDrafts } from "../shared/proto.gen";
import { platformFetch, refusalOf } from "../shared/maintenance";
import { session } from "./session";
import { toast } from "./toast";

async function call<T>(method: string, path: string, body?: unknown): Promise<T> {
  const init: RequestInit = { method, credentials: "same-origin" };
  if (body !== undefined) {
    init.headers = { "Content-Type": "application/json" };
    init.body = JSON.stringify(body);
  }
  const r = await platformFetch(path, init);
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

/** The param GitHub's authorization comes back to this page with. */
const BACK = "draft_send";

/** What became of a sent draft, as a line. */
function outcome(d: ForgeDraft): string {
  if (d.state === "sent") return `Sent to GitHub as you${d.sent_url ? "" : "."}`;
  if (d.state === "sending") return "Sending…";
  return d.error ?? "Not sent: it waits here.";
}

/** Send a draft as its person: GitHub's authorization in this browser, or
 * the answer of one of their machines. */
async function sendDraft(d: ForgeDraft): Promise<DraftSent | null> {
  const back = location.pathname + location.search;
  const v = await call<DraftSent>("POST", `/api/forge/drafts/${encodeURIComponent(d.id)}/send`, { via: "browser", back });
  if (v.authorize) {
    location.assign(v.authorize);
    return null;
  }
  return v;
}

/** The draft GitHub's authorization came back for (`?draft_send=`), read
 * once as the page loads, before its router tidies the address: the
 * project page may mount more than once. */
// A session's PR (`pdr_`, M259) comes back here too: its card reads it.
const backFor = new URLSearchParams(location.search).get(BACK)?.replace(/^pdr_.*/, "") || null;
let backDraft: Promise<ForgeDraft> | null = null;

/** Back from GitHub's authorization: the draft it was for, read again,
 * and the param gone from the address. */
export function useSendResult(onBack: () => void) {
  const [sent, setSent] = useState<ForgeDraft | null>(null);
  useEffect(() => {
    if (!backFor) return;
    if (!backDraft) {
      const q = new URLSearchParams(location.search);
      q.delete(BACK);
      const rest = q.toString();
      history.replaceState(history.state, "", location.pathname + (rest ? `?${rest}` : "") + location.hash);
      backDraft = call<ForgeDraft>("GET", `/api/forge/drafts/${encodeURIComponent(backFor)}`);
    }
    let live = true;
    backDraft.then(
      (d) => {
        if (!live) return;
        setSent(d);
        onBack();
      },
      (e: Error) => toast(e.message),
    );
    return () => void (live = false);
  }, []);
  return [sent, setSent] as const;
}

/** A sent (or not) draft's outcome, with GitHub's link. */
export function SendResult({ draft, onClose }: { draft: ForgeDraft; onClose: () => void }) {
  const ok = draft.state === "sent";
  return (
    <div class={`platform-send-result${ok ? " sent" : ""}`} data-send-result={draft.state}>
      <span>
        {draftKind(draft)} on {draftTarget(draft)}: {outcome(draft)}{" "}
        {ok && draft.sent_url && (
          <a href={draft.sent_url} target="_blank" rel="noopener noreferrer" data-sent-url>
            Open on GitHub ↗
          </a>
        )}
      </span>
      <button class="control-linkish" onClick={onClose} aria-label="Dismiss">
        ✕
      </button>
    </div>
  );
}

/** Drafts as a list: the reader's own with Send, each with Discard. */
export function DraftList({
  drafts,
  nameOf,
  onChange,
  onSent,
}: {
  drafts: ForgeDraft[];
  nameOf: (account: string) => string;
  onChange: () => void;
  onSent?: (d: ForgeDraft) => void;
}) {
  const [busy, setBusy] = useState<string | null>(null);
  const me = session.me?.account;
  const send = async (d: ForgeDraft) => {
    setBusy(d.id);
    try {
      const v = await sendDraft(d);
      if (!v) return; // off to GitHub
      onSent?.(v.draft);
      toast(v.machine ? `Sent from ${v.machine}` : "Sent");
      onChange();
    } catch (e) {
      toast((e as Error).message);
      onChange();
    } finally {
      setBusy(null);
    }
  };
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
          {(d.error || d.state !== "waiting") && <div class="dim platform-draft-why">{outcome(d)}</div>}
          <div class="platform-draft-actions">
            {d.author === me && (
              <button class="primary" data-draft-send={d.id} disabled={busy === d.id || d.state === "sending"} onClick={() => send(d)}>
                {busy === d.id ? "Sending…" : "Send"}
              </button>
            )}
            <button class="control-linkish" data-draft-discard={d.id} disabled={busy === d.id || d.state === "sending"} onClick={() => discard(d)}>
              Discard
            </button>
          </div>
        </li>
      ))}
    </ul>
  );
}

/** A project's waiting drafts under a heading, and what became of one just
 * sent; nothing when there are none. */
export function ProjectDrafts({ project, nameOf }: { project: string; nameOf: (account: string) => string }) {
  const { drafts, error, reload } = useProjectDrafts(project);
  const [sent, setSent] = useSendResult(() => void reload());
  if (!error && !drafts?.length && !sent) return null;
  return (
    <>
      <h2>Drafts waiting</h2>
      {sent && <SendResult draft={sent} onClose={() => setSent(null)} />}
      {error && <p class="dim">{error}</p>}
      {drafts && <DraftList drafts={drafts} nameOf={nameOf} onChange={reload} onSent={setSent} />}
    </>
  );
}
