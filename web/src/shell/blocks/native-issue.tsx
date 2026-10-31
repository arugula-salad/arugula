// A project's own issue as an issue block (M203): a pin onto the
// platform's issue (`pin: {kind: "issue", global_id: "iss_…"}`), which the
// daemon reads as its machine for the owner (crates/arugulad-forge/src/
// native.rs). This draws it with the platform page's issue view
// (../../shared/issues/view), the way a Cloud workspace block once drew
// control's views: the view reads through `IssueApiContext`.
//
// - On control's page, signed in, with chat on (issues come with it), the
//   view reads and writes as the person looking, through control, as the
//   platform page does: its thread, the timeline and every change, live.
//   Nobody needs an agent grant to see their own issue.
// - Otherwise (a machine's own page, or someone the project doesn't
//   have), the machine's owner sees what the daemon read for them, read
//   only, with a link to the platform page (`native_issue`, the owner's
//   call: the daemon reads only with its threads.read grant). Anyone else
//   who sees the pane gets `#N` and that they can't read it here: the
//   block's state, which everyone who sees the pane gets, holds nothing
//   more of it.
//
// The header, facts and source are ItemBlock's (#410); where it was read
// from is the source icon's tooltip, and only when Arugula control can't be
// reached does a line say that this is what it said last.

import { useEffect, useState } from "preact/hooks";
import type { Client } from "../client";
import type { ForgePin, NativeIssue, PlatformProjects } from "../../shared/proto";
import { chat } from "../../shared/chat-store";
import { IssueApiContext, IssueView, type IssueApi, type IssuePlace } from "../../shared/issues/view";
import { IssueError, issues } from "../../shared/issues/store";
import { ItemBlock, type ItemFact } from "../../shared/ui/item-block";
import { Time } from "../../shared/ui/time";
import { platformFetch } from "../../shared/maintenance";
import { useSubscribe } from "../../shared/ui/hooks";
import { platformHref } from "../platform-links";
import { BlockLoading } from "./block-states";
import { getControlSession } from "../ui/people";

/** What the daemon says of it to everyone who sees the pane. */
export interface NativeIssueState {
  kind: "issue";
  native: true;
  pin: ForgePin;
  number: number | null;
  /** The daemon holds a read of it, for its owner. */
  read: boolean;
  read_ms: number | null;
  error: string | null;
  loading: boolean;
}

/** What the daemon read, for the machine's owner only (`native_issue`). */
interface OwnersRead {
  issue: NativeIssue | null;
  number: number | null;
  project: string | null;
  title: string | null;
}

/** The owner's read through the daemon: null for anyone else (403), or
 * when it can't be asked. */
async function ownersRead(client: Client, block: number): Promise<OwnersRead | null> {
  try {
    const res = await client.request("POST", `/api/blocks/${block}/call/native_issue`, {});
    return res.ok ? await res.json<OwnersRead>() : null;
  } catch {
    return null;
  }
}

/** The owner's last read of each issue on this page, for its tab. */
const ownersReads = new Map<string, OwnersRead>();

/** The owner's read, asked again whenever the daemon reads it again. */
function useOwnersRead(client: Client, block: number, s: NativeIssueState): OwnersRead | null | undefined {
  const [got, setGot] = useState<OwnersRead | null | undefined>(undefined);
  useEffect(() => {
    let live = true;
    void ownersRead(client, block).then((r) => {
      if (r) ownersReads.set(s.pin.global_id, r);
      else ownersReads.delete(s.pin.global_id);
      if (live) setGot(r);
    });
    return () => void (live = false);
  }, [block, s.pin.global_id, s.read_ms, s.error, s.read]);
  return got;
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

/** The issue as the daemon read it for its owner: read only. */
function blockApi(issue: NativeIssue): IssueApi {
  const nope = (what: string) => Promise.reject(new IssueError(403, `${what} on the platform page`));
  return {
    get: () => Promise.resolve(issue),
    events: () => Promise.resolve([]),
    change: () => nope("Change it"),
    thread: () => nope("Its comments are"),
    known: () => issue,
    all: () => [issue],
  };
}

/** How the page reads it: as the person (`me`), as the daemon did
 * (`block`), or not yet known. */
type Reader = { as: "me"; place: IssuePlace } | { as: "block" } | null;

function useReader(s: NativeIssueState, project: string | null | undefined): Reader {
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
      // Its project: the daemon's word to the owner, else the person's
      // own read.
      const place = placeIn(await readProjects(), project ?? (await issues.get(id)).project);
      if (live) setReader(place ? { as: "me", place } : { as: "block" });
    })().catch(() => live && setReader({ as: "block" }));
    return () => void (live = false);
  }, [signedIn, on, id, project]);
  return reader;
}

/** That Arugula control can't be reached, when it can't: what it said
 * last. Where an issue was read from is the source icon's tooltip. */
function Foot({ s }: { s: NativeIssueState }) {
  if (!s.pin.stale) return null;
  return <span data-issue-foot>Arugula control can't be reached now: this is what it said {s.read_ms ? <Time at={s.read_ms} long /> : "last"}.</span>;
}

