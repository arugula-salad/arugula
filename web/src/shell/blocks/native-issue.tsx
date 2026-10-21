// A project's own issue as an issue block (M203): a pin onto the
// platform's issue (`pin: {kind: "issue", global_id: "iss_…"}`), which the
// daemon reads as its machine for the owner (crates/daemon/src/forge/
// native.rs). This draws it with the platform page's issue view
// (../../shared/issues/view), the way a Cloud workspace block once drew
// control's views: the view reads through `IssueApiContext`.
//
// - On control's page, signed in, with chat on (issues come with it), the
//   view reads and writes as the person looking, through control, as the
//   platform page does: its thread, the timeline and every change, live.
//   Nobody needs an agent grant to see their own issue.
// - Otherwise (a machine's own page, or someone the project doesn't
//   have), it shows what the daemon read, read only, with a link to the
//   platform page. The daemon reads only with its threads.read grant.
//
// The foot says where it was read from, and when Arugula control can't be
// reached, that this is what it said last.

import { useEffect, useState } from "preact/hooks";
import type { ForgePin, NativeIssue, PlatformProjects } from "../../shared/proto";
import { chat } from "../../shared/chat-store";
import { IssueApiContext, IssueView, type IssueApi, type IssuePlace } from "../../shared/issues/view";
import { IssueError, issues } from "../../shared/issues/store";
import { ago } from "../../shared/issues/text";
import { platformFetch } from "../../shared/maintenance";
import { useSubscribe } from "../../shared/ui/hooks";
import { platformHref } from "../platform-links";
import { getControlSession } from "../ui/people";

/** What the daemon says of it. */
export interface NativeIssueState {
  kind: "issue";
  native: true;
  pin: ForgePin;
  number: number | null;
  project: string | null;
  title: string | null;
  issue: NativeIssue | null;
  read_ms: number | null;
  error: string | null;
  loading: boolean;
}

export const isNativeIssue = (s: unknown): s is NativeIssueState => (s as { native?: unknown } | null)?.native === true;

/** The reader's projects, as control says them: asked once a page. */
let projects: Promise<PlatformProjects> | null = null;
function readProjects(): Promise<PlatformProjects> {
  projects ??= platformFetch("/api/projects", { credentials: "same-origin" }).then(async (r) => {
    if (!r.ok) throw new IssueError(r.status, `control said ${r.status}`);
    return (await r.json()) as PlatformProjects;
  });
  projects.catch(() => (projects = null));
  return projects;
}

/** The reader's place in `project`, or null when it isn't theirs. */
function placeIn(all: PlatformProjects, project: string): IssuePlace | null {
  const p = all.projects.find((x) => x.id === project);
  if (!p) return null;
  if (!p.team) return { project, members: [{ account: all.me.account, name: all.me.name || all.me.login }], writer: true };
  return { project, members: p.team.members.map((m) => ({ account: m.account, name: m.name })), writer: p.team.role !== "viewer" };
}

/** The issue as the daemon read it: read only. */
function blockApi(s: NativeIssueState): IssueApi {
  const nope = (what: string) => Promise.reject(new IssueError(403, `${what} on the platform page`));
  return {
    get: () => (s.issue ? Promise.resolve(s.issue) : Promise.reject(new IssueError(403, s.error ?? "this machine hasn't read it"))),
    events: () => Promise.resolve([]),
    change: () => nope("Change it"),
    thread: () => nope("Its comments are"),
    known: () => s.issue ?? undefined,
    all: () => (s.issue ? [s.issue] : []),
  };
}

/** How the page reads it: as the person (`me`), as the daemon did
 * (`block`), or not yet known. */
type Reader = { as: "me"; place: IssuePlace } | { as: "block" } | null;

function useReader(s: NativeIssueState): Reader {
  const session = getControlSession();
  useSubscribe((fn) => session?.subscribe(fn) ?? (() => {}));
  useSubscribe((fn) => chat.subscribe(fn));
  const [reader, setReader] = useState<Reader>(null);
  const signedIn = session?.phase === "ready";
  const on = chat.on;
  const id = s.pin.global_id;
  useEffect(() => {
    if (!signedIn || on === false) return setReader({ as: "block" });
    if (on === null) return;
    let live = true;
    (async () => {
      // Its project: the daemon's word, else the person's own read.
      const project = s.project ?? (await issues.get(id)).project;
      const place = placeIn(await readProjects(), project);
      if (live) setReader(place ? { as: "me", place } : { as: "block" });
    })().catch(() => live && setReader({ as: "block" }));
    return () => void (live = false);
  }, [signedIn, on, id, s.project]);
  return reader;
}

