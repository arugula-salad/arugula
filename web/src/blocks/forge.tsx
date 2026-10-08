// Forge blocks (M36): a pull request on the person's Forgejo, read by the
// daemon with their own `tea` login; on GitHub with their `gh` login (M38);
// or a GitLab merge request with their `glab` login (M39; anonymously and
// read-only when glab has none). M37: or an issue, with *Agent on this* (a
// worktree and branch for it, an agent there, the two in a tab, and the
// agent's PR joining them), the PRs that refer to it, and a new issue an
// agent drafted, waiting on the card for a person to send. What waits on
// you comes first (a
// review asked of you, red checks, changes asked for, a mention), then an
// agent's drafts, the checks, the reviews and the timeline. An agent's
// draft is answered on the card over the block (the daemon's ask): edit
// the text and Send, or Drop. Everything drawn comes from the daemon's
// state, so a shared session's viewers see the same, without the buttons.

import { render, type ComponentChildren } from "preact";
import { useLayoutEffect, useRef, useState } from "preact/hooks";
import type { Client } from "../client";
import type { Draft, Event as ForgeEvent, ForgeState, PaneId, Pr } from "../proto";
import { Markdown } from "../ui/markdown";
import { askText } from "../ui/menu";
import { registerBlock, type BlockView } from "./view";

/** "Open pull request…": a link, OWNER/REPO#N, or N in `dir`'s repository,
 * beside `split` or in a new tab of `session`. */
export async function openPr(client: Client, where: { split?: PaneId; session?: number; dir?: string | null }) {
  const pr = await askText("Open a pull request", "", "a link, OWNER/REPO#N, or N in this repository");
  if (!pr?.trim()) return;
  const place = where.split !== undefined ? { split: where.split, from_pane: where.split } : { session: where.session !== undefined ? String(where.session) : undefined };
  await client.openBlock({ type: "forge", config: { pr: pr.trim(), dir: where.dir ?? undefined }, local: true, ...place }, "couldn't open the pull request");
}

/** "Open issue…": as `openPr`, for an issue. */
export async function openIssue(client: Client, where: { split?: PaneId; session?: number; dir?: string | null }) {
  const issue = await askText("Open an issue", "", "a link, OWNER/REPO#N, or N in this repository");
  if (!issue?.trim()) return;
  const place = where.split !== undefined ? { split: where.split, from_pane: where.split } : { session: where.session !== undefined ? String(where.session) : undefined };
  await client.openBlock({ type: "forge", config: { issue: issue.trim(), dir: where.dir ?? undefined }, local: true, ...place }, "couldn't open the issue");
}

