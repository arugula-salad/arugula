// Share with… (Arugula #418): a machine Doc's copy on the platform, turned
// into a team Doc in a project the person picks. One list, in the platform's
// copy page and in the machine's share dialog, so both give the same rows:
// each project the person writes Docs in, with Share, or once shared there
// "Made <ago>", Open and Update copy. The team Doc is a snapshot (the team
// owns and edits it); Update copy replaces it from the copy when asked, and
// the copy having changed since is what enables it. Both pages call the
// platform as the person (`platformFetch`), so a shell that isn't served
// through control gets `unreachable` and says so.

import type { ComponentChildren } from "preact";
import { useEffect, useState } from "preact/hooks";
import type { PlatformProject, PlatformProjects, ProjectDocCopy } from "../proto";
import { platformFetch } from "../maintenance";
import { Modal } from "../ui/modal";
import { Time } from "../ui/time";
import { DocError, projectDocs } from "./store";
import "./share-with.css";

/** Update copy found the team's edits: what the host asks about before it
 * runs `go`, which replaces them. */
export interface Edited {
  project: string;
  go: () => Promise<void>;
}

/** The dialog that asks it: the host shows it in place of the list, so
 * there is one dialog at a time. */
export function EditedDialog({ edited, cancel }: { edited: Edited; cancel: () => void }) {
  return (
    <Modal
      alert
      x={false}
      title="Replace their edits?"
      label="Replace their edits"
      close={cancel}
      root={{ "data-share-with-confirm": true }}
      buttons={
        <>
          <button class="secondary" type="button" onClick={cancel}>
            Cancel
          </button>
          <button
            class="primary"
            type="button"
            data-share-with-replace
            onClick={() => {
              cancel();
              void edited.go();
            }}
          >
            Update copy
          </button>
        </>
      }
    >
      <p>{edited.project} edited its copy since; Update copy replaces their edits.</p>
    </Modal>
  );
}

/** The projects the person makes Docs in, other than their own. */
const teams = (ps: PlatformProject[]) => ps.filter((p) => p.team && p.team.role !== "viewer");

type Loaded = { projects: PlatformProject[]; copies: ProjectDocCopy[] };

/**
 * The rows for the copy `id` in `project`. `ask` hears when Update copy
 * would replace the team's edits; `say` hears a refusal (an error toast);
 * `done` hears a success, for a host whose row doesn't say it already;
 * `open` follows a team Doc's link in the host (else it opens in a tab);
 * `unreachable` is what shows when the platform can't be called as the
 * person from here.
 */
export function ShareWith({
  project,
  id,
  ask,
  say,
  done,
  open,
  unreachable = null,
}: {
  project: string;
  id: string;
  ask: (e: Edited) => void;
  say: (message: string) => void;
  done?: (message: string) => void;
  open?: (c: ProjectDocCopy) => void;
  unreachable?: ComponentChildren;
}) {
  const [got, setGot] = useState<Loaded | "unreachable" | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const load = async () => {
    try {
      const r = await platformFetch("/api/projects", { credentials: "same-origin" });
      // A daemon with no control answers something that isn't the list.
      const l = r.ok ? ((await r.json()) as PlatformProjects) : null;
      if (!l || !Array.isArray(l.projects)) return setGot("unreachable");
      const { copies } = await projectDocs.copies(project, id);
      setGot({ projects: teams(l.projects), copies });
    } catch {
      setGot("unreachable");
    }
  };
  useEffect(() => void load(), [project, id]);
  if (got === "unreachable") return <>{unreachable}</>;
  if (!got) return <p class="dim">Loading…</p>;
  if (!got.projects.length) return <p class="dim">You aren't on a team with a project to share it with.</p>;

  const refused = (e: unknown) => say((e as Error).message);
  const share = async (p: PlatformProject) => {
    setBusy(p.id);
    try {
      await projectDocs.share(project, id, p.id);
      done?.(`Shared with ${p.name}`);
    } catch (e) {
      // Someone made it there since the list was read: it shows as made.
      if (!(e instanceof DocError && e.status === 409)) refused(e);
    }
    setBusy(null);
    await load();
  };
  const update = async (c: ProjectDocCopy, replace: boolean): Promise<void> => {
    setBusy(c.project);
    try {
      await projectDocs.updateCopy(project, id, c.project, replace);
      done?.(`${c.project_name}'s copy now matches`);
    } catch (e) {
      if (e instanceof DocError && e.status === 409 && e.body.edited) ask({ project: c.project_name, go: () => update(c, true) });
      else refused(e);
    }
    setBusy(null);
    await load();
  };
  return (
    <ul class="share-with" data-share-with>
      {got.projects.map((p) => {
        const c = got.copies.find((c) => c.project === p.id);
        return (
          <li key={p.id} data-share-with-project={p.id}>
            <span class="share-with-name">{p.name}</span>
            {c ? (
              <>
                <span class="dim" data-share-with-made>
                  Made <Time at={c.made} />
                </span>
                <a
                  class="link"
                  href={c.link}
                  target={open ? undefined : "_blank"}
                  rel="noopener"
                  data-share-with-open
                  onClick={open ? (e) => (e.preventDefault(), open(c)) : undefined}
                >
                  Open
                </a>
                <button
                  type="button"
                  class="secondary small"
                  data-share-with-update
                  disabled={!c.stale || busy !== null}
                  title={c.stale ? `Replace ${p.name}'s Doc with this one as it is now` : "The Doc hasn't changed since it was shared"}
                  onClick={() => void update(c, false)}
                >
                  Update copy
                </button>
              </>
            ) : (
              <button type="button" class="secondary small" data-share-with-go disabled={busy !== null} onClick={() => void share(p)}>
                Share
              </button>
            )}
          </li>
        );
      })}
    </ul>
  );
}