/** Where it was read from, and that it's what it said last when the
 * platform can't be reached. */
function Foot({ s, me }: { s: NativeIssueState; me: boolean }) {
  if (s.pin.stale && !me) return <span data-issue-foot>Arugula control can't be reached now: this is what it said {s.read_ms ? ago(s.read_ms) : "last"}.</span>;
  return <span data-issue-foot>{me ? "Read as you, through Arugula control." : "Read through Arugula control by this machine, for its owner."}</span>;
}

export function NativeIssueBlock({ s, refresh }: { s: NativeIssueState; refresh: () => void }) {
  const reader = useReader(s);
  const id = s.pin.global_id;
  // What this page hears changed, the daemon reads again too, so its
  // text (and its agents' read) keeps up.
  const me = reader?.as === "me";
  useEffect(() => (me ? chat.onObject(id, refresh) : undefined), [id, me]);
  const name = nativeIssueTitle(s);
  const there = platformHref(`/issues/${id}`);
  return (
    <div class="review issue-block" data-issue-block={id} data-issue-reader={reader?.as} data-stale={s.pin.stale ? "" : undefined}>
      <div class="review-bar">
        <span class="review-path">
          <b>{name}</b> <span class="ws-tag" title="A project's own issue, read through Arugula control">control</span>
        </span>
        {there && (
          <a class="browser-open" href={there} target="_blank" rel="noopener noreferrer" title="Open this issue on the platform page" data-issue-platform-link>
            Open ↗
          </a>
        )}
      </div>
      <div class="review-body issue-block-body">
        {reader === null ? (
          <p class="dim">Reading it from Arugula control…</p>
        ) : reader.as === "me" ? (
          <IssueView key={id} id={id} place={reader.place} refresh={s.issue?.updated_at} foot={<Foot s={s} me />} Missing={({ what }) => <FromBlock s={s} what={what} />} />
        ) : (
          <FromBlock s={s} />
        )}
      </div>
    </div>
  );
}

/** What the daemon read, read only; or why there's nothing. */
function FromBlock({ s, what }: { s: NativeIssueState; what?: string }) {
  const session = getControlSession();
  if (!s.issue) {
    return (
      <div class="issue-page" data-issue-missing>
        <p class="dim">{s.loading ? "Reading it from Arugula control…" : (what ?? s.error ?? "Arugula control didn't say what it is.")}</p>
        <p class="dim">
          <Foot s={s} me={false} />
        </p>
      </div>
    );
  }
  const place: IssuePlace = {
    project: s.issue.project,
    members: [],
    writer: false,
    nameOf: (a) => (session && a === session.account ? session.name || session.login : undefined),
  };
  return (
    <IssueApiContext.Provider value={blockApi(s)}>
      <IssueView key={`${s.pin.global_id}-block`} id={s.pin.global_id} place={place} refresh={s.read_ms} foot={<Foot s={s} me={false} />} />
    </IssueApiContext.Provider>
  );
}

/** Its plain text, as `capture --text` gives it. */
export function nativeIssueText(s: NativeIssueState): string {
  const i = s.issue;
  const lines = [i ? `#${i.number} ${i.title} (${i.state})` : s.number ? `#${s.number} ${s.title ?? ""}`.trim() : s.pin.global_id];
  if (i?.body.trim()) lines.push(i.body.trim());
  if (s.pin.stale) lines.push(`(as Arugula control said it last: ${s.pin.stale})`);
  if (s.error) lines.push(s.error);
  return lines.join("\n");
}

/** What to call it in tabs and lists. */
export function nativeIssueTitle(s: NativeIssueState): string {
  // The daemon's word, else what this page read as the person.
  const i = s.issue ?? issues.known(s.pin.global_id);
  if (i) return `#${i.number} ${i.title}`;
  return s.number ? `#${s.number}${s.title ? ` ${s.title}` : ""}` : "issue";
}
