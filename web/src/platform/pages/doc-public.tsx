// A team Doc's public link on its page (Arugula #871): make it, copy it, update
// what it shows and stop sharing. The platform keeps the link's key and
// gives it to anyone who reads the Doc, so a viewer sees the URL; owners and
// editors change it. This browser seals the Doc and puts it to control
// (shared/docs/public-link.ts): control never has the key.

import { useState } from "preact/hooks";
import type { ProjectDocView } from "../../shared/proto";
import { publicBehind, revokePublic, sharePublic } from "../../shared/docs/public-link";
import { toast } from "../toast";

export function PublicLinkBar({ project, view, reload }: { project: string; view: ProjectDocView; reload: () => void }) {
  const [busy, setBusy] = useState(false);
  const link = view.public && view.public.ver > 0 ? view.public : undefined;
  // A reader has nothing to see until it's shared; an owner or editor can share.
  if (!link && !view.writer) return null;
  const run = async (what: () => Promise<unknown>, said?: string) => {
    if (busy) return;
    setBusy(true);
    try {
      await what();
      if (said) toast(said);
    } catch (e) {
      toast((e as Error).message);
    }
    setBusy(false);
    reload();
  };
  const copy = async () => {
    try {
      await navigator.clipboard.writeText(link!.url);
      toast("Public link copied");
    } catch {
      toast("Couldn't copy the link");
    }
  };
  return (
    <div class="project-doc-form doc-public" data-doc-public data-doc-public-shared={link ? "true" : "false"}>
      {link ? (
        <>
          <span class="dim">Anyone with the link can read this Doc:</span>{" "}
          <input type="text" readOnly value={link.url} data-doc-public-url onFocus={(e) => (e.currentTarget as HTMLInputElement).select()} />{" "}
          <button type="button" class="secondary small" onClick={() => void copy()} data-doc-public-copy>
            Copy link
          </button>
          {publicBehind(link) && (
            <>
              {" "}
              <span class="dim" data-doc-public-behind>
                The link shows an older version.
              </span>
              {view.writer && (
                <button type="button" class="secondary small" disabled={busy} onClick={() => void run(() => sharePublic(project, view.doc.id), "The link is up to date")} data-doc-public-update>
                  Update
                </button>
              )}
            </>
          )}
          {view.writer && (
            <button type="button" class="secondary small danger" disabled={busy} onClick={() => void run(() => revokePublic(project, view.doc.id), "The link no longer opens")} data-doc-public-stop>
              Stop sharing
            </button>
          )}
        </>
      ) : (
        <button type="button" class="secondary small" disabled={busy} onClick={() => void run(() => sharePublic(project, view.doc.id), "Anyone with the link can read this Doc")} data-doc-public-share>
          Share with a public link
        </button>
      )}
    </div>
  );
}
