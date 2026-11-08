// An item block (#407, the epic is #406): one look for a PR and an issue.
// Six parts, top to bottom, and a part with nothing to say isn't drawn:
// the header (kind, #N, title, one state pill, the source and ↻ ↗), the
// next-step strip (at most one thing, coloured by what it asks), the facts
// (icon and count pairs, the words in their tooltips), the body (folded,
// with More), tabs, and the footer's buttons. Data in, callbacks out: it
// fetches nothing and knows no forge, so the shell and the platform page
// both draw it. The look is ./item-block.css.

import type { ComponentChildren } from "preact";
import { useLayoutEffect, useRef, useState } from "preact/hooks";
import { CheckIcon, CloseIcon, ExternalIcon, IconButton, IssueIcon, MergeIcon, PrIcon, ReloadIcon, SourceGlyph } from "./icons";
import { StateChip, type Tone } from "./state-chip";
import { Card } from "./card";
import "./item-block.css";

export type ItemKind = "pr" | "issue";
/** The state pill: Open green, Draft grey, Merged purple, Closed red. */
export type ItemState = "open" | "draft" | "merged" | "closed";

const STATE: Record<ItemState, { tone: Tone; word: string }> = {
  open: { tone: "ok", word: "Open" },
  draft: { tone: "muted", word: "Draft" },
  merged: { tone: "merged", word: "Merged" },
  closed: { tone: "danger", word: "Closed" },
};

/** Where the item lives: a forge's icon, or the Arugula mark for a project
 * issue. `live` is the dot on it: filled while following, hollow when
 * paused, none when nothing follows. `title` is the tooltip (who you act
 * as, how it's followed). */
export type ItemSource = { kind: "github" | "gitlab" | "forgejo" | "arugula"; live?: "following" | "paused" | null; title?: string };

export type Reviewer = {
  name: string;
  /** `asked`: a ring, nothing said yet. */
  state: "approved" | "changes" | "commented" | "asked";
  /** Said on an older commit: the badge dims. */
  stale?: boolean;
};

/** What the facts row can say, in the order it's given. */
export type ItemFact =
  | { kind: "branch"; from: string; to: string }
  | { kind: "checks"; ok?: number; pending?: number; failed?: number }
  | { kind: "reviewers"; people: Reviewer[] }
  | { kind: "mergeable"; state: "clean" | "checking" | "conflicts" | "blocked"; reason?: string }
  | { kind: "labels"; labels: string[] }
  | { kind: "assignees"; people: string[] }
  | { kind: "comments"; count: number }
  | { kind: "text"; text: string; title?: string };

/** The one thing the reader should do next. `warn` waits on you, `danger`
 * is broken, `ok` is ready, `info` is a note (the merge is held). */
export type NextStep = { tone: "warn" | "danger" | "ok" | "info"; text: string; actions?: ComponentChildren };

export type ItemTab = {
  id: string;
  label: string;
  count?: number;
  /** A failing tab is red, a pending one yellow. */
  tone?: "danger" | "warn";
  content: ComponentChildren;
};

const SOURCE_NAME: Record<ItemSource["kind"], string> = { github: "GitHub", gitlab: "GitLab", forgejo: "Forgejo", arugula: "Arugula" };

function SourceIcon({ source }: { source: ItemSource }) {
  return (
    <span class="item-source" data-item-source={source.kind} data-live={source.live ?? undefined} title={source.title ?? SOURCE_NAME[source.kind]}>
      <SourceGlyph kind={source.kind} />
      {source.live && <span class="item-live" data-item-live={source.live} />}
    </span>
  );
}

function Avatar({ role, name, badge, ring, dim, title }: { role: "reviewer" | "assignee"; name: string; badge?: "ok" | "danger" | "info"; ring?: boolean; dim?: boolean; title: string }) {
  return (
    <span class="item-avatar" data-item-avatar={role} data-name={name} data-ring={ring ? "asked" : undefined} data-dim={dim ? "" : undefined} title={title}>
      {name.charAt(0).toUpperCase()}
      {badge && <span class={`item-avatar-badge item-avatar-${badge}`}>{badge === "ok" ? <CheckIcon size="xs" /> : badge === "danger" ? <CloseIcon size="xs" /> : "•"}</span>}
    </span>
  );
}

