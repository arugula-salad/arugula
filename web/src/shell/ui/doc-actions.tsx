// A Doc's three actions (Arugula #782): Copy link, Share (Update when the
// shared copy is older) and Open in browser, as the same small icon buttons
// wherever a Doc is shown: at the right of its bar, and in the Docs page's
// preview. This is the buttons; what each does is the caller's, because a Doc
// open in a pane and a library Doc on the page reach their link differently.

import { useRef, useState } from "preact/hooks";
import { CheckIcon, ExternalIcon, LinkIcon, ShareIcon } from "../../shared/ui/icons";

export const NO_LINK = "Not on the platform yet: sign in to Arugula control to get a link";
export const ONLY_YOU = "only you can open it until it's shared";

type Hooks = Record<`data-${string}`, string | undefined>;

export function DocActions({
  none,
  copy,
  open,
  share,
  older,
  hooks,
}: {
  /** It has no link to copy or open: not on the platform, not shared. */
  none: boolean;
  /** Copy the link: "private" when only its owner can open what was copied,
   * false when nothing was copied. */
  copy: () => Promise<"ok" | "private" | false>;
  open: () => void;
  /** Open the share dialog. Left out, there is no Share: a viewer with a
   * role doesn't own the Doc. */
  share?: () => void;
  /** The shared copy is older than the Doc: Share reads Update, with a dot. */
  older?: boolean;
  /** The caller's own hooks on each button. */
  hooks?: { copy?: Hooks; share?: Hooks; open?: Hooks };
}) {
  const [copied, setCopied] = useState<"ok" | "private" | null>(null);
  const timer = useRef<number | undefined>(undefined);
  const click = async () => {
    const how = await copy();
    if (!how) return;
    setCopied(how);
    clearTimeout(timer.current);
    timer.current = window.setTimeout(() => setCopied(null), 1500);
  };
  const shareTitle = older ? "Update the shared copy: it's older than this Doc" : "Share…";
  return (
    <span class="doc-actions" data-doc-actions>
      <button
        type="button"
        class="doc-action bar-btn sq"
        data-copied={copied ?? undefined}
        disabled={none}
        title={none ? NO_LINK : copied === "private" ? `Copied: ${ONLY_YOU}` : "Copy link"}
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
        <button type="button" class="doc-action bar-btn sq" data-older={older ? "" : undefined} title={shareTitle} aria-label={shareTitle} onClick={share} {...hooks?.share}>
          <ShareIcon />
        </button>
      )}
      <button type="button" class="doc-action bar-btn sq" disabled={none} title={none ? NO_LINK : "Open in browser"} aria-label="Open in browser" data-doc-open-browser onClick={open} {...hooks?.open}>
        <ExternalIcon />
      </button>
    </span>
  );
}

/** Open a link in a new tab, with no way back to this page. */
export const openTab = (url: string) => void window.open(url, "_blank", "noopener,noreferrer");
