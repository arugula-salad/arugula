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
import { StateChip, type Tone } from "./state-chip";
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
 * is broken, `ok` is ready. */
export type NextStep = { tone: "warn" | "danger" | "ok"; text: string; actions?: ComponentChildren };

export type ItemTab = {
  id: string;
  label: string;
  count?: number;
  /** A failing tab is red, a pending one yellow. */
  tone?: "danger" | "warn";
  content: ComponentChildren;
};

const PrIcon = () => (
  <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
    <circle cx="6" cy="6" r="2.5" />
    <circle cx="6" cy="18" r="2.5" />
    <circle cx="18" cy="18" r="2.5" />
    <path d="M6 8.5v7M18 15.5V10a3 3 0 0 0-3-3h-3m2-2.5L12 7l2 2.5" />
  </svg>
);
const IssueIcon = () => (
  <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" aria-hidden="true">
    <circle cx="12" cy="12" r="9" />
    <circle cx="12" cy="12" r="1.5" fill="currentColor" />
  </svg>
);
const MergeIcon = () => (
  <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
    <circle cx="6" cy="5" r="2.5" />
    <circle cx="6" cy="19" r="2.5" />
    <circle cx="18" cy="14" r="2.5" />
    <path d="M6 7.5v9M18 11.5C18 8 12 9 7.5 6.5" />
  </svg>
);
const SOURCE_PATH: Record<ItemSource["kind"], string> = {
  github: "M9 19c-5 1.5-5-2.5-7-3m14 6v-3.9a3.4 3.4 0 0 0-.9-2.6c3.100-.4 6.400-1.500 6.400-7A5.400 5.400 0 0 0 20 4.800 5.100 5.100 0 0 0 19.900 1S18.700.7 16 2.500a13.400 13.400 0 0 0-7 0C6.300.7 5.100 1 5.100 1A5.100 5.100 0 0 0 5 4.800a5.400 5.400 0 0 0-1.500 3.800c0 5.400 3.300 6.600 6.400 7A3.400 3.400 0 0 0 9 18.100V22",
  gitlab: "M12 21 3 14l2.500-8 2.500 6h8l2.500-6L21 14z",
  forgejo: "M7 4v6a3 3 0 0 0 3 3h4a3 3 0 0 0 3-3V4M12 13v7",
  arugula: "M12 3c5 0 9 4 9 9-4 0-9-1-9-9zM12 3C7 3 3 7 3 12c4 0 9-1 9-9zM12 21v-9",
};
const SOURCE_NAME: Record<ItemSource["kind"], string> = { github: "GitHub", gitlab: "GitLab", forgejo: "Forgejo", arugula: "Arugula" };

function SourceIcon({ source }: { source: ItemSource }) {
  return (
    <span class="item-source" data-item-source={source.kind} data-live={source.live ?? undefined} title={source.title ?? SOURCE_NAME[source.kind]}>
      <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
        <path d={SOURCE_PATH[source.kind]} />
      </svg>
      {source.live && <span class="item-live" data-item-live={source.live} />}
    </span>
  );
}

function Avatar({ role, name, badge, ring, dim, title }: { role: "reviewer" | "assignee"; name: string; badge?: "ok" | "danger" | "info"; ring?: boolean; dim?: boolean; title: string }) {
  return (
    <span class="item-avatar" data-item-avatar={role} data-name={name} data-ring={ring ? "asked" : undefined} data-dim={dim ? "" : undefined} title={title}>
      {name.charAt(0).toUpperCase()}
      {badge && <span class={`item-avatar-badge item-avatar-${badge}`}>{badge === "ok" ? "✓" : badge === "danger" ? "✕" : "•"}</span>}
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
              ✓ {ok}
            </span>
          )}
          {pending > 0 && (
            <span class="item-warn" title={`${pending} running`}>
              ● {pending}
            </span>
          )}
          {failed > 0 && (
            <span class="item-danger" title={`${failed} failed`}>
              ✕ {failed}
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

const STRIP_ICON = { warn: "●", danger: "✕", ok: "✓" } as const;

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
    <section class="item-block" data-item-block={kind} data-item-state={state}>
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
          <button type="button" class="item-icon-btn" data-item-refresh title={refreshTitle ?? "Refresh"} aria-label="Refresh" onClick={onRefresh}>
            ↻
          </button>
        )}
        {href && (
          <a class="item-icon-btn" data-item-open href={href} target="_blank" rel="noreferrer" title="Open in the browser" aria-label="Open in the browser">
            ↗
          </a>
        )}
      </header>
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
    </section>
  );
}
