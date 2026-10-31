// Forge blocks (M36): a pull request on the person's Forgejo, read by the
// daemon with their own `tea` login; on GitHub with their `gh` login (M38);
// or a GitLab merge request with their `glab` login (M39; anonymously and
// read-only when glab has none). M37: or an issue, with *Agent on this* (a
// worktree and branch for it, an agent there, the two in a tab, and the
// agent's PR joining them), the PRs that refer to it, and a new issue an
// agent drafted, shown once on its block with Send, Edit, Ask for changes
// and Drop (no card over it). What waits on
// you comes first (a
// review asked of you, red checks, changes asked for, a mention), then an
// agent's drafts, the checks, the reviews and the timeline. An agent's
// draft is answered on the card over the block (the daemon's ask): edit
// the text and Send, or Drop. Everything drawn comes from the daemon's
// state, so a shared session's viewers see the same, without the buttons.
// M172: on a repo linked in Arugula control the block is a pin (`s.pin`):
// the daemon reads the platform's mirror, not GitHub, and the block looks
// the same, less what the mirror doesn't keep (the description, comments'
// text), and says where it's read.

import { render, type ComponentChildren } from "preact";
import { useLayoutEffect, useRef, useState } from "preact/hooks";
import type { Client } from "../client";
import type { CheckState, Draft, Event as ForgeEvent, ForgeState, PaneId, Pr } from "../../shared/proto";
import { Markdown } from "../../shared/ui/markdown";
import { askText, type PromptChoice } from "../../shared/ui/menu";
import { isNativeIssue, NativeIssueBlock, nativeIssueText, nativeIssueTitle, type NativeIssueState } from "./native-issue";
import { BlockError, BlockLoading } from "./block-states";
import { registerBlock, type BlockView } from "./view";
import { Time } from "../../shared/ui/time";
import { ago } from "../../shared/time";
import { ItemBlock, type ItemFact, type ItemSource, type ItemState, type ItemTab, type NextStep, type Reviewer as ItemReviewer } from "../../shared/ui/item-block";

/** "Open pull request…": a link, OWNER/REPO#N, or N in `dir`'s repository,
 * beside `split` or in a new tab of `session`. */
export async function openPr(client: Client, where: { split?: PaneId; session?: number; dir?: string | null }) {
  const pr = await askText("Open a pull request", "", "a link, OWNER/REPO#N, N in this repository, or a project's iss_…");
  if (!pr?.trim()) return;
  const place = where.split !== undefined ? { split: where.split, from_pane: where.split } : { session: where.session !== undefined ? String(where.session) : undefined };
  await client.openBlock({ type: "forge", config: { pr: pr.trim(), dir: where.dir ?? undefined }, local: true, ...place }, "couldn't open the pull request");
}

/** "Open issue…": as `openPr`, for an issue. */
export async function openIssue(client: Client, where: { split?: PaneId; session?: number; dir?: string | null }) {
  const issue = await askText("Open an issue", "", "a link, OWNER/REPO#N, N in this repository, or a project's iss_…");
  if (!issue?.trim()) return;
  const place = where.split !== undefined ? { split: where.split, from_pane: where.split } : { session: where.session !== undefined ? String(where.session) : undefined };
  await client.openBlock({ type: "forge", config: { issue: issue.trim(), dir: where.dir ?? undefined }, local: true, ...place }, "couldn't open the issue");
}

const words = (s: string) => s.replace(/_/g, " ");

function eventLine(e: ForgeEvent): string {
  switch (e.kind) {
    case "commented":
      return "commented";
    case "review_comment":
      return "commented on the code";
    case "review_requested":
      return `asked ${e.target && "user" in e.target ? e.target.user : e.target && "team" in e.target ? e.target.team : "someone"} for a review`;
    case "pushed":
      return e.commits ? `pushed ${e.commits} commit${e.commits === 1 ? "" : "s"}${e.force ? " (forced)" : ""}` : "pushed";
    case "assigned": {
      const who = e.target && "user" in e.target ? e.target.user : e.target && "team" in e.target ? e.target.team : "";
      return `${e.what === "unassigned" ? "unassigned" : "assigned"} ${who}`.trim();
    }
    case "referenced":
      return e.what?.startsWith("#") ? `referenced it in ${e.what}` : "referenced it";
    case "other":
      return e.what ?? "did something";
    default:
      return words(e.kind);
  }
}

function draftWhat(d: Draft): string {
  if (d.method === "comment") return "a comment";
  if (d.method === "merge") return `a merge (${d.style ?? "merge"})`;
  if (d.method === "rerun_checks") return "a rerun of the checks";
  if (d.method !== "review") return "a review";
  return d.event === "approve" ? "an approval" : d.event === "request_changes" ? "a review asking for changes" : "a review";
}

/** A draft's text: a comment's or a review's (a merge or a rerun has none). */
const draftBody = (d: Draft) => ("body" in d ? d.body : undefined);

/** Where a body's `#N` goes: the repository's issues, on the forge's own
 * origin. GitLab numbers issues and merge requests apart, so it stays text. */
function refsOf(s: ForgeState): string | null {
  const url = (s.pr ?? s.issue)?.item.url;
  if (!url || s.provider === "gitlab") return null;
  try {
    return `${new URL(url).origin}/${s.repo}/issues/`;
  } catch {
    return null;
  }
}

/** A description longer than this is cut, with *Show all*. */
const LONG_BODY = 20000;

/** A forge body as Markdown. The description is drawn whole; the rest clamp
 * to eight lines (in CSS), with a toggle once they're taller than that. */
