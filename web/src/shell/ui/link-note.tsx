// A read-only link's page (M19, `#link=…`) says what it is (UI audit
// #149): the viewer watches and can't type, and the link ends, at a time
// the link carries when it was made since (else it says only read-only).
// Opening the page doesn't connect: the page waits for someone to press its
// button in a window that has the focus, and only then fetches anything.

import { render } from "preact";
import { useEffect, useState } from "preact/hooks";
import { Time } from "../../shared/ui/time";

/** Undefined: not a link's page. Null: one that doesn't say when it ends. */
let link: { ends: number | null } | undefined;

export function setReadOnlyLink(ends: number | null) {
  link = { ends: ends && Number.isFinite(ends) ? ends : null };
}

export function LinkNote() {
  const ends = link?.ends ?? null;
  const [over, setOver] = useState(() => ends !== null && ends <= Date.now());
  useEffect(() => {
    if (ends === null || over) return;
    const t = setTimeout(() => setOver(true), ends - Date.now());
    return () => clearTimeout(t);
  }, [ends, over]);
  if (!link) return null;
  return (
    <div class="ws-stale maintenance-note" role="status" data-read-only-link={ends ?? ""}>
      Read-only link
      {ends !== null ? (
        <>
          {" "}
          · {over ? "ended" : "ends"} <Time at={ends} />
        </>
      ) : (
        ""
      )}
    </div>
  );
}

/** What a link's page shows until someone opens it, or once it's over. */
function LinkPrompt({ ends, open }: { ends: number | null; open: () => void }) {
  const [over, setOver] = useState(() => ends !== null && ends <= Date.now());
  if (over) return <LinkEnded />;
  return (
    <div class="prompt-backdrop">
      <div class="prompt control-prompt" role="dialog" aria-label="Read-only link" data-link-gate>
        <h2>Read-only link</h2>
        <p>Someone is sharing a session on their machine with you. You can watch it live here, but not type into it, and you don't need an account.</p>
        <p class="dim">
          Nothing connects until you open it.
          {ends !== null ? (
            <>
              {" "}
              The link ends <Time at={ends} />.
            </>
          ) : null}
        </p>
        <div class="prompt-buttons">
          <button
            class="primary"
            data-link-open
            onClick={(e) => {
              // A person's click, here: a script's `click()` isn't trusted,
              // and a window in the background hasn't got the focus.
              if (!e.isTrusted || !document.hasFocus()) return;
              if (ends !== null && ends <= Date.now()) setOver(true);
              else open();
            }}
          >
            Watch this session
          </button>
        </div>
      </div>
    </div>
  );
}

/** Resolves once someone opens the link. Nothing else on the page has run. */
export function waitForLinkOpen(root: HTMLElement, ends: number | null): Promise<void> {
  return new Promise((resolve) => {
    render(
      <LinkPrompt
        ends={ends && Number.isFinite(ends) ? ends : null}
        open={() => {
          render(null, root);
          resolve();
        }}
      />,
      root,
    );
  });
}

/** A link that's over: its owner ended it, or its time ran out. */
export function LinkEnded() {
  return (
    <div class="prompt-backdrop">
      <div class="prompt control-prompt" role="status" data-link-ended>
        <h2>This link has ended</h2>
        <p>Whoever shared it ended it, or its time ran out. Ask them for a new one if you still need it.</p>
      </div>
    </div>
  );
}