function ago(ms: number | null | undefined): string {
  if (!ms) return "";
  const s = Math.max(0, (Date.now() - ms) / 1000);
  if (s < 60) return "just now";
  if (s < 3600) return `${Math.round(s / 60)}m ago`;
  if (s < 86400) return `${Math.round(s / 3600)}h ago`;
  return `${Math.round(s / 86400)}d ago`;
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

function Timeline({ events, s }: { events: ForgeEvent[]; s: ForgeState }) {
  if (!events.length) return null;
  return (
    <section data-forge-timeline>
      <h4>Timeline</h4>
      {tidy(events).map((e) => (
        <div key={e.id} class="forge-row forge-event">
          <span class="dim">{ago(e.at)}</span> <b>{e.actor ?? "someone"}</b> {eventLine(e)}
          {e.body && <Body text={e.body} s={s} />}
        </div>
      ))}
    </section>
  );
}

/** The description in the main column, `side` beside it (under it when the
 * block is narrow). */
function Cols({ s, body, side }: { s: ForgeState; body: string; side: ComponentChildren[] }) {
  const sides = side.filter(Boolean);
  return (
    <div class={sides.length ? "forge-cols forge-two" : "forge-cols"}>
      <div class="forge-main">{body.trim() ? <Body text={body} s={s} description /> : <p class="dim">No description.</p>}</div>
      {sides.length > 0 && <div class="forge-side">{sides}</div>}
    </div>
  );
}

/** The actions, held at the foot of the scrolling body, and the polls line;
 * with no actions the line just ends the body. */
function Foot({ s, buttons }: { s: ForgeState; buttons: ComponentChildren }) {
  const polls = (
    <p class="dim ws-note forge-polls" data-forge-live-state title={s.live_why ?? undefined}>
      {s.polls} polls, {s.reads} full reads · read {ago(s.updated_ms)}
      {s.live && ` · ${s.live === "webhook" ? `live (${s.live_via === "github-app" ? "GitHub App" : "webhook"})` : "polling"}`}
    </p>
  );
  if (!buttons) return polls;
  return (
    <div class="forge-foot">
      <div class="ws-actions forge-actions">{buttons}</div>
      {polls}
    </div>
  );
}

const plural = (n: number, one: string) => `${n} ${one}${n === 1 ? "" : "s"}`;

function checksLine(pr: Pr): string {
  const r = pr.rollup;
  const n = pr.checks.length;
  if (!r) return n ? plural(n, "check") : "no checks";
  if (r === "success") return "checks passed";
  if (r === "running" || r === "queued") return "checks running";
  if (r === "failure") {
    const failed = pr.checks.filter((c) => c.state === "failure").length;
    return failed ? `${failed} of ${n} checks failed` : "checks failed";
  }
  return `checks ${words(r)}`;
}

const BLOCKED = "The forge won't merge it yet, though it has no conflicts: on GitHub, branch protection; on GitLab, approvals, unresolved discussions and the like.";

/** Where the PR stands, in a row under the meta line. */
function Chips({ pr }: { pr: Pr }) {
  const it = pr.item;
  const open = it.state === "open";
  return (
    <div class="forge-chips" data-forge-chips>
      <span class={`ws-tag forge-check ${pr.rollup ?? "none"}`}>{checksLine(pr)}</span>
      <span class="ws-tag forge-chip">{pr.reviews.length ? plural(pr.reviews.length, "review") : "no reviews"}</span>
      {/* null is the forge still working it out, and also every merged or closed PR */}
      {open && <span class={`ws-tag forge-chip ${it.mergeable === null ? "wait" : it.mergeable ? "good" : "bad"}`}>{it.mergeable === null ? "mergeable: checking" : it.mergeable ? "mergeable" : "conflicts"}</span>}
      {open && it.blocked && (
        <span class="ws-tag forge-chip bad" title={BLOCKED}>
          merge blocked
        </span>
      )}
      <span class="ws-tag forge-chip">{plural(it.comments, "comment")}</span>
    </div>
  );
}

const linkedState = (st: string) => <span class={`ws-tag forge-state ${st}`}>{st}</span>;

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

  const n = s.new;
  if (s.number === 0 && n) {
    // A new issue: an agent's waits on the card; a person's goes out.
    return (
      <div class="review ws forge" data-forge-block={id} data-forge-new={n.status}>
        <div class="review-bar">
          <span class="review-path">
            <b>{s.repo}</b> new issue: {n.title}
          </span>
          <span class={`ws-tag forge-state ${n.status}`}>{n.status === "waiting" ? (n.agent ? "draft" : "opening") : n.status}</span>
        </div>
        <div class="review-body ws-body">
          {n.status === "waiting" && n.agent && (
            <p class="forge-want" data-new-waiting>
              <b>{n.by.replace(/^mcp:/, "")}</b> drafted it <span class="dim">{ago(n.at_ms)}</span>: it's on the card, to edit and send, or drop
            </p>
          )}
          {n.status === "dropped" && <p class="dim" data-new-dropped>Dropped by {n.settled_by ?? "someone"}</p>}
          {n.error && <p class="ws-tag bad">{n.error}</p>}
          {s.error && <p class="ws-tag bad">{s.error}</p>}
          {n.body.trim() && <Body text={n.body} s={s} description />}
        </div>
      </div>
    );
  }
  if (s.loading && !s.updated_ms) {
    return (
      <div class="review ws forge">
        <div class="browser-card dim">Reading the issue…</div>
      </div>
    );
  }
  const issue = s.issue;
  const it = issue?.item;
  const l = s.link;
  const linked = [...(issue?.linked ?? [])];
  if (l?.pr && !linked.some((x) => x.number === l.pr)) linked.push({ number: l.pr, title: `from ${l.branch}`, state: "open", url: l.pr_url ?? "", head: l.branch });
  const waiting = s.drafts.filter((d) => d.status === "waiting");
  return (
    <div class="review ws forge" data-forge-block={id} data-forge-kind="issue">
      <div class="review-bar">
        <span class="review-path" title={it?.url ?? s.repo}>
          <b>
            {s.repo}#{s.number}
          </b>{" "}
          {it?.title}
        </span>
        {it && <span class={`ws-tag forge-state ${it.state}`} data-issue-state={it.state}>{it.state}</span>}
        {it && (
          <a class="dim forge-link" href={it.url} target="_blank" rel="noopener">
            open ↗
          </a>
        )}
        {mayWrite && (
          <button title="Read it again" disabled={busy !== null} onClick={refresh}>
            {busy === "refresh" ? "…" : "↻"}
          </button>
        )}
        <span class={`review-live ${s.watching ? "on" : ""}`}>{s.watching ? "live" : "paused"}</span>
      </div>
      {s.error && (
        <div class="browser-card" data-forge-error>
          <p>Can't read this issue</p>
          <p class="dim">{s.error}</p>
          {mayOwn && s.logins.length > 0 && (
            <div class="ws-actions" data-forge-logins>
              {s.logins.map((x) => (
                <button key={x.name} onClick={() => void call("login", { name: x.name }, "couldn't use that login")}>
                  Use login {x.name} ({x.url})
                </button>
              ))}
            </div>
          )}
          {mayWrite && <button onClick={refresh}>Try again</button>}
        </div>
      )}
      {issue && it && (
        <div class="review-body ws-body">
          <div class="dim forge-meta">
            opened by {it.author}
            {" · "}
            {it.assignees.length ? `assigned to ${it.assignees.join(", ")}` : "assigned to nobody"}
            {s.login && ` · as ${s.me ?? "?"} (tea login ${s.login})`}
            {it.labels.length > 0 && " · "}
            {it.labels.map((x) => (
              <span key={x} class="ws-tag">
                {x}
              </span>
            ))}
          </div>
          {(s.wants.length > 0 || waiting.length > 0) && (
            <section class="ws-gates" data-forge-wants>
              <h4>Waiting on you</h4>
              {waiting.length > 0 && (
                <p class="forge-want" data-forge-drafts-waiting>
                  {waiting.length === 1 ? "A draft waits" : `${waiting.length} drafts wait`} for you to send it: it's on the card
                </p>
              )}
              {s.wants.map((w) => (
                <div class="ws-gate" key={w.kind} data-want={w.kind}>
                  <div class="ws-gate-what">{w.why}</div>
                </div>
              ))}
            </section>
          )}
          {s.said && <p class="ws-said" data-forge-said>{s.said}</p>}
          <section data-issue-agent>
            <h4>Agent</h4>
            {l ? (
              <div class="forge-row" data-agent-link={l.block}>
                <b>{l.agent}</b> works on it in %{l.block}, on <code>{l.branch}</code> from <code>{l.base}</code> <span class="dim">{ago(l.at_ms)}</span>
                <div class="dim">{l.worktree}</div>
                <div>
                  {l.pr ? (
                    <span data-agent-pr={l.pr}>
                      Its pull request: <b>#{l.pr}</b>
                      {l.pr_block !== undefined && <span class="dim"> (beside it, %{l.pr_block})</span>}
                    </span>
                  ) : (
                    <span class="dim" data-agent-pr-waiting>Its pull request shows up here when it opens one from {l.branch}.</span>
                  )}
                </div>
              </div>
            ) : it.state === "open" && mayOwn ? (
              <div class="ws-actions">
                <button class="pri" data-agent-on disabled={busy !== null} onClick={() => void agentOn(false)}>
                  {busy === "agent" ? "Starting…" : "Agent on this"}
                </button>
                <button disabled={busy !== null} onClick={() => void agentOn(true)}>
                  With instructions…
                </button>
                <span class="dim ws-note">A worktree and branch i{s.number}-… from the default branch, Claude Code there with the issue, both in a tab.</span>
              </div>
            ) : (
              <p class="dim">No agent on it.</p>
            )}
          </section>
          <section data-issue-linked>
            <h4>Pull requests ({linked.length})</h4>
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
                {mayOwn && (
                  <button class="forge-open-pr" onClick={() => openPrBlock(x.number)}>
                    Open
                  </button>
                )}
              </div>
            ))}
          </section>
          {s.drafts.length > 0 && <Drafts s={s} />}
          <Cols s={s} body={it.body} side={[<Timeline key="t" events={issue.events} s={s} />]} />
          <Foot s={s} buttons={mayWrite && it.state === "open" ? <button onClick={() => void comment()}>Comment…</button> : null} />
        </div>
      )}
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
            <b>{d.by.replace(/^mcp:/, "")}</b> drafted {draftWhat(d)} <span class="dim">{ago(d.at_ms)}</span>
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
  const gitlab = s?.provider === "gitlab";
  const merge = async () => {
    const style = await askText("Merge: how?", "merge", gitlab ? "merge or squash" : s?.provider === "github" ? "merge, squash or rebase" : "merge, rebase, rebase-merge, squash or fast-forward-only");
    if (style?.trim()) await call("merge", { style: style.trim() }, "couldn't merge");
  };
  const refresh = () => void call("refresh", {}, "couldn't read the pull request");
  const viewer = role === "viewer";

  if (!s || (s.loading && !s.updated_ms)) {
    return (
      <div class="review ws forge">
        <div class="browser-card dim">Reading the pull request…</div>
      </div>
    );
  }
  const pr = s.pr;
  const it = pr?.item;
  const state = it ? (it.state === "open" && it.draft ? "draft" : it.state) : null;
  const waiting = s.drafts.filter((d) => d.status === "waiting");
  const asked = s.wants.some((w) => w.kind === "review");
  const canComment = mayWrite && it?.state === "open";
  return (
    <div class="review ws forge" data-forge-block={id}>
      <div class="review-bar">
        <span class="review-path" title={it?.url ?? s.repo}>
          <b>
            {s.repo}#{s.number}
          </b>{" "}
          {it?.title}
        </span>
        {state && <span class={`ws-tag forge-state ${state}`} data-pr-state={state}>{state}</span>}
        {it && (
          <a class="dim forge-link" href={it.url} target="_blank" rel="noopener">
            open ↗
          </a>
        )}
        {!viewer && (
          <button title="Read it again" disabled={busy !== null} onClick={refresh}>
            {busy === "refresh" ? "…" : "↻"}
          </button>
        )}
        <span class={`review-live ${s.watching ? "on" : ""}`}>{s.watching ? "live" : "paused"}</span>
      </div>
      {s.rate?.backoff && (
        <p class="dim ws-note" data-forge-rate>
          {s.rate.backoff}
        </p>
      )}
      {s.error && (
        <div class="browser-card" data-forge-error>
          <p>Can't read this pull request</p>
          <p class="dim">{s.error}</p>
          {mayOwn && s.logins.length > 0 && (
            <div class="ws-actions" data-forge-logins>
              {s.logins.map((l) => (
                <button key={l.name} onClick={() => void call("login", { name: l.name }, "couldn't use that login")}>
                  Use login {l.name} ({l.url})
                </button>
              ))}
            </div>
          )}
          {!viewer && <button onClick={refresh}>Try again</button>}
        </div>
      )}
      {s.read_only && (
        <p class="dim ws-note" data-forge-read-only>
          {s.read_only}
        </p>
      )}
      {pr && it && (
        <div class="review-body ws-body">
          <div class="forge-head">
            <div class="dim forge-meta">
              {it.author} wants to merge <code>{it.head.repo && it.head.repo !== s.repo ? `${it.head.repo}:` : ""}{it.head.branch}</code> into <code>{it.base.branch}</code>
              {" · "}
              {s.login && `as ${s.me ?? "?"} (${gitlab ? s.login.replace(/^glab:/, "glab, ") : s.provider === "github" ? `gh on ${s.login}` : `tea login ${s.login}`})`}
              {!s.login && s.read_only && "anonymously, read-only"}
              {it.labels.length > 0 && " · "}
              {it.labels.map((l) => (
                <span key={l} class="ws-tag">
                  {l}
                </span>
              ))}
            </div>
            <Chips pr={pr} />
          </div>
          {(s.wants.length > 0 || waiting.length > 0) && (
            <section class="ws-gates" data-forge-wants>
              <h4>Waiting on you</h4>
              {waiting.length > 0 && (
                <p class="forge-want" data-forge-drafts-waiting>
                  {waiting.length === 1 ? "A draft waits" : `${waiting.length} drafts wait`} for you to send it: it's on the card
                </p>
              )}
              {s.wants.map((w) => (
                <div class="ws-gate" key={w.kind} data-want={w.kind}>
                  <div class="ws-gate-what">{w.why}</div>
                  <div class="ws-actions">
                    {w.kind === "review" && mayWrite && (
                      <>
                        <button class="pri" data-approve-review disabled={busy !== null} onClick={() => void review("approve")}>
                          {busy === "review" ? "Approving…" : "Approve"}
                        </button>
                        <button disabled={busy !== null} onClick={() => void review("request_changes")}>
                          Request changes…
                        </button>
                      </>
                    )}
                    {w.kind === "failed" && s.rerun?.api && mayWrite && (
                      <button class="pri" data-rerun disabled={busy !== null} title={s.rerun.note} onClick={() => void call("rerun_checks", {}, "couldn't rerun the checks")}>
                        {busy === "rerun_checks" ? "Retrying…" : "Rerun"}
                      </button>
                    )}
                    {w.kind === "failed" && s.rerun?.url && (
                      <a class="button" href={s.rerun.url} target="_blank" rel="noopener" title={s.rerun.note} data-rerun-link>
                        Open the run ↗
                      </a>
                    )}
                  </div>
                </div>
              ))}
              {s.rerun && !s.rerun.api && <p class="dim ws-note">{s.rerun.note}</p>}
              {viewer && <p class="dim ws-note">You're watching this session: the owner or an editor answers.</p>}
            </section>
          )}
          {s.said && <p class="ws-said" data-forge-said>{s.said}</p>}
          {s.drafts.length > 0 && <Drafts s={s} />}
          <Cols
            s={s}
            body={it.body}
            side={[
              pr.checks.length > 0 && (
                <section key="c" data-forge-checks>
                  <h4>Checks</h4>
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
              (pr.reviews.length > 0 || it.requested.length > 0) && (
                <section key="r" data-forge-reviews>
                  <h4>Reviews ({pr.reviews.length})</h4>
                  {it.requested.length > 0 && (
                    <p class="dim">
                      Asked: {it.requested.map((r) => ("user" in r ? r.user : r.team)).join(", ")}
                      {asked && " (you)"}
                    </p>
                  )}
                  {pr.reviews.map((r) => (
                    <div key={r.id} class="forge-row" data-review={r.state}>
                      <b>{r.author ?? "a team"}</b> <span class={`ws-tag forge-review ${r.state}`}>{words(r.state)}</span>
                      {r.stale && <span class="dim"> (an older head)</span>}
                      {r.body && <Body text={r.body} s={s} />}
                    </div>
                  ))}
                </section>
              ),
              <Timeline key="t" events={pr.events} s={s} />,
            ]}
          />
          <Foot
            s={s}
            buttons={
              (canComment || mayOwn) && (
                <>
                  {canComment && <button onClick={() => void comment()}>Comment…</button>}
                  {canComment && !asked && <button onClick={() => void review("comment")}>Review…</button>}
                  {canComment && <button onClick={() => void merge()}>Merge…</button>}
                  {mayOwn && (
                    <button data-forge-diff disabled={busy !== null} onClick={() => void call("diff", {}, "couldn't show the changes")}>
                      Diff
                    </button>
                  )}
                  {mayOwn && (
                    <button data-forge-checkout disabled={busy !== null} onClick={() => void call("checkout", {}, "couldn't check it out")}>
                      Checkout
                    </button>
                  )}
                  {mayOwn && s.provider !== "github" && !s.read_only && (
                    <button data-forge-live disabled={busy !== null} title="A webhook on the repository, straight to this daemon" onClick={() => void call("live", { on: !s.hook }, "couldn't change live updates")}>
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

function plain(s: ForgeState | null): string {
  if (!s) return "";
  const it = s.pr?.item ?? s.issue?.item;
  const lines = [`${s.repo}#${s.number} ${it?.title ?? ""}`.trim()];
  if (s.error) lines.push(s.error);
  for (const w of s.wants) lines.push(`waiting on you: ${w.why}`);
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
  let state: ForgeState | null = null;
  const draw = () => render(<ForgeBlock client={client} id={id} s={state} />, host);
  draw();
  const off = client.subscribe(draw);
  return {
    host,
    setVisible: (v) => (host.style.display = v ? "" : "none"),
    update: (s) => {
      state = s as ForgeState;
      draw();
    },
    title: () => (state?.number === 0 ? `${state.repo} new issue` : `${state?.repo ?? (state?.kind === "issue" ? "issue" : "pull request")}#${state?.number ?? ""}`),
    text: () => plain(state),
    focus: () => host.querySelector<HTMLElement>("button")?.focus(),
    dispose: () => {
      off();
      render(null, host);
      host.remove();
    },
  };
});
