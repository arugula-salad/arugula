// An owner's settings (Arugula #897): a team's at `/t/:team/settings`,
// Personal's at `/settings/personal`. One scrolling page with the section
// index beside it, as the account's and a project's settings are: General
// (its default project), Members, Machines, Chat (the team chat's
// owners-only switches, from a project's settings), Plan and billing (from
// the account's settings), Labs, and Project defaults.
//
// Project defaults is a table: a row per key a project overrides, the
// owner's default first, then a column per project that isn't archived,
// sideways when it's wide with the key and the default kept in place.
// Every cell is a select, and a change is staged (../settings/overrides):
// a bar above the table counts them, Save sends one PUT each and says
// which failed, Cancel drops them, and leaving asks first. A team's
// viewers read it; only its owners choose its default project.

import { useEffect, useRef, useState } from "preact/hooks";
import type { SessionSettings } from "../../shared/proto";
import { platformFetch } from "../../shared/maintenance";
import { useSubscribe } from "../../shared/ui/hooks";
import { PageHeader } from "../../shared/ui/page-header";
import { Section, SectionIndex } from "../../shared/ui/settings-section";
import { names } from "../links";
import { follow, pathOf, useHoldLeaving } from "../router";
import { runs, type HostedWorkspace } from "../runs";
import { controlOrigin, session, type PlatformProject } from "../session";
import { TeamChat } from "../settings/chat-alerts";
import { PlanAndBilling } from "../settings/plan";
import {
  ISSUE_HOMES,
  KEYS,
  OWNER,
  STALL_MINUTES,
  changes,
  choicesWith,
  defaultOf,
  infoOf,
  optionOf,
  ownerCell,
  ownerOf,
  ownOf,
  projectCell,
  send,
  settingOf,
  stage,
  valueOf,
  words,
  writes,
  type Cell,
  type EffectiveSetting,
  type Key,
  type Layers,
  type Staged,
  type Value,
  type Write,
} from "../settings/overrides";
import { Missing } from "./project";

