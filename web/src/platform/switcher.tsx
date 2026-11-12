// The bar's project switcher (Arugula #895, option B of Doc
// pdc_9640f67e1745ddbe2b7733831fe82c1f): a popover with the reader's owners
// on the left, Personal then each team, and the chosen owner's projects on
// the right, under that owner's own links. It opens on the current
// project's owner. Archived projects wait behind "Show archived (n)". On a
// phone the two stack: the owners, then one owner's projects with a way
// back to them. New project… asks for a name and an owner.

import { useState } from "preact/hooks";
import { usePhone, useSubscribe } from "../shared/ui/hooks";
import { Popover } from "../shared/ui/menu";
import { Modal } from "../shared/ui/modal";
import { BackIcon } from "../shared/ui/icons";
import type { PlatformProject } from "../shared/proto.gen";
import { names } from "./links";
import { makers, owners, PERSONAL, type Owner } from "./owners";
import { follow, go, pathOf } from "./router";
import { session } from "./session";

/** Where an owner's links go: a team's pages; Personal's settings are the
 * account's. */
function ownerLinks(o: Owner): { label: string; to: string; data: string }[] {
  if (o.kind === "personal") return [{ label: "Settings", to: "/settings", data: "settings" }];
  return [
    { label: "Chat", to: pathOf({ page: "team", team: o.key, tab: "chat" }, names), data: "chat" },
    { label: "Settings", to: pathOf({ page: "team", team: o.key, tab: "settings" }, names), data: "settings" },
    { label: "Members", to: pathOf({ page: "team", team: o.key, tab: "members" }, names), data: "members" },
  ];
}

/** The switcher's popover, hanging from `from`. `project`: the one the bar
 * is in; `owner`: the owner it opens on. */
export function Switcher({ from, project, owner, close }: { from: { current: HTMLElement | null }; project: string | undefined; owner: string | undefined; close: () => void }) {
  useSubscribe((fn) => session.subscribe(fn));
  const phone = usePhone();
  const all = owners(session.projects);
  const [chosen, choose] = useState(owner && all.some((o) => o.key === owner) ? owner : PERSONAL);
  // A phone shows one column at a time, the owners first.
  const [step, setStep] = useState<"owners" | "projects">("owners");
  const [archived, setArchived] = useState(false);
  const [making, setMaking] = useState(false);
  const o = all.find((x) => x.key === chosen) ?? all[0];
  const leave = (e: MouseEvent, to: string) => {
    follow(e, to);
    close();
  };
  const row = (p: PlatformProject) => {
    const to = pathOf({ page: "project", project: p.id }, names);
    const here = p.id === project;
    return (
      <a key={p.id} class="menu-item switcher-project" href={to} aria-current={here ? "page" : undefined} data-switcher-project={p.id} onClick={(e) => leave(e, to)}>
        <span class="menu-lead menu-check" aria-hidden="true">
          {here ? "●" : ""}
        </span>
        {p.name}
      </a>
    );
  };
  if (making) return <NewProject owner={o.key} close={close} />;
  const ownerList = (
    <div class="switcher-owners" role="group" aria-label="Owners" data-switcher-owners>
      <a class="menu-item" href="/" aria-current={!project ? "page" : undefined} data-switcher-all onClick={(e) => leave(e, "/")}>
        All projects
      </a>
      <div class="menu-header">Owners</div>
      {all.map((x) => (
        <button
          key={x.key}
          class="menu-item switcher-owner"
          aria-pressed={!phone && x.key === o.key}
          data-switcher-owner={x.key}
          onClick={() => {
            choose(x.key);
            setArchived(false);
            setStep("projects");
          }}
        >
          {x.name}
        </button>
      ))}
    </div>
  );
  const projects = (
    <div class="switcher-projects" data-switcher-projects={o.key}>
      {phone && (
        <button class="menu-item" data-switcher-back onClick={() => setStep("owners")}>
          <span class="menu-lead" aria-hidden="true">
            <BackIcon />
          </span>
          Owners
        </button>
      )}
      <div class="switcher-head">
        <span class="menu-header named">{o.name}</span>
        <span class="switcher-links">
          {ownerLinks(o).map((l) => (
            <a key={l.data} href={l.to} data-switcher-link={l.data} onClick={(e) => leave(e, l.to)}>
              {l.label}
            </a>
          ))}
        </span>
      </div>
      {o.projects.map(row)}
      {!o.projects.length && <p class="menu-note">No projects yet.</p>}
      {o.archived.length > 0 && (
        <button class="menu-item faded" aria-expanded={archived} data-switcher-archived={o.archived.length} onClick={() => setArchived(!archived)}>
          {archived ? "Hide archived" : `Show archived (${o.archived.length})`}
        </button>
      )}
      {archived && o.archived.map(row)}
      <div class="menu-sep" />
      <button class="menu-item" data-switcher-new onClick={() => setMaking(true)}>
        New project…
      </button>
    </div>
  );
  return (
    <Popover from={from} label="Projects" attrs={{ "data-switcher": true }} onClose={close}>
      <div class={phone ? "switcher phone" : "switcher"}>
        {(!phone || step === "owners") && ownerList}
        {(!phone || step === "projects") && projects}
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
