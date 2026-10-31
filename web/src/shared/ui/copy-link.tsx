// The Copy link button (#411): top-right in a Doc's or a board's header, the
// same everywhere. A click copies the team link and says "Copied ✓" in
// place, on every device. A ▾ beside it holds the rarer items (open in the
// browser, a public link).

import { useRef, useState } from "preact/hooks";
import { openMenu, type MenuItem } from "./menu";
import "./copy-link.css";

const LinkIcon = () => (
  <svg class="copy-link-icon" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
    <path d="M10 14a4 4 0 0 0 5.7 0l3-3a4 4 0 0 0-5.7-5.7l-1 1" />
    <path d="M14 10a4 4 0 0 0-5.7 0l-3 3a4 4 0 0 0 5.7 5.7l1-1" />
  </svg>
);

/**
 * - `link`: the team link a click copies. `null` while there isn't one
 *   (no platform page to name): the button isn't drawn, unless `onClick` is.
 * - `menu`: what the ▾ opens. None, no ▾.
 * - `onClick`: instead of copying `link`, what a click does (a Doc that
 *   lives on one machine has no team link yet; its click starts the public
 *   link's flow). It says nothing of its own.
 */
export function CopyLinkButton({ link, menu, onClick }: { link: string | null; menu?: MenuItem[]; onClick?: () => void }) {
  const [copied, setCopied] = useState<"ok" | "no" | null>(null);
  const timer = useRef<number | undefined>(undefined);
  if (link === null && !onClick) return null;
  const say = (s: "ok" | "no") => {
    setCopied(s);
    clearTimeout(timer.current);
    timer.current = window.setTimeout(() => setCopied(null), 1500);
  };
  const click = async () => {
    if (onClick) return onClick();
    if (link === null) return;
    try {
      await navigator.clipboard.writeText(link);
      say("ok");
    } catch {
      say("no");
    }
  };
  return (
    <span class="copy-link" data-copy-link data-link={link ?? undefined} data-copied={copied ?? undefined}>
      <button type="button" data-copy-link-main title="Copy link" onClick={() => void click()}>
        {copied === "ok" ? "Copied ✓" : copied === "no" ? "Couldn't copy" : (
          <>
            <LinkIcon /> Copy link
          </>
        )}
      </button>
      {menu && menu.length > 0 && (
        <button type="button" data-copy-link-more aria-label="More ways to share" aria-haspopup="menu" onClick={(e) => openMenu(e, menu, e.currentTarget)}>
          ▾
        </button>
      )}
    </span>
  );
}