function Body({ text, s, description }: { text: string; s: ForgeState; description?: boolean }) {
  const [open, setOpen] = useState(false);
  const [over, setOver] = useState(false);
  const clip = useRef<HTMLDivElement>(null);
  useLayoutEffect(() => {
    const el = clip.current;
    if (!el || description || open) return;
    const measure = () => setOver(el.scrollHeight > el.clientHeight + 1);
    measure();
    const watch = new ResizeObserver(measure);
    watch.observe(el);
    return () => watch.disconnect();
  }, [text, open, description]);
  const cut = description && text.length > LONG_BODY;
  const toggle = description ? cut : over || open;
  return (
    <div class={description ? "forge-body forge-desc" : "forge-body"}>
      <div ref={clip} class={description || open ? undefined : over ? "forge-clamp cut" : "forge-clamp"}>
        <Markdown text={cut && !open ? text.slice(0, LONG_BODY) : text} refs={refsOf(s)} />
      </div>
      {toggle && (
        <button class="forge-more" onClick={() => setOpen(!open)}>
          {open ? "Show less" : "Show all"}
        </button>
      )}
    </div>
  );
}

/** The newest first, with a run of pushes by one person as one entry. A
 * force-push stays its own: it rewrote what the others pushed. */
function tidy(events: ForgeEvent[]): ForgeEvent[] {
  const out: ForgeEvent[] = [];
  for (const e of [...events].reverse()) {
    const last = out[out.length - 1];
    if (last && e.kind === "pushed" && last.kind === "pushed" && !e.force && !last.force && e.actor === last.actor) {
      out[out.length - 1] = { ...last, commits: (last.commits ?? 1) + (e.commits ?? 1), at: Math.max(last.at, e.at) };
    } else out.push(e);
  }
  return out;
}

const plural = (n: number, one: string) => `${n} ${one}${n === 1 ? "" : "s"}`;

const BLOCKED = "The forge won't merge it yet, though it has no conflicts: on GitHub, branch protection; on GitLab, approvals, unresolved discussions and the like.";

const linkedState = (st: string) => <span class={`ws-tag forge-state ${st}`}>{st}</span>;

/** How each forge merges, as the daemon names them, with a word each. */
const MERGE_STYLES: Record<string, [string, string][]> = {
  github: [["merge", "Merge commit"], ["squash", "Squash"], ["rebase", "Rebase"]],
  gitlab: [["merge", "Merge commit"], ["squash", "Squash"]],
  forgejo: [["merge", "Merge commit"], ["squash", "Squash"], ["rebase", "Rebase"], ["rebase-merge", "Rebase, then a merge commit"], ["fast-forward-only", "Fast-forward only"]],
};

/** The ways to merge to offer: the forge's, less those the repository
 * doesn't allow when it says (UI audit #196). */
function mergeStyles(provider: string, allowed: string[] | undefined): [string, string][] {
  const all = MERGE_STYLES[provider] ?? MERGE_STYLES.forgejo;
  if (!allowed) return all;
  const ok = all.filter(([v]) => allowed.includes(v));
  return ok.length ? ok : all;
}