const REVIEW_WORD: Record<Reviewer["state"], string> = { approved: "approved", changes: "asked for changes", commented: "commented", asked: "asked, hasn't answered" };

function Fact({ fact }: { fact: ItemFact }) {
  switch (fact.kind) {
    case "branch":
      return (
        <span class="item-fact item-branch" data-item-fact="branch" title={`${fact.from} into ${fact.to}`}>
          {fact.from} → {fact.to}
        </span>
      );
    case "checks": {
      const { ok = 0, pending = 0, failed = 0 } = fact;
      if (!ok && !pending && !failed) return null;
      return (
        <span class="item-fact" data-item-fact="checks">
          {ok > 0 && (
            <span class="item-ok" title={`${ok} passed`}>
              <CheckIcon size="xs" /> {ok}
            </span>
          )}
          {pending > 0 && (
            <span class="item-warn" title={`${pending} running`}>
              ● {pending}
            </span>
          )}
          {failed > 0 && (
            <span class="item-danger" title={`${failed} failed`}>
              <CloseIcon size="xs" /> {failed}
            </span>
          )}
        </span>
      );
    }
    case "reviewers":
      if (fact.people.length === 0) return null;
      return (
        <span class="item-fact item-avatars" data-item-fact="reviewers">
          {fact.people.map((p) => (
            <Avatar
              role="reviewer"
              key={p.name}
              name={p.name}
              ring={p.state === "asked"}
              dim={p.stale}
              badge={p.state === "approved" ? "ok" : p.state === "changes" ? "danger" : p.state === "commented" ? "info" : undefined}
              title={`${p.name} ${REVIEW_WORD[p.state]}${p.stale ? " (older commit)" : ""}`}
            />
          ))}
        </span>
      );
    case "mergeable": {
      const tone = fact.state === "clean" ? "ok" : fact.state === "checking" ? "warn" : "danger";
      const word = { clean: "No conflicts", checking: "Checking for conflicts", conflicts: "Conflicts with the base", blocked: "Blocked" }[fact.state];
      return (
        <span class={`item-fact item-${tone}`} data-item-fact="mergeable" data-mergeable={fact.state} title={fact.reason ? `${word}: ${fact.reason}` : word}>
          {fact.state === "blocked" ? "🔒" : <MergeIcon />}
          {fact.state === "checking" ? " …" : fact.state === "clean" ? " clean" : fact.state === "conflicts" ? " conflicts" : ""}
        </span>
      );
    }
    case "labels":
      if (fact.labels.length === 0) return null;
      return (
        <span class="item-fact" data-item-fact="labels">
          {fact.labels.map((l) => (
            <span key={l} class="item-label">
              {l}
            </span>
          ))}
        </span>
      );
    case "assignees":
      if (fact.people.length === 0) return null;
      return (
        <span class="item-fact item-avatars" data-item-fact="assignees">
          {fact.people.map((p) => (
            <Avatar key={p} role="assignee" name={p} title={`Assigned to ${p}`} />
          ))}
        </span>
      );
    case "comments":
      return (
        <span class="item-fact" data-item-fact="comments" title={`${fact.count} comment${fact.count === 1 ? "" : "s"}`}>
          💬 {fact.count}
        </span>
      );
    case "text":
      return (
        <span class="item-fact item-text" data-item-fact="text" title={fact.title}>
          {fact.text}
        </span>
      );
  }
}

const STRIP_ICON = { warn: "●", danger: <CloseIcon size="xs" />, ok: <CheckIcon size="xs" />, info: "○" } as const;

/** The body, folded to a few lines. More shows only when something is
 * folded away. */
function Body({ children }: { children: ComponentChildren }) {
  const el = useRef<HTMLDivElement>(null);
  const [open, setOpen] = useState(false);
  const [folded, setFolded] = useState(false);
  useLayoutEffect(() => {
    const e = el.current;
    if (e && !open) setFolded(e.scrollHeight > e.clientHeight + 1);
  });
  return (
    <div class="item-body" data-item-body data-open={open ? "" : undefined}>
      <div class="item-body-text" ref={el}>
        {children}
      </div>
      {(folded || open) && (
        <button type="button" class="link item-more" data-item-more onClick={() => setOpen(!open)}>
          {open ? "Less" : "More"}
        </button>
      )}
    </div>
  );
}

