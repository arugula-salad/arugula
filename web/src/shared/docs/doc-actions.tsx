// A Doc's three actions (Arugula #782, #820): Copy link, Share (Update when
// the shared copy is older) and Open in browser, as the same small icon
// buttons wherever a Doc is shown: at the right of its bar, in the Docs
// pages' previews and in the platform's Doc page. This is the buttons; what
// each does is the caller's, because a Doc open in a pane, a library Doc on
// the machine's page and a Doc on the platform reach their link differently.
// Share means two things: on the machine it makes a public link ("Share with
// a link"), on the platform it puts the Doc in a team ("Share with <team>"),
// so the caller says which in `labels`.

import type { ComponentChildren } from "preact";
import { useRef, useState } from "preact/hooks";
import { CheckIcon, ExternalIcon, LinkIcon, ShareIcon } from "../ui/icons";
import "./doc-actions.css";

export const NO_LINK = "No link yet: share it to get one";

type Hooks = Record<`data-${string}`, string | undefined>;

export function DocActions({
  none,
  copy,
  open,
  share,
  older,
  labels,
  hooks,
  children,
}: {
  /** It has no link to copy or open: not shared, and no page on the platform. */
  none: boolean;
  /** Copy the link: false when nothing was copied. */
  copy: () => Promise<"ok" | false>;
  open: () => void;
  /** Share, or Update when `older`. Left out, there is no Share: a viewer
   * with a role doesn't own the Doc, and a team's Doc has nowhere to go. */
  share?: (e: MouseEvent) => void;
  /** The shared copy is older than the Doc: Share reads Update, with a dot. */
  older?: boolean;
  /** What Share and Update say, as their tooltip and name. Share is a public
   * link unless the caller says otherwise. */
  labels?: { share?: string; update?: string };
  /** The caller's own hooks on each button. */
  hooks?: { copy?: Hooks; share?: Hooks; open?: Hooks };
  /** After the three, inside the same group: the platform's ⋯. */
  children?: ComponentChildren;
}) {
  const [copied, setCopied] = useState(false);
  const timer = useRef<number | undefined>(undefined);
  const click = async () => {
    const how = await copy();
    if (!how) return;
    setCopied(true);
    clearTimeout(timer.current);
    timer.current = window.setTimeout(() => setCopied(false), 1500);
  };
  const shareTitle = older ? (labels?.update ?? "Update the shared copy: it's older than this Doc") : (labels?.share ?? "Share with a link");
  return (
    <span class="doc-actions" data-doc-actions>
      <button
        type="button"
        class="doc-action bar-btn sq"
        data-copied={copied ? "ok" : undefined}
        disabled={none}
        title={none ? NO_LINK : "Copy link"}
        aria-label="Copy link"
        onClick={() => void click()}
        {...hooks?.copy}
      >
        {copied ? (
          <span class="doc-action-copied">
            Copied <CheckIcon size="xs" />
          </span>
        ) : (
          <LinkIcon />
        )}
      </button>
      {share && (
        <button type="button" class="doc-action bar-btn sq" data-older={older ? "" : undefined} title={shareTitle} aria-label={shareTitle} onClick={(e) => share(e)} {...hooks?.share}>
          <ShareIcon />
        </button>
      )}
      <button type="button" class="doc-action bar-btn sq" disabled={none} title={none ? NO_LINK : "Open in browser"} aria-label="Open in browser" data-doc-open-browser onClick={open} {...hooks?.open}>
        <ExternalIcon />
      </button>
      {children}
    </span>
  );
}

/** Open a link in a new tab, with no way back to this page. */
export const openTab = (url: string) => void window.open(url, "_blank", "noopener,noreferrer");