/** M37: an issue, the agent on it, and a new issue's draft. */
function IssueBlock({ client, id, s }: { client: Client; id: PaneId; s: ForgeState }) {
  const [busy, setBusy] = useState<string | null>(null);
  const session = client.sessionOfTab(client.tabOfPane(id)?.id ?? -1) ?? null;
  const role = client.role(session);
  const mayWrite = role !== "viewer";
  const mayOwn = role === "owner";
  const call = async (method: string, args: unknown, failure: string) => {
    setBusy(method);
    const ok = await client.api(`/api/blocks/${id}/call/${method}`, args, failure);
    setBusy(null);
    return ok;
  };
  const refresh = () => void call("refresh", {}, "couldn't read the issue");
  const comment = async () => {
    const body = await askText("Comment", "", "markdown");
    if (body?.trim()) await call("comment", { body }, "couldn't comment");
  };
  const agentOn = async (ask: boolean) => {
    let extra: string | null = "";
    if (ask) {
      extra = await askText("Agent on this", "", "anything to add to its prompt");
      if (extra === null) return;
    }
    await call("agent", { prompt_extra: extra || undefined }, "couldn't start an agent on it");
  };
  const openPrBlock = (n: number) =>
    void client.openBlock({ type: "forge", config: { repo: s.repo, number: n, kind: "pr", dir: s.dir ?? undefined, api: s.api ?? undefined, login: s.login ?? undefined }, split: id, from_pane: id, local: true }, "couldn't open the pull request");

  if (s.number === 0 && s.new) return <NewIssueBlock client={client} id={id} s={s} />;
  if (s.loading && !s.updated_ms) {
    return (
      <div class="review ws forge">
        <BlockLoading what="the issue" />
      </div>
    );
  }
  const issue = s.issue;
  const it = issue?.item;
  const l = s.link;
  const linked = [...(issue?.linked ?? [])];
  if (l?.pr && !linked.some((x) => x.number === l.pr)) linked.push({ number: l.pr, title: `from ${l.branch}`, state: "open", url: l.pr_url ?? "", head: l.branch });
  const open = it?.state === "open";
  const waiting = open ? s.drafts.filter((d) => d.status === "waiting") : [];
  // The agent on it, asking: its pane's reason is the question (#409).
  const asking = open && l ? client.state?.panes.find((p) => p.id === l.block)?.reason : null;
  const question = asking?.kind === "ask" || asking?.kind === "input" ? asking : null;
  const asks = open ? s.wants.filter((w) => w.kind !== "done") : [];
  // One strip, the first that applies: the agent's question, a draft of
  // yours, then what the forge says waits on you. None once it's closed.
  const next: NextStep | null = question
    ? {
        tone: "warn",
        text: `${l!.agent} asks: ${question.headline}`,
        actions: mayWrite && (
          <button class="primary small" data-agent-answer onClick={() => client.focusPane(l!.block)}>
            Answer
          </button>
        ),
      }
    : waiting.length > 0
      ? { tone: "warn", text: waiting.length === 1 ? "Your draft is waiting" : `${waiting.length} drafts of yours are waiting` }
      : asks.length > 0
        ? { tone: "warn", text: asks[0].why }
        : null;
  const failed = !!it && s.error;
  return (
    <div class="review ws forge" data-forge-block={id} data-forge-kind="issue">
      {!it && (
        <div class="review-bar">
          <span class="review-path" title={s.repo}>
            <span class="dim forge-ref">
              #{s.number} {s.repo}
            </span>
          </span>
        </div>
      )}
      <div class="review-body ws-body">
        {s.error && (
          <BlockError data-forge-error title="Can't read this issue" detail={s.error} retry={mayWrite ? refresh : undefined} inline={!!issue}>
            {mayOwn && s.logins.length > 0 && (
              <div class="ws-actions" data-forge-logins>
                {s.logins.map((x) => (
                  <button class="secondary small" key={x.name} onClick={() => void call("login", { name: x.name }, "couldn't use that login")}>
                    Use login {x.name} ({x.url})
                  </button>
                ))}
              </div>
            )}
          </BlockError>
        )}
        {s.said && <p class="ws-said" data-forge-said>{s.said}</p>}
        {issue && it && (
          <ItemBlock
            kind="issue"
            number={s.number}
            title={it.title}
            state={it.state === "open" ? "open" : "closed"}
            source={issueSource(s, !!failed)}
            refreshTitle={s.updated_ms ? `Read ${ago(s.updated_ms)}` : "Read it again"}
            onRefresh={mayWrite && busy === null ? refresh : undefined}
            href={it.url}
            next={next}
            facts={[
              ...(it.labels.length ? [{ kind: "labels" as const, labels: it.labels }] : []),
              ...(it.assignees.length ? [{ kind: "assignees" as const, people: it.assignees }] : []),
              { kind: "comments" as const, count: it.comments },
            ]}
            body={it.body.trim() ? <Body text={it.body} s={s} description /> : undefined}
            tabs={[
              {
                id: "activity",
                label: "Activity",
                count: issue.events.length + linked.length,
                content: (
                  <IssueActivity
                    s={s}
                    events={issue.events}
                    linked={linked}
                    drafts={open}
                    onOpen={mayOwn ? openPrBlock : undefined}
                  />
                ),
              },
              ...(l
                ? [
                    {
                      id: "agents",
                      label: "Agents",
                      count: 1,
                      tone: question ? ("warn" as const) : undefined,
                      content: (
                        <div class="forge-row" data-agent-link={l.block}>
                          <b>{l.agent}</b> in %{l.block}, on <code>{l.branch}</code> from <code>{l.base}</code>{" "}
                          <span class="dim">
                            <Time at={l.at_ms} />
                          </span>
                          <div class="dim">{l.worktree}</div>
                          <div>
                            {l.pr ? (
                              <span data-agent-pr={l.pr}>
                                PR <b>#{l.pr}</b>
                                {l.pr_block !== undefined && <span class="dim"> (beside it, %{l.pr_block})</span>}
                              </span>
                            ) : (
                              <span class="dim" data-agent-pr-waiting title={`Its pull request shows up here when it opens one from ${l.branch}.`}>
                                no pull request yet
                              </span>
                            )}
                          </div>
                        </div>
                      ),
                    },
                  ]
                : []),
            ]}
            footer={
              <>
                {mayWrite && open && <button class="secondary small" onClick={() => void comment()}>Comment</button>}
                {!l && open && mayOwn && (
                  <>
                    <button class="primary small" data-agent-on disabled={busy !== null} title={`A worktree and branch i${s.number}-… from the default branch, Claude Code there with the issue, both in a tab.`} onClick={() => void agentOn(false)}>
                      {busy === "agent" ? "Starting…" : "Agent on this"}
                    </button>
                    <button class="secondary small" disabled={busy !== null} onClick={() => void agentOn(true)}>
                      With instructions…
                    </button>
                  </>
                )}
              </>
            }
          />
        )}
      </div>
    </div>
  );
}

/** Where an issue is read, as the header's source icon: the forge's, a dot
 * while the block follows it, and in the tooltip who it acts as and how
 * it's read (what the old polls line said). */
function issueSource(s: ForgeState, failed: boolean): ItemSource {
  const how = s.pin
    ? `read through Arugula control${s.pin.stale ? ", can't reach it now: as it said last" : ""}`
    : s.live === "webhook"
      ? `live (${s.live_via === "github-app" ? "GitHub App" : "webhook"})`
      : s.live
        ? "polling"
        : "";
  const as = s.pin ? `as ${s.me ?? "?"}` : s.login ? `as ${s.me ?? "?"} (${s.login})` : "";
  return {
    kind: s.provider === "github" || s.provider === "gitlab" ? s.provider : "forgejo",
    live: failed ? null : s.watching ? "following" : "paused",
    title: [s.provider, as, how, s.live_why, s.pin?.stale].filter(Boolean).join(" · "),
  };
}

/** The Activity tab: the pull requests that refer to it, an agent's drafts
 * (while it's open), then what happened, newest first. */
