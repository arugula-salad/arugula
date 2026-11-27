// New project… and New project from… (Arugula #895, #1040), Go to…'s
// commands since the switcher's popover went (Lift 3, #1479). New project…
// asks for a name and an owner; New project from…, for a reader who owns a
// project, asks which, then opens its "Move to project…" (./move), where
// the target starts as a new project.

import { useState } from "preact/hooks";
import { useSubscribe } from "../shared/ui/hooks";
import { Modal } from "../shared/ui/modal";
import type { PlatformProject } from "../shared/proto.gen";
import { names } from "./links";
import { makers, ownerKey, owners, PERSONAL } from "./owners";
import { go, pathOf } from "./router";
import { session } from "./session";

/** Which is asked for now, and from where. */
let making: { kind: "new" | "from"; project: string | undefined } | null = null;
const listeners = new Set<() => void>();
const set = (to: typeof making) => {
  making = to;
  listeners.forEach((fn) => fn());
};

/** New project… (`new`) or New project from… (`from`), the page being in `project`. */
export const openNewProject = (kind: "new" | "from", project: string | undefined) => set({ kind, project });

/** The projects the reader owns: those a split can start from. */
export const ownedProjects = () => makers(owners(session.projects)).flatMap((o) => o.projects);

/** Whichever is asked for; once, on the app's page. */
export function NewProjectLayer() {
  useSubscribe((fn) => {
    listeners.add(fn);
    return () => void listeners.delete(fn);
  });
  if (!making) return null;
  const close = () => set(null);
  const p = session.project(making.project);
  if (making.kind === "new") return <NewProject owner={p ? ownerKey(p) : PERSONAL} close={close} />;
  const owned = ownedProjects();
  return owned.length ? <NewProjectFrom owned={owned} project={making.project} close={close} /> : null;
}

/** New project…: a name and an owner, the project's owner if the reader
 * may make one there (a team's owners may; anyone for Personal). */
function NewProject({ owner, close }: { owner: string; close: () => void }) {
  const can = makers(owners(session.projects));
  const [name, setName] = useState("");
  const [to, setTo] = useState(can.some((o) => o.key === owner) ? owner : PERSONAL);
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState("");
  const submit = async (e: Event) => {
    e.preventDefault();
    const n = name.trim();
    if (!n) return;
    setBusy(true);
    setErr("");
    try {
      const made = await session.create({ name: n, owner: to === PERSONAL ? { kind: "personal" } : { kind: "team", id: to } });
      close();
      go(made.id ? pathOf({ page: "project", project: made.id }, names) : "/");
    } catch (e) {
      setErr((e as Error).message);
      setBusy(false);
    }
  };
  return (
    <Modal
      close={close}
      title="New project"
      onSubmit={(e) => void submit(e)}
      root={{ "data-new-project": true }}
      buttons={
        <>
          <button type="button" class="secondary" onClick={close}>
            Cancel
          </button>
          <button type="submit" class="primary" disabled={busy || !name.trim()} data-new-project-create>
            {busy ? "Creating…" : "Create"}
          </button>
        </>
      }
    >
      <label>
        Name
        <input autofocus required value={name} maxLength={100} data-new-project-name onInput={(e) => setName(e.currentTarget.value)} />
      </label>
      <label>
        Owner
        <select value={to} data-new-project-owner onChange={(e) => setTo(e.currentTarget.value)}>
          {can.map((o) => (
            <option key={o.key} value={o.key}>
              {o.name}
            </option>
          ))}
        </select>
      </label>
      {can.length < owners(session.projects).length && <p class="dim">Only a team's owners make its projects.</p>}
      {err && (
        <p class="control-error" role="alert" data-new-project-error>
          {err}
        </p>
      )}
    </Modal>
  );
}

/** New project from…: which of the reader's own projects to move some work
 * out of, the one they're in if it's theirs; then its "Move to project…". */
function NewProjectFrom({ owned, project, close }: { owned: PlatformProject[]; project: string | undefined; close: () => void }) {
  const [from, setFrom] = useState(owned.some((p) => p.id === project) ? project! : owned[0].id);
  return (
    <Modal
      close={close}
      title="New project from…"
      onSubmit={(e) => {
        e.preventDefault();
        close();
        go(pathOf({ page: "move", project: from }, names));
      }}
      root={{ "data-new-project-from": true }}
      buttons={
        <>
          <button type="button" class="secondary" onClick={close}>
            Cancel
          </button>
          <button type="submit" class="primary" data-new-project-from-go>
            Continue
          </button>
        </>
      }
    >
      <label>
        Move some work out of
        <select value={from} data-new-project-from-source onChange={(e) => setFrom(e.currentTarget.value)}>
          {owned.map((p) => (
            <option key={p.id} value={p.id}>
              {p.name}
            </option>
          ))}
        </select>
      </label>
      <p class="dim">Next, pick its repos, boards, issues and Docs for the new project.</p>
    </Modal>
  );
}