/** The source icon's tooltip: who it's read as, and how it's followed. */
function sourceTitle(s: NativeIssueState, me: boolean): string {
  if (s.pin.stale && !me) return "Arugula · control can't be reached: this is what it said last";
  return me ? "Arugula · read as you, through Arugula control" : "Arugula · read through Arugula control by this machine, for its owner";
}

export function NativeIssueBlock({ client, block, s, refresh }: { client: Client; block: number; s: NativeIssueState; refresh: () => void }) {
  const owners = useOwnersRead(client, block, s);
  const reader = useReader(s, owners?.project ?? owners?.issue?.project);
  useSubscribe((fn) => issues.subscribe(fn));
  const id = s.pin.global_id;
  // What this page hears changed, the daemon reads again too, so its
  // text (and its agents' read) keeps up.
  const me = reader?.as === "me";
  useEffect(() => (me ? chat.onObject(id, refresh) : undefined), [id, me]);
  const there = platformHref(`/issues/${id}`);
  const issue = owners?.issue ?? issues.known(id);
  const attrs = { "data-issue-block": id, "data-issue-reader": reader?.as, "data-stale": s.pin.stale ? "" : undefined };
  // Nothing to draw a header from yet, or ever (not the reader's to
  // read): one line.
  if (reader === null || !issue) {
    return (
      <div class="issue-block" {...attrs}>
        {reader === null ? (
          <BlockLoading what="the issue from Arugula control" />
        ) : reader.as === "me" ? (
          <IssueView key={id} id={id} place={reader.place} refresh={s.read_ms ?? undefined} Missing={({ what }) => <FromBlock s={s} owners={owners} what={what} />} />
        ) : (
          <FromBlock s={s} owners={owners} />
        )}
      </div>
    );
  }
  const nameOf = (a: string) => (reader.as === "me" ? reader.place.members.find((m) => m.account === a)?.name : undefined) ?? a;
  const facts: ItemFact[] = [
    { kind: "labels", labels: issue.labels },
    { kind: "assignees", people: issue.assignees.map(nameOf) },
  ];
  return (
    <div class="issue-block" {...attrs}>
      <ItemBlock
        kind="issue"
        number={issue.number}
        title={issue.title}
        state={issue.state === "closed" ? "closed" : "open"}
        source={{ kind: "arugula", live: me ? (s.pin.stale ? "paused" : "following") : null, title: sourceTitle(s, me) }}
        onRefresh={refresh}
        refreshTitle={s.read_ms ? `Read ${new Date(s.read_ms).toLocaleTimeString()}` : undefined}
        href={there ?? undefined}
        facts={facts}
        tabs={[
          {
            id: "activity",
            label: "Activity",
            content:
              reader.as === "me" ? (
                <IssueView key={id} id={id} place={reader.place} refresh={s.read_ms ?? undefined} foot={<Foot s={s} />} Missing={({ what }) => <FromBlock s={s} owners={owners} what={what} />} />
              ) : (
                <FromBlock s={s} owners={owners} />
              ),
          },
        ]}
      />
    </div>
  );
}

/** What the daemon read for its owner, read only; or why there's nothing
 * (for anyone else: `#N`, and that it can't be read here), in one line. */
function FromBlock({ s, owners, what }: { s: NativeIssueState; owners: OwnersRead | null | undefined; what?: string }) {
  const session = getControlSession();
  const issue = owners?.issue;
  if (!issue) {
    const why =
      s.loading || owners === undefined
        ? "Loading the issue from Arugula control…"
        : owners === null
          ? `${s.number ? `#${s.number}: you` : "You"} can't read this here.`
          : (what ?? s.error ?? "Arugula control didn't say what it is.");
    return (
      <p class="dim" data-issue-missing data-issue-unreadable={owners === null ? "" : undefined}>
        {why}
      </p>
    );
  }
  const place: IssuePlace = {
    project: issue.project,
    members: [],
    writer: false,
    nameOf: (a) => (session && a === session.account ? session.name || session.login : undefined),
  };
  return (
    <IssueApiContext.Provider value={blockApi(issue)}>
      <IssueView key={`${s.pin.global_id}-block`} id={s.pin.global_id} place={place} refresh={s.read_ms} foot={<Foot s={s} />} />
    </IssueApiContext.Provider>
  );
}

/** Its plain text, as `capture --text` gives it: its number, as the
 * daemon gives it to everyone. */
export function nativeIssueText(s: NativeIssueState): string {
  const lines = [s.number ? `#${s.number} · ${s.pin.global_id}` : s.pin.global_id];
  if (s.pin.stale) lines.push(`(as Arugula control said it last: ${s.pin.stale})`);
  if (s.error) lines.push(s.error);
  return lines.join("\n");
}

/** What to call it in tabs and lists: the owner's read, else what this
 * page read as the person, else its number, else that it's a project's
 * issue (UI audit #177: not a bare "issue"). */
export function nativeIssueTitle(s: NativeIssueState, owners: OwnersRead | null | undefined = ownersReads.get(s.pin.global_id)): string {
  const i = owners?.issue ?? issues.known(s.pin.global_id);
  if (i) return `#${i.number} ${i.title}`;
  if (owners?.number && owners.title) return `#${owners.number} ${owners.title}`;
  return s.number ? `Issue #${s.number}` : "Project issue";
}