function IssueActivity({ s, events, linked, drafts, onOpen }: { s: ForgeState; events: ForgeEvent[]; linked: NonNullable<ForgeState["issue"]>["linked"]; drafts: boolean; onOpen?: (n: number) => void }) {
  const settled = drafts ? s.drafts : [];
  if (!events.length && !linked.length && !settled.length) return <p class="dim">Nothing yet.</p>;
  return (
    <div data-forge-timeline>
      {linked.map((x) => (
        <div key={x.number} class="forge-row" data-linked={x.number}>
          {linkedState(x.state)} <b>#{x.number}</b> {x.title}
          {x.url && (
            <>
              {" "}
              <a class="dim" href={x.url} target="_blank" rel="noopener">
                ↗
              </a>
            </>
          )}
          {onOpen && (
            <button class="forge-open-pr" onClick={() => onOpen(x.number)}>
              Open
            </button>
          )}
        </div>
      ))}
      {settled.map((d) => (
        <div key={d.id} class={`forge-draft ${d.status}`} data-draft={d.id} data-draft-status={d.status}>
          <b>{d.by.replace(/^mcp:/, "")}</b> drafted {draftWhat(d)} <span class="dim"><Time at={d.at_ms} /></span>
          {d.status === "sent" && <span class="ws-tag">sent by {d.settled_by}</span>}
          {d.status === "dropped" && <span class="ws-tag">dropped by {d.settled_by}</span>}
          {d.error && <span class="ws-tag bad">{d.error}</span>}
          {draftBody(d) && <Body text={draftBody(d)!} s={s} />}
        </div>
      ))}
      {tidy(events).map((e) => (
        <div key={e.id} class="forge-row forge-event">
          <Time class="dim" at={e.at} /> <b>{e.actor ?? "someone"}</b> {eventLine(e)}
          {e.body && <Body text={e.body} s={s} />}
        </div>
      ))}
    </div>
  );
}

/** A new issue: an agent's draft, shown once, with every action in the
 * footer (the card the daemon raises is a line for the swarm and the phone).
 * Send it as it is or edited in place, ask the agent for changes, or drop it.
 * A person's own goes out at once. */