/** A call to the platform: its JSON, or its `{error}` thrown, with the status. */
async function call<T>(method: string, path: string, body?: unknown): Promise<T> {
  const r = await platformFetch(path, {
    method,
    credentials: "same-origin",
    headers: body === undefined ? {} : { "Content-Type": "application/json" },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  const v = (await r.json().catch(() => ({}))) as { error?: string };
  if (!r.ok) throw Object.assign(new Error(v.error ?? `Request failed (${r.status})`), { status: r.status });
  return v as T;
}

const enc = encodeURIComponent;

/** The owner's projects that aren't archived, by name. */
function projectsOf(owner: string): PlatformProject[] {
  return session.projects.filter((p) => !p.archived && ownerOf(p).owner === owner).sort((a, b) => a.name.localeCompare(b.name));
}

/** The reader's role in the owner: Personal is theirs; a team's, as its
 * project says. */
function roleIn(owner: string): string | undefined {
  if (owner === "personal") return "owner";
  return session.projects.find((p) => p.team?.id === owner)?.team?.role;
}

export function OwnerSettings({ owner }: { owner: string }) {
  useSubscribe((fn) => session.subscribe(fn));
  const body = useRef<HTMLDivElement>(null);
  const projects = projectsOf(owner);
  const team = owner === "personal" ? null : session.projects.find((p) => p.team?.id === owner)?.team ?? null;
  const name = owner === "personal" ? "Personal" : (team?.name ?? (projects[0] && ownerOf(projects[0]).name) ?? "Team");
  const role = roleIn(owner);
  if (owner !== "personal" && !team && projects.length === 0) return <Missing what="That team isn't one of yours (or it's gone)." />;
  const account = pathOf({ page: "settings" }, names);
  return (
    <section class="platform-section sw-page settings-page" data-owner-settings={owner}>
      <div class="settings-layout">
        <SectionIndex body={body} />
        <div class="settings-body" ref={body}>
          <PageHeader title={`${name} settings`} back={{ href: account, label: "Your settings", onClick: (e) => follow(e, account) }} />
          <General owner={owner} projects={projects} role={role} />
          {team && (
            <Section id="members" title="Members" count={team.members.length} lede={`Who's in ${name}, and their role.`} hooks={{ "data-owner-members": owner }}>
              <ul class="platform-list settings-rows">
                {team.members.map((m) => (
                  <li key={m.account} data-owner-member={m.account}>
                    <b>{m.name}</b>
                    <span class="dim">{m.role}</span>
                  </li>
                ))}
              </ul>
              <p class="dim">
                Invite, remove and change roles on{" "}
                <a href={`${controlOrigin()}/#settings=account`} data-settings-control>
                  your machines page
                </a>
                : the roster is signed by an owner's device.
              </p>
            </Section>
          )}
          <Section id="machines" title="Machines" lede={team ? `The machines shared with ${name}, and what they may do, are on your machines page.` : "Your machines are on your machines page."}>
            <p>
              <a href={`${controlOrigin()}/`} data-settings-control>
                Your machines
              </a>
            </p>
          </Section>
          {team && <TeamChat team={owner} name={name} role={role} />}
          <PlanAndBilling team={team ? owner : undefined} />
          <Section id="labs" title="Labs" lede="Labs features are turned on per machine, in its Developer settings.">
            <p>
              <a href={`${controlOrigin()}/#settings=machine`} data-settings-control>
                This machine's settings
              </a>
            </p>
          </Section>
          <ProjectDefaults owner={owner} name={name} projects={projects} role={role} />
        </div>
      </div>
    </section>
  );
}

/** General: the owner's name, and its default project, which only its
 * owners choose. One select and a Save, so a change isn't one click. */
function General({ owner, projects, role }: { owner: string; projects: PlatformProject[]; role: string | undefined }) {
  const [read, setRead] = useState<EffectiveSetting[] | null>(null);
  const [draft, setDraft] = useState<Value>(null);
  const [err, setErr] = useState("");
  const [busy, setBusy] = useState(false);
  const path = `/api/owners/${enc(owner)}/settings`;
  const load = () =>
    call<EffectiveSetting[]>("GET", path).then(
      (v) => {
        setRead(v);
        setDraft(ownOf(settingOf(v, "default_project"), "owner"));
      },
      (e: Error) => setErr(e.message),
    );
  useEffect(() => void load(), [owner]);
  const saved = read ? ownOf(settingOf(read, "default_project"), "owner") : null;
  const changes = role === "owner";
  const save = async (e: Event) => {
    e.preventDefault();
    setBusy(true);
    setErr("");
    try {
      await call("PUT", path, { key: "default_project", value: draft });
      await load();
    } catch (x) {
      setErr((x as Error).message);
    } finally {
      setBusy(false);
    }
  };
  const nameOf = (id: string) => session.project(id)?.name;
  return (
    <Section id="general" title="General" lede="Its default project: where its new things go when nothing says which.">
      {read && (
        <form class="settings-row" onSubmit={save} data-default-project-form>
          <label>
            Default project{" "}
            {changes ? (
              <select class="field" value={optionOf(draft)} disabled={busy} data-default-project onChange={(e) => setDraft(e.currentTarget.value || null)}>
                <option value="">None</option>
                {choicesWith(
                  projects.map((p) => p.id),
                  draft,
                ).map((id) => (
                  <option key={String(id)} value={String(id)}>
                    {words("default_project", id, nameOf)}
                  </option>
                ))}
              </select>
            ) : (
              <b data-default-project>{words("default_project", saved, nameOf)}</b>
            )}
          </label>
          {changes && (
            <button type="submit" class="secondary small" data-default-project-save disabled={busy || draft === saved}>
              Save
            </button>
          )}
        </form>
      )}
      {read && !changes && <p class="dim">Its owners choose it.</p>}
      {err && <p class="control-error">{err}</p>}
    </Section>
  );
}

/** Project defaults: the table, its staged changes and the bar above it. */
function ProjectDefaults({ owner, name, projects, role }: { owner: string; name: string; projects: PlatformProject[]; role: string | undefined }) {
  const [layers, setLayers] = useState<Layers | null>(null);
  const [err, setErr] = useState("");
  const [staged, setStaged] = useState<Staged>({});
  const [busy, setBusy] = useState(false);
  const [failed, setFailed] = useState<{ write: Write; error: string }[]>([]);
  const [models, setModels] = useState<string[]>([]);
  const [spaces, setSpaces] = useState<Record<string, HostedWorkspace[]>>({});
  const ids = projects.map((p) => p.id);
  const key = ids.join();
  const load = async () => {
    const [own, ...each] = await Promise.all([
      call<EffectiveSetting[]>("GET", `/api/owners/${enc(owner)}/settings`),
      ...ids.map((id) => call<EffectiveSetting[]>("GET", `/api/projects/${enc(id)}/settings`).catch(() => [] as EffectiveSetting[])),
    ]);
    setLayers({ owner: own, projects: Object.fromEntries(ids.map((id, i) => [id, each[i]])) });
  };
  useEffect(() => {
    let live = true;
    load().catch((e: Error & { status?: number }) => live && setErr(e.status === 404 ? `You're not in ${name}.` : e.message));
    // The models agents may use, as a project's agent model lists them.
    if (ids[0]) call<SessionSettings>("GET", `/api/projects/${enc(ids[0])}/sessions/settings`).then((v) => live && setModels(v.models), () => {});
    // Each project's Cloud workspaces, for its agent workspace.
    void Promise.all(ids.map((id) => runs.workspaces(id))).then((ws) => live && setSpaces(Object.fromEntries(ids.map((id, i) => [id, ws[i]]))));
    return () => void (live = false);
  }, [owner, key]);
  const n = changes(staged);
  useHoldLeaving(n > 0);
  const writer = role !== undefined && role !== "viewer";
  const set = (scope: string, k: Key, v: Value) => layers && setStaged(stage(staged, layers, scope, k, v));
  const save = async () => {
    setBusy(true);
    const r = await send(writes(staged, owner, ids), (w) => call("PUT", w.path, w.body));
    setFailed(r.failed);
    try {
      await load();
    } catch (e) {
      setErr((e as Error).message);
    }
    setStaged(r.left);
    setBusy(false);
  };
  const cancel = () => {
    setStaged({});
    setFailed([]);
  };
  const workspaceName = (id: string) => Object.values(spaces).flat().find((w) => w.id === id)?.name;
  const projectName = (id: string) => session.project(id)?.name ?? id;
  const choices = (k: Key, project?: string): Value[] => {
    if (k === "issue_home") return ISSUE_HOMES;
    if (k === "agents_model") return models;
    if (k === "agents_workspace") return (project ? spaces[project] : [])?.map((w) => w.id) ?? [];
    return STALL_MINUTES;
  };
  const head = owner === "personal" ? "Your default" : "Team default";
  return (
    <Section
      id="defaults"
      title="Project defaults"
      lede={`What ${owner === "personal" ? "your projects use" : `every project in ${name} uses`} unless it overrides it.`}
      more={<p>A highlighted cell is a project's own value. Choose Default in it to follow the default again. Changes wait for Save.</p>}
      hooks={{ "data-project-defaults": owner }}
    >
      {err && <p class="control-error">{err}</p>}
      {layers && (
        <>
          {n > 0 && (
            <div class="defaults-bar" role="status" data-defaults-bar>
              <span data-defaults-count>{n === 1 ? "1 change" : `${n} changes`}</span>
              <button class="secondary small" disabled={busy} data-defaults-cancel onClick={cancel}>
                Cancel
              </button>
              <button class="primary small" disabled={busy} data-defaults-save onClick={() => void save()}>
                {busy ? "Saving…" : "Save"}
              </button>
            </div>
          )}
          {failed.length > 0 && (
            <div class="control-error" role="alert" data-defaults-failed>
              <p>Some changes weren't saved. They're still here to try again:</p>
              <ul>
                {failed.map((f) => (
                  <li key={`${f.write.scope}/${f.write.key}`}>
                    {f.write.scope === OWNER ? head : projectName(f.write.scope)}, {infoOf(f.write.key).name}: {f.error}
                  </li>
                ))}
              </ul>
            </div>
          )}
          {projects.length === 0 && <p class="dim">No projects yet: the defaults count for each one it makes.</p>}
          <div class="defaults-scroll">
            <table class="settings-table defaults-table" data-defaults-table>
              <thead>
                <tr>
                  <th scope="col">Setting</th>
                  <th scope="col">{head}</th>
                  {projects.map((p) => (
                    <th scope="col" key={p.id}>
                      {p.name}
                    </th>
                  ))}
                </tr>
              </thead>
              <tbody>
                {KEYS.map((info) => {
                  const own = ownerCell(layers, staged, info.key);
                  return (
                    <tr key={info.key} data-defaults-key={info.key}>
                      <th scope="row">{info.name}</th>
                      <td class={cellClass(own)} data-defaults-cell="owner">
                        {info.owner && writer ? (
                          <CellSelect
                            label={`${info.name}, ${head.toLowerCase()}`}
                            k={info.key}
                            cell={own}
                            first={`Built-in (${words(info.key, info.builtin)})`}
                            choices={choices(info.key)}
                            name={workspaceName}
                            disabled={busy}
                            set={(v) => set(OWNER, info.key, v)}
                          />
                        ) : (
                          <span class={info.owner ? undefined : "dim"} title={info.owner ? undefined : "Each project chooses its own."}>
                            {words(info.key, own.shown, workspaceName)}
                          </span>
                        )}
                      </td>
                      {projects.map((p) => {
                        const c = projectCell(layers, staged, p.id, info.key);
                        const below = defaultOf(layers, staged, info.key);
                        return (
                          <td key={p.id} class={cellClass(c)} data-defaults-cell={p.id}>
                            {writer ? (
                              <CellSelect
                                label={`${info.name}, ${p.name}`}
                                k={info.key}
                                cell={c}
                                first={`Default (${words(info.key, below, workspaceName)})`}
                                choices={choices(info.key, p.id)}
                                name={workspaceName}
                                disabled={busy}
                                set={(v) => set(p.id, info.key, v)}
                              />
                            ) : (
                              <span>{c.overridden ? words(info.key, c.shown, workspaceName) : `Default (${words(info.key, c.shown, workspaceName)})`}</span>
                            )}
                          </td>
                        );
                      })}
                    </tr>
                  );
                })}
              </tbody>
            </table>
          </div>
          {!writer && <p class="dim">{name}'s owners and editors change these.</p>}
        </>
      )}
    </Section>
  );
}

const cellClass = (c: Cell) => ["defaults-cell", c.overridden && "defaults-own", c.staged && "defaults-staged"].filter(Boolean).join(" ");

/** A cell's select: the layer below first (its own value cleared), then
 * each choice. */
function CellSelect({
  label,
  k,
  cell,
  first,
  choices,
  name,
  disabled,
  set,
}: {
  label: string;
  k: Key;
  cell: Cell;
  first: string;
  choices: Value[];
  name: (id: string) => string | undefined;
  disabled: boolean;
  set: (v: Value) => void;
}) {
  return (
    <select
      class="field"
      aria-label={label}
      title={cell.staged ? "Not saved yet" : undefined}
      value={optionOf(cell.own)}
      disabled={disabled}
      onChange={(e) => set(valueOf(k, e.currentTarget.value))}
    >
      <option value="">{first}</option>
      {choicesWith(choices, cell.own).map((v) => (
        <option key={String(v)} value={optionOf(v)}>
          {words(k, v, name)}
        </option>
      ))}
    </select>
  );
}