/**
 * - `source`, `onRefresh`, `refreshTitle`, `href`: the header's end. ↻ asks
 *   for a fresh read (`refreshTitle` says when the last one was: "Read 2m
 *   ago"); ↗ opens `href`. Each is left out when not given.
 * - `next`: the strip. None while nothing needs the reader, and never on a
 *   merged or closed item (the caller says nothing, and this draws nothing).
 * - `facts`: the row, in order. `body`: its children, empty or absent and
 *   the part isn't drawn. `tabs`: the first is shown until another is picked.
 * - `footer`: the action buttons.
 */
export function ItemBlock({
  kind,
  number,
  title,
  state,
  source,
  refreshTitle,
  onRefresh,
  href,
  next,
  facts,
  body,
  tabs,
  footer,
}: {
  kind: ItemKind;
  number: number | string;
  title: string;
  state: ItemState;
  source?: ItemSource;
  refreshTitle?: string;
  onRefresh?: () => void;
  href?: string;
  next?: NextStep | null;
  facts?: ItemFact[];
  body?: ComponentChildren;
  tabs?: ItemTab[];
  footer?: ComponentChildren;
}) {
  const [picked, setPicked] = useState<string | null>(null);
  const tab = tabs?.find((t) => t.id === picked) ?? tabs?.[0];
  const rows = (facts ?? []).map((f, i) => <Fact key={i} fact={f} />).filter(Boolean);
  const hasBody = body !== undefined && body !== null && body !== false && body !== "" && !(Array.isArray(body) && body.length === 0);
  const { tone, word } = STATE[state];
  return (
    <Card
      as="section"
      class="item-block"
      head={
          <header class="item-head" data-item-head>
            <span class="item-kind" title={kind === "pr" ? "Pull request" : "Issue"}>
              {kind === "pr" ? <PrIcon /> : <IssueIcon />}
            </span>
            <span class="item-number">#{number}</span>
            <span class="item-title">{title}</span>
            <StateChip tone={tone} data-item-pill>
              {word}
            </StateChip>
            {source && <SourceIcon source={source} />}
            {onRefresh && (
              <IconButton label="Refresh" title={refreshTitle ?? "Refresh"} class="item-icon-btn" data-item-refresh onClick={onRefresh}>
                <ReloadIcon size="sm" />
              </IconButton>
            )}
            {href && (
              <a class="item-icon-btn" data-item-open href={href} target="_blank" rel="noreferrer" title="Open in the browser" aria-label="Open in the browser">
                <ExternalIcon size="xs" />
              </a>
            )}
          </header>
      }
      data-item-block={kind}
      data-item-state={state}
    >
      {next && (
        <div class={`item-next item-next-${next.tone}`} data-item-next={next.tone}>
          <span class="item-next-icon" aria-hidden="true">
            {STRIP_ICON[next.tone]}
          </span>
          <span class="item-next-text">{next.text}</span>
          {next.actions && <span class="item-next-actions">{next.actions}</span>}
        </div>
      )}
      {rows.length > 0 && (
        <div class="item-facts" data-item-facts>
          {rows}
        </div>
      )}
      {hasBody && <Body>{body}</Body>}
      {tabs && tabs.length > 0 && tab && (
        <div class="item-tabs" data-item-tabs>
          <div class="item-tab-list" role="tablist">
            {tabs.map((t) => (
              <button
                key={t.id}
                type="button"
                role="tab"
                class="item-tab"
                data-item-tab={t.id}
                data-tone={t.tone}
                aria-selected={t === tab}
                onClick={() => setPicked(t.id)}
              >
                {t.label}
                {t.count !== undefined && <span class="item-tab-count">{t.count}</span>}
              </button>
            ))}
          </div>
          <div class="item-tab-panel" role="tabpanel" data-item-panel={tab.id}>
            {tab.content}
          </div>
        </div>
      )}
      {footer && (
        <footer class="item-foot" data-item-foot>
          {footer}
        </footer>
      )}
    </Card>
  );
}