function NewIssueBlock({ client, id, s }: { client: Client; id: PaneId; s: ForgeState }) {
  const n = s.new!;
  const [mode, setMode] = useState<"view" | "edit" | "ask">("view");
  const [title, setTitle] = useState(n.title);
  const [body, setBody] = useState(n.body);
  const [preview, setPreview] = useState(false);
  const [note, setNote] = useState("");
  const [busy, setBusy] = useState<string | null>(null);
  const session = client.sessionOfTab(client.tabOfPane(id)?.id ?? -1) ?? null;
  const mayWrite = client.role(session) !== "viewer";
  const agent = n.by.replace(/^mcp:/, "");
  const waiting = n.status === "waiting" && n.agent;
  const revising = waiting && !!n.asked && n.asked.revised_ms === undefined;
  const call = async (method: string, args: unknown, failure: string) => {
    setBusy(method);
    const ok = await client.api(`/api/blocks/${id}/call/${method}`, args, failure);
    setBusy(null);
    return ok;
  };
  const edit = () => {
    setTitle(n.title);
    setBody(n.body);
    setPreview(false);
    setMode("edit");
  };
  const send = () => {
    const content = mode === "edit" ? { title: title.trim() || n.title, body } : {};
    void call("answer", { id: "new", content }, "couldn't send the issue").then((ok) => ok && setMode("view"));
  };
  const revise = () => {
    if (!note.trim()) return;
    void call("revise", { note: note.trim() }, `couldn't ask ${agent} for changes`).then((ok) => {
      if (!ok) return;
      setNote("");
      setMode("view");
    });
  };
  const drop = () => void call("drop", {}, "couldn't drop the draft").then((ok) => ok && setMode("view"));
  const as = s.me && s.login ? `as ${s.me} on ${s.login}` : "with your forge login";
  const tag = !waiting ? (n.status === "waiting" ? "opening" : n.status) : revising ? "revising" : "draft";

  let foot: ComponentChildren = null;
  if (waiting && mayWrite) {
    if (mode === "edit") {
      foot = (
        <div class="ws-actions forge-actions">
          <button class="primary small" data-new-send disabled={busy !== null || !title.trim()} onClick={send}>
            {busy === "answer" ? "Sending…" : "Send"}
          </button>
          <button class="secondary small" onClick={() => setMode("view")}>Cancel</button>
          <span class="forge-spacer" />
          <span class="dim forge-as">{as}</span>
        </div>
      );
    } else if (mode === "ask") {
      foot = (
        <div class="forge-compose">
          <label>
            <span class="dim">
              What should <b>{agent}</b> change?
            </span>
            <textarea
              data-new-note
              rows={3}
              value={note}
              placeholder="Say what's wrong or missing. It goes to the agent, not the issue."
              onInput={(e) => setNote((e.currentTarget as HTMLTextAreaElement).value)}
              onKeyDown={(e) => (e.metaKey || e.ctrlKey) && e.key === "Enter" && revise()}
            />
          </label>
          <div class="ws-actions forge-actions">
            <button class="primary small" data-new-revise disabled={busy !== null || !note.trim()} onClick={revise}>
              {busy === "revise" ? "Sending…" : `Send to ${agent}`}
            </button>
            <button class="secondary small" onClick={() => setMode("view")}>Cancel</button>
          </div>
        </div>
      );
    } else {
      foot = (
        <div class="ws-actions forge-actions">
          <button class="primary small" data-new-send disabled={busy !== null || revising} onClick={send}>
            {busy === "answer" ? "Sending…" : n.error ? "Try again" : "Send"}
          </button>
          <button class="secondary small" data-new-edit disabled={busy !== null || revising} onClick={edit}>
            Edit
          </button>
          {n.pane !== undefined && (
            <button class="secondary small" data-new-ask disabled={busy !== null || revising} onClick={() => setMode("ask")}>
              Ask for changes…
            </button>
          )}
          <span class="forge-spacer" />
          <button class="secondary danger small forge-drop" data-new-drop disabled={busy !== null} onClick={drop}>
            Drop
          </button>
        </div>
      );
    }
  }

  return (
    <div class="review ws forge" data-forge-block={id} data-forge-new={n.status} data-new-mode={mode} data-new-revising={revising || undefined}>
      <div class="review-bar">
        <span class="review-path">
          <b>{s.repo}</b> <span class="dim">new issue</span>
        </span>
        <span class={`ws-tag forge-state ${tag}`}>{tag}</span>
      </div>
      <div class="review-body ws-body">
        <div class="forge-new">
          {mode === "edit" ? (
            <>
              <label class="forge-field">
                <span>Title</span>
                <input data-new-title value={title} onInput={(e) => setTitle((e.currentTarget as HTMLInputElement).value)} />
              </label>
              <div class="forge-field">
                <div class="forge-tabs" role="tablist">
                  <button role="tab" aria-selected={!preview} onClick={() => setPreview(false)}>
                    Write
                  </button>
                  <button role="tab" aria-selected={preview} onClick={() => setPreview(true)}>
                    Preview
                  </button>
                </div>
                {preview ? (
                  <div class="forge-preview">
                    <Markdown text={body} refs={null} />
                  </div>
                ) : (
                  <textarea data-new-body aria-label="Text" value={body} onInput={(e) => setBody((e.currentTarget as HTMLTextAreaElement).value)} />
                )}
              </div>
            </>
          ) : (
            <>
              <p class="dim forge-new-who">
                {n.agent ? (
                  <>
                    <b>{agent}</b> drafted this new issue · <Time at={n.at_ms} />
                  </>
                ) : (
                  <>opening it as {n.by || "you"}…</>
                )}
              </p>
              {revising && n.asked && (
                <div class="forge-status asked" data-new-asked>
                  <b>{n.asked.by}</b> asked for changes <span class="dim">· <Time at={n.asked.at_ms} /></span>
                  <div class="forge-status-note">{n.asked.note}</div>
                  <div class="dim">{agent} is revising it. The new draft replaces this one here.</div>
                </div>
              )}
              {waiting && !revising && n.asked?.revised_ms !== undefined && (
                <div class="forge-status ok" data-new-revised>
                  {agent} revised it after {n.asked.by}'s note <span class="dim">· <Time at={n.asked.revised_ms} /></span>
                  <div class="dim forge-status-note">“{n.asked.note}”</div>
                </div>
              )}
              {n.status === "dropped" && (
                <div class="forge-status" data-new-dropped>
                  Dropped by {n.settled_by ?? "someone"}
                </div>
              )}
              {n.error && <div class="forge-status bad">Sending failed: {n.error}. The draft is unchanged, so you can try again.</div>}
              {s.error && <div class="forge-status bad">{s.error}</div>}
              <div class={n.status === "dropped" ? "forge-new-text dropped" : "forge-new-text"}>
                <h2 class="forge-new-title">{n.title}</h2>
                {n.body.trim() ? <Body text={n.body} s={s} description /> : <p class="dim">No text.</p>}
              </div>
            </>
          )}
        </div>
        {foot && (
          <div class="forge-foot forge-new-foot">
            {foot}
            {mode === "view" && <span class="dim forge-as">{as}</span>}
          </div>
        )}
      </div>
    </div>
  );
}

/** An agent's drafts on the block: waiting, then the newest settled. */
function Drafts({ s }: { s: ForgeState }) {
  const waiting = s.drafts.filter((d) => d.status === "waiting");
  const settled = s.drafts.filter((d) => d.status !== "waiting").slice(-5).reverse();
  return (
    <section data-forge-drafts>
      <h4>Drafts</h4>
      {[...waiting, ...settled].map((d) => (
        <div key={d.id} class={`forge-draft ${d.status}`} data-draft={d.id} data-draft-status={d.status}>
          <div>
            <b>{d.by.replace(/^mcp:/, "")}</b> drafted {draftWhat(d)} <span class="dim"><Time at={d.at_ms} /></span>
            {d.status === "sent" && (
              <span class="ws-tag">
                sent by {d.settled_by}
                {d.url && (
                  <>
                    {" "}
                    <a href={d.url} target="_blank" rel="noopener">
                      ↗
                    </a>
                  </>
                )}
              </span>
            )}
            {d.status === "dropped" && <span class="ws-tag">dropped by {d.settled_by}</span>}
            {d.error && <span class="ws-tag bad">{d.error}</span>}
          </div>
          {draftBody(d) && <Body text={draftBody(d)!} s={s} />}
        </div>
      ))}
    </section>
  );
}

function ForgeBlock({ client, id, s }: { client: Client; id: PaneId; s: ForgeState | null }) {
  if (s?.kind === "issue") return <IssueBlock client={client} id={id} s={s} />;
  return <PrBlock client={client} id={id} s={s} />;
}

