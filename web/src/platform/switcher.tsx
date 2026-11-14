// The project switcher (Arugula #895; the prototype's, Arugula #989): a
// popover hanging from the rail's project tile (or a phone's bar), one list
// under small headings: Everything (All projects), each team's projects,
// then yours (Just you). Each project has its letter tile and a count of
// what wants you there (./inbox-count). The current one is lit. Archived
// projects wait behind "Show archived (n)"; New project… asks for a name
// and an owner. A team's own pages aren't here: its rooms are the rail's
// Chat, and its settings and members are reached from Settings.

import type { ComponentChildren } from "preact";
import { useState } from "preact/hooks";
import { useSubscribe } from "../shared/ui/hooks";
import { Popover } from "../shared/ui/menu";
import { Modal } from "../shared/ui/modal";
import { PlusIcon } from "../shared/ui/icons";
import type { PlatformProject } from "../shared/proto.gen";
import { useInboxByProject } from "./inbox-count";
import { names } from "./links";
import { makers, ownerKey, owners, PERSONAL, type Owner } from "./owners";
import { follow, go, pathOf } from "./router";
import { session } from "./session";
import { placeOf } from "./places-kept";

/** A project's tile: "Me" for one of your own, else its name's first
 * letter; "∗" for All projects. */
export function initialsOf(p: Pick<PlatformProject, "name" | "owner" | "team"> | undefined): string {
  if (!p) return "∗";
  if (ownerKey(p) === PERSONAL) return "Me";
  return (p.name.trim()[0] ?? "?").toUpperCase();
}

/** The switcher's popover, hanging from `from`. `project`: the one the page
 * is in; `owner`: whose New project… asks for first. */
export function Switcher({ from, project, owner, close, beside = false }: { from: { current: HTMLElement | null }; project: string | undefined; owner: string | undefined; close: () => void; beside?: boolean }) {
  useSubscribe((fn) => session.subscribe(fn));
  const counts = useInboxByProject();
  const all = owners(session.projects);
  const [archived, setArchived] = useState<string | null>(null);
  const [making, setMaking] = useState(false);
  const leave = (e: MouseEvent, to: string) => {
    follow(e, to);
    close();
  };
  const row = (key: string, to: string, tile: string, name: ComponentChildren, n: number, here: boolean, data: Record<string, string>) => (
    <a key={key} class="menu-item switcher-row" href={to} aria-current={here ? "page" : undefined} {...data} onClick={(e) => leave(e, to)}>
      <span class="switcher-tile" aria-hidden="true">
        {tile}
      </span>
      <span class="switcher-name">{name}</span>
      {n > 0 && (
        <span class="switcher-count" title={`${n} want${n === 1 ? "s" : ""} you`}>
          {n}
        </span>
      )}
    </a>
  );
  // Where you were in it (Arugula #989), else its Overview.
  const projectRow = (p: PlatformProject) =>
    row(p.id, placeOf(p.id) ?? pathOf({ page: "project", project: p.id }, names), initialsOf(p), p.name, counts.by.get(p.id) ?? 0, p.id === project, { "data-switcher-project": p.id });
  const group = (o: Owner, heading: ComponentChildren) => (
    <div key={o.key} class="switcher-group" role="group" aria-label={o.name} data-switcher-owner={o.key}>
      <div class="menu-header switcher-head">{heading}</div>
      {o.projects.map(projectRow)}
      {o.archived.length > 0 && (
        <button class="menu-item faded" aria-expanded={archived === o.key} data-switcher-archived={o.archived.length} onClick={() => setArchived(archived === o.key ? null : o.key)}>
          {archived === o.key ? "Hide archived" : `Show archived (${o.archived.length})`}
        </button>
      )}
      {archived === o.key && o.archived.map(projectRow)}
    </div>
  );
  if (making) return <NewProject owner={owner && all.some((o) => o.key === owner) ? owner : PERSONAL} close={close} />;
  const personal = all.find((o) => o.kind === "personal");
  return (
    <Popover from={from} beside={beside} label="Projects" attrs={{ "data-switcher": true }} onClose={close}>
      <div class="switcher">
        <div class="switcher-group" role="group" aria-label="Everything">
          <div class="menu-header switcher-head">Everything</div>
          {row("all", "/", "∗", "All projects", counts.all, !project, { "data-switcher-all": "" })}
        </div>
        {all.filter((o) => o.kind === "team").map((o) => group(o, o.name))}
        {personal && group(personal, "Just you")}
        <div class="menu-sep" />
        <button class="menu-item switcher-new" data-switcher-new onClick={() => setMaking(true)}>
          <PlusIcon /> New project
        </button>
      </div>
    </Popover>
  );
}

/** New project…: a name and an owner, the one chosen in the switcher if the
 * reader may make one there (a team's owners may; anyone for Personal). */
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
