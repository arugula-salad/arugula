// PrMark (kit, #1566): a pull request's state as GitHub draws it, its
// icon in its colour (open green, a draft grey, queued amber, merged
// purple, closed red), and its checks beside it (running an amber dot,
// passed a green tick, failed a red cross). Wherever a PR's state shows,
// this is its mark, so it reads the same as on GitHub. The words stay
// beside it; the mark's label says it to a screen reader.

import type { JSX } from "preact";
import { CheckIcon, CloseIcon, DotIcon, MergeIcon, MergeQueueIcon, PrClosedIcon, PrDraftIcon, PrIcon, type IconSize } from "../ui/icons";
import "./kit.css";

export type PrState = "open" | "draft" | "queued" | "merged" | "closed";
export type Checks = "running" | "passed" | "failed";

const ICON: Record<PrState, (p: { size?: IconSize }) => JSX.Element> = { open: PrIcon, draft: PrDraftIcon, queued: MergeQueueIcon, merged: MergeIcon, closed: PrClosedIcon };
const CHECK: Record<Checks, (p: { size?: IconSize }) => JSX.Element> = { running: DotIcon, passed: CheckIcon, failed: CloseIcon };
const SAID: Record<PrState, string> = { open: "open", draft: "draft", queued: "in the merge queue", merged: "merged", closed: "closed" };

/** A PR's mark from what the wire says of it: a board's live item, a
 * forge item. A PR that left the queue unmerged failed its checks there.
 * The board's says no more than running or failed: an `open` PR's checks
 * passed or it has none, so it gets no checks mark. */
export function prMarkOf(l: { state: string; queued?: boolean; left_queue?: unknown; draft?: boolean }): { state: PrState; checks?: Checks } {
  if (l.state === "merged" || l.state === "closed") return { state: l.state };
  const state: PrState = l.queued ? "queued" : l.draft ? "draft" : "open";
  const checks: Checks | undefined = l.state === "checks_failed" || l.left_queue ? "failed" : l.state === "checks_running" ? "running" : undefined;
  return { state, checks };
}

/** Whether a word is a PR's state, as control's chat cards say it. */
export const isPrState = (s: string): s is PrState => Object.hasOwn(SAID, s);

/** The state's icon, then its checks' mark when there are checks to say;
 * `word`, after them in the state's colour, where nothing else says it. */
export function PrMark({ state, checks, size = "sm", word, ...data }: { state: PrState; checks?: Checks; size?: IconSize; word?: string; [data: `data-${string}`]: unknown }) {
  const Icon = ICON[state];
  const Check = checks && CHECK[checks];
  const said = `pull request ${SAID[state]}${checks ? `, checks ${checks}` : ""}`;
  return (
    <span class="k-pr" data-pr={state} data-checks={checks} role={word ? undefined : "img"} aria-label={word ? undefined : said} title={said} data-kit="pr" {...data}>
      <Icon size={size} />
      {Check && (
        <span class="k-pr-checks">
          <Check size="xs" />
        </span>
      )}
      {word}
    </span>
  );
}