function PrBlock({ client, id, s }: { client: Client; id: PaneId; s: ForgeState | null }) {
  const [busy, setBusy] = useState<string | null>(null);
  const session = client.sessionOfTab(client.tabOfPane(id)?.id ?? -1) ?? null;
  const role = client.role(session);
  // Writes are the owner's and editors' (they go out with the owner's
  // login, naming who sent them); the clone and the login are the owner's.
  const mayWrite = role !== "viewer" && !s?.read_only;
  const mayOwn = role === "owner";
  const call = async (method: string, args: unknown, failure: string) => {
    setBusy(method);
    const ok = await client.api(`/api/blocks/${id}/call/${method}`, args, failure);
    setBusy(null);
    return ok;
  };
  const comment = async () => {
    const body = await askText("Comment", "", "markdown");
    if (body?.trim()) await call("comment", { body }, "couldn't comment");
  };
  const review = async (event: "approve" | "request_changes" | "comment") => {
    let body: string | null = "";
    if (event !== "approve") {
      body = await askText(event === "request_changes" ? "Request changes" : "Review", "", "what to change");
      if (!body?.trim()) return;
    }
    await call("review", { event, body: body || undefined }, "couldn't review");
  };
  // UI audit #178: the forge's ways to merge, to pick from, rather than a
  // word to type; #196: only the ones the repository allows, when the
  // forge says (a squash-only repo offers Squash alone).
  const merge = async () => {
    const styles = mergeStyles(s?.provider ?? "", s?.merge_styles);
    const choices: PromptChoice[] = styles.map(([value, label]) => ({ value, label }));
    const style = await askText(`Merge #${s?.number ?? ""}`, "", "pick how, or type a method", Promise.resolve(choices));
    if (style?.trim()) await call("merge", { style: style.trim() }, "couldn't merge");
  };
  const refresh = () => void call("refresh", {}, "couldn't read the pull request");
  const viewer = role === "viewer";

  if (!s || (s.loading && !s.updated_ms)) {
    return (
      <div class="review ws forge">
        <BlockLoading what="the pull request" />
      </div>
    );
  }
  const pr = s.pr;
  const it = pr?.item;
  const open = it?.state === "open";
  const state: ItemState | null = it ? (open && it.draft ? "draft" : it.state) : null;
  // Never a want after the end: a merged or closed PR has no strip, and the
  // drafts for it are dropped.
  const waiting = open ? s.drafts.filter((d) => d.status === "waiting") : [];
  const asked = s.wants.some((w) => w.kind === "review");
  const canComment = mayWrite && open;
  const send = (d: Draft) => void client.api(`/api/blocks/${id}/call/answer`, { id: d.id, content: {} }, "couldn't send the draft");
  const next = pr && it && open ? nextStep(s, pr, mayWrite, busy, { review, call, merge, send, waiting }) : null;
  const source: ItemSource = {
    kind: s.provider,
    live: s.error ? null : s.watching ? "following" : "paused",
    title: sourceTitle(s),
  };
  return (
    <div class="review ws forge" data-forge-block={id}>
      {s.rate?.backoff && (
        <p class="dim ws-note" data-forge-rate>
          {s.rate.backoff}
        </p>
      )}
      {s.error && (
        <BlockError data-forge-error title="Can't read this pull request" detail={s.error} retry={viewer ? undefined : refresh} inline={!!pr}>
          {mayOwn && s.logins.length > 0 && (
            <div class="ws-actions" data-forge-logins>
              {s.logins.map((l) => (
                <button class="secondary small" key={l.name} onClick={() => void call("login", { name: l.name }, "couldn't use that login")}>
                  Use login {l.name} ({l.url})
                </button>
              ))}
            </div>
          )}
        </BlockError>
      )}
      {s.read_only && (
        <p class="dim ws-note" data-forge-read-only>
          {s.read_only}
        </p>
      )}
      {s.said && <p class="ws-said" data-forge-said>{s.said}</p>}
      {pr && it && state && (
        <div class="review-body ws-body">
          <ItemBlock
            kind="pr"
            number={s.number}
            title={it.title}
            state={state}
            source={source}
            refreshTitle={`Read ${ago(s.updated_ms)}`}
            onRefresh={viewer || busy !== null ? undefined : refresh}
            href={it.url}
            next={next}
            facts={prFacts(s, pr)}
            body={it.body.trim() ? <Body text={it.body} s={s} description /> : undefined}
            tabs={prTabs(s, pr, open, waiting)}
            footer={
              (canComment || mayOwn) && (
                <>
                  {canComment && <button class="secondary small" onClick={() => void comment()}>Comment</button>}
                  {canComment && !asked && <button class="secondary small" onClick={() => void review("comment")}>Review</button>}
                  {canComment && <button class="secondary small" onClick={() => void merge()}>Merge ▾</button>}
                  {mayOwn && (
                    <button class="secondary small" data-forge-diff disabled={busy !== null} onClick={() => void call("diff", {}, "couldn't show the changes")}>
                      Diff
                    </button>
                  )}
                  {mayOwn && (
                    <button class="secondary small" data-forge-checkout disabled={busy !== null} onClick={() => void call("checkout", {}, "couldn't check it out")}>
                      Checkout
                    </button>
                  )}
                  {mayOwn && s.provider !== "github" && !s.read_only && (
                    <button class="secondary small" data-forge-live disabled={busy !== null} title="A webhook on the repository, straight to this daemon" onClick={() => void call("live", { on: !s.hook }, "couldn't change live updates")}>
                      {s.hook ? "Stop live updates" : "Live updates"}
                    </button>
                  )}
                </>
              )
            }
          />
        </div>
      )}
    </div>
  );
}

/** The tooltip on the source icon: where it is, who you act as, how it's
 * followed and read (it replaces the meta line's "as …" and the polls line). */
function sourceTitle(s: ForgeState): string {
  const gitlab = s.provider === "gitlab";
  const as = s.pin
    ? `as ${s.me ?? "?"} on GitHub`
    : s.login
      ? `as ${s.me ?? "?"} (${gitlab ? s.login.replace(/^glab:/, "glab, ") : s.provider === "github" ? `gh on ${s.login}` : `tea login ${s.login}`})`
      : s.read_only
        ? "anonymously, read-only"
        : "";
  const live = s.live ? (s.live === "webhook" ? `live (${s.live_via === "github-app" ? "GitHub App" : "webhook"})` : "polling") : "";
  const read = s.pin
    ? `read through Arugula control${s.pin.stale ? ", can't reach it now: as it said last" : ""}`
    : `${plural(s.polls, "poll")}, ${plural(s.reads, "full read")}`;
  return [`${s.repo}`, as, read, live, s.pin?.stale ?? s.live_why ?? ""].filter(Boolean).join("\n");
}

const FAILED: CheckState[] = ["failure"];
const RUNNING: CheckState[] = ["queued", "running"];

/** The one thing to do next, the first that applies: conflicts, a check
 * failed, your review asked for, a draft of yours waiting, ready to merge.
 * None for anything but an open PR. */
function nextStep(
  s: ForgeState,
  pr: Pr,
  mayWrite: boolean,
  busy: string | null,
  on: { review: (e: "approve" | "request_changes") => Promise<void>; call: (m: string, a: unknown, f: string) => Promise<unknown>; merge: () => Promise<void>; send: (d: Draft) => void; waiting: Draft[] },
): NextStep | null {
  const it = pr.item;
  if (it.mergeable === false) return { tone: "danger", text: `Conflicts with ${it.base.branch}` };
  if (s.wants.some((w) => w.kind === "failed")) {
    const failed = pr.checks.filter((c) => FAILED.includes(c.state));
    const text = failed.length === 0 ? "Checks failed" : failed.length === 1 ? `${failed[0].name} failed` : `${failed[0].name} and ${failed.length - 1} more failed`;
    return {
      tone: "danger",
      text,
      actions: (
        <>
          {s.rerun?.api && mayWrite && (
            <button class="primary small" data-rerun disabled={busy !== null} title={s.rerun.note} onClick={() => void on.call("rerun_checks", {}, "couldn't rerun the checks")}>
              {busy === "rerun_checks" ? "Rerunning…" : "Rerun"}
            </button>
          )}
          {s.rerun?.url && (
            <a class="secondary small" href={s.rerun.url} target="_blank" rel="noopener" title={s.rerun.note} data-rerun-link>
              Open run ↗
            </a>
          )}
        </>
      ),
    };
  }
  if (s.wants.some((w) => w.kind === "review")) {
    return {
      tone: "warn",
      text: "Your review is asked for",
      actions: mayWrite && (
        <>
          <button class="primary small" data-approve-review disabled={busy !== null} onClick={() => void on.review("approve")}>
            {busy === "review" ? "Approving…" : "Approve"}
          </button>
          <button class="secondary small" disabled={busy !== null} onClick={() => void on.review("request_changes")}>
            Request changes
          </button>
        </>
      ),
    };
  }
  const [draft] = on.waiting;
  if (draft) {
    const more = on.waiting.length - 1;
    return {
      tone: "warn",
      text: `Your ${draftKind(draft)} draft${more > 0 ? ` and ${more} more` : ""}`,
      actions: mayWrite && (
        <button class="primary small" data-send-draft disabled={busy !== null} onClick={() => on.send(draft)}>
          Send
        </button>
      ),
    };
  }
  const settled = !it.draft && it.mergeable === true && !it.blocked && !pr.checks.some((c) => [...FAILED, ...RUNNING].includes(c.state));
  if (settled && mayWrite) {
    return {
      tone: "ok",
      text: "Ready to merge",
      actions: (
        <button class="primary small" data-merge disabled={busy !== null} onClick={() => void on.merge()}>
          Merge ▾
        </button>
      ),
    };
  }
  return null;
}

const draftKind = (d: Draft) => (d.method === "rerun_checks" ? "rerun" : d.method === "live" ? "live updates" : d.method);

/** The facts, in the Doc's order: the branch, checks, reviews, mergeable,
 * labels, comments. A merged or closed PR has no mergeable (the forge no
 * longer says). */
function prFacts(s: ForgeState, pr: Pr): ItemFact[] {
  const it = pr.item;
  const count = (states: CheckState[]) => pr.checks.filter((c) => states.includes(c.state)).length;
  const people = new Map<string, ItemReviewer>();
  for (const r of pr.reviews) {
    const state = r.state === "approved" ? "approved" : r.state === "changes_requested" ? "changes" : r.state === "commented" ? "commented" : null;
    if (state && r.author) people.set(r.author, { name: r.author, state, stale: r.stale });
  }
  for (const r of it.requested) {
    const name = "user" in r ? r.user : r.team;
    if (!people.has(name)) people.set(name, { name, state: "asked" });
  }
  const facts: ItemFact[] = [
    { kind: "branch", from: `${it.head.repo && it.head.repo !== s.repo ? `${it.head.repo}:` : ""}${it.head.branch}`, to: it.base.branch },
    { kind: "checks", ok: count(["success"]), pending: count(RUNNING), failed: count(FAILED) },
    { kind: "reviewers", people: [...people.values()] },
  ];
  if (it.state === "open") {
    // null is the forge still working it out
    facts.push(
      it.blocked
        ? { kind: "mergeable", state: "blocked", reason: BLOCKED }
        : { kind: "mergeable", state: it.mergeable === null ? "checking" : it.mergeable ? "clean" : "conflicts" },
    );
  } else if (it.state === "merged") {
    facts.push({ kind: "text", text: it.merged_by ? `merged by ${it.merged_by}` : "merged", title: it.merged_at ? new Date(it.merged_at).toLocaleString() : undefined });
  }
  if (it.labels.length) facts.push({ kind: "labels", labels: it.labels });
  facts.push({ kind: "comments", count: it.comments });
  return facts;
}

/** Checks · Reviews · Activity: the side columns, as tabs. An empty one isn't
 * drawn; a failing one is red, a running one yellow. */
function prTabs(s: ForgeState, pr: Pr, open: boolean, waiting: Draft[]): ItemTab[] {
  const tabs: ItemTab[] = [];
  const failing = pr.checks.some((c) => FAILED.includes(c.state));
  const running = pr.checks.some((c) => RUNNING.includes(c.state));
  if (pr.checks.length > 0) {
    tabs.push({
      id: "checks",
      label: "Checks",
      count: pr.checks.length,
      tone: failing ? "danger" : running ? "warn" : undefined,
      content: (
        <section data-forge-checks>
          {pr.checks.map((c) => (
            <div key={c.name} class="forge-row" data-check={c.state}>
              <span class={`forge-dot ${c.state}`} title={words(c.state)} />
              {c.url ? (
                <a href={c.url} target="_blank" rel="noopener">
                  {c.name}
                </a>
              ) : (
                c.name
              )}
              {c.description && <span class="dim"> · {c.description}</span>}
            </div>
          ))}
        </section>
      ),
    });
  }
  if (pr.reviews.length > 0) {
    tabs.push({
      id: "reviews",
      label: "Reviews",
      count: pr.reviews.length,
      tone: pr.reviews.some((r) => r.state === "changes_requested" && !r.stale) ? "danger" : undefined,
      content: (
        <section data-forge-reviews>
          {pr.reviews.map((r) => (
            <div key={r.id} class="forge-row" data-review={r.state} title={r.stale ? "older commit" : undefined} style={r.stale ? { opacity: 0.6 } : undefined}>
              <b>{r.author ?? "a team"}</b> <span class={`ws-tag forge-review ${r.state}`}>{words(r.state)}</span>
              {r.body && <Body text={r.body} s={s} />}
            </div>
          ))}
        </section>
      ),
    });
  }
  const events = tidy(pr.events);
  // The drafts of a merged or closed PR are dropped, settled or not.
  const drafts = open && s.drafts.length > 0;
  if (events.length > 0 || drafts) {
    tabs.push({
      id: "activity",
      label: "Activity",
      count: events.length,
      tone: waiting.length > 0 ? "warn" : undefined,
      content: (
        <>
          {drafts && <Drafts s={s} />}
          <section data-forge-timeline>
            {events.map((e) => (
              <div key={e.id} class="forge-row forge-event">
                <Time class="dim" at={e.at} /> <b>{e.actor ?? "someone"}</b> {eventLine(e)}
                {e.body && <Body text={e.body} s={s} />}
              </div>
            ))}
          </section>
        </>
      ),
    });
  }
  // A merged or closed PR has nothing left to act on: its Activity first, not a red Checks.
  return open ? tabs : [...tabs.filter((x) => x.id === "activity"), ...tabs.filter((x) => x.id !== "activity")];
}

function plain(s: ForgeState | null): string {
  if (!s) return "";
  const it = s.pr?.item ?? s.issue?.item;
  const lines = [`${s.repo}#${s.number} ${it?.title ?? ""}`.trim()];
  if (s.error) lines.push(s.error);
  for (const w of s.wants) lines.push(w.kind === "done" ? `done: ${w.why}` : `waiting on you: ${w.why}`);
  for (const d of s.drafts) lines.push(`draft ${d.id} ${d.status}: ${draftBody(d) ?? d.method}`);
  for (const c of s.pr?.checks ?? []) lines.push(`${c.state} ${c.name}`);
  for (const r of s.pr?.reviews ?? []) lines.push(`${r.author ?? "?"} ${r.state}`);
  if (s.link) lines.push(`agent %${s.link.block} on ${s.link.branch}${s.link.pr ? `, PR #${s.link.pr}` : ""}`);
  if (s.number === 0 && s.new) lines.push(`new issue (${s.new.status}): ${s.new.title}`);
  return lines.join("\n");
}

registerBlock("forge", (client, id): BlockView => {
  const host = document.createElement("div");
  host.className = "block block-forge";
  // M203: a project's own issue is a forge block too, drawn by the
  // platform page's issue view.
  let state: ForgeState | NativeIssueState | null = null;
  const draw = () => render(isNativeIssue(state) ? <NativeIssueBlock client={client} block={id} s={state} refresh={() => void client.api(`/api/blocks/${id}/call/refresh`, {})} /> : <ForgeBlock client={client} id={id} s={state} />, host);
  draw();
  const off = client.subscribe(draw);
  return {
    host,
    setVisible: (v) => (host.style.display = v ? "" : "none"),
    update: (s) => {
      state = s as ForgeState | NativeIssueState;
      draw();
    },
    title: () =>
      isNativeIssue(state)
        ? nativeIssueTitle(state)
        : state?.number === 0
          ? `${state.repo} new issue`
          : state?.number
            ? `#${state.number} ${(state.pr?.item ?? state.issue?.item)?.title ?? state.repo}`
            : `${state?.repo ?? (state?.kind === "issue" ? "issue" : "pull request")}#${state?.number ?? ""}`,
    text: () => (isNativeIssue(state) ? nativeIssueText(state) : plain(state)),
    focus: () => host.querySelector<HTMLElement>("button")?.focus(),
    dispose: () => {
      off();
      render(null, host);
      host.remove();
    },
  };
});
